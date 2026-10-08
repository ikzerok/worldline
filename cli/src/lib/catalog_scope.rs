//! 查询范围只读输出，全部语义由 core 的单次快照承担。
use super::*;
use worldline_core::catalog::TargetRef;
use worldline_core::relations::{RelationQueryOptions, RelationQueryResult};

pub(super) struct ScopeArgs {
    query: CatalogQueryArgs,
    focus: Option<TargetRef>,
    relation_options: RelationQueryOptions,
}
pub(super) fn parse_catalog_scope_args(args: &[String]) -> Result<ScopeArgs, String> {
    if args
        .iter()
        .try_fold(0usize, |sum, value| sum.checked_add(value.len()))
        .is_none_or(|size| size > 4 * 1024 * 1024)
    {
        return Err("查询范围参数超过4MiB预算".into());
    }
    let mut remaining = Vec::new();
    let mut focus = None;
    let mut options = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(key, value)| (key, Some(value)));
        if matches!(key, "--focus" | "--relation-options") {
            let value = inline
                .map(str::to_owned)
                .or_else(|| iter.next().cloned())
                .ok_or_else(|| format!("{key} 需要 JSON DTO"))?;
            let value = worldline_core::parse_unique_json(value.as_bytes())?;
            if key == "--focus" {
                if focus.is_some() {
                    return Err("--focus 只能提供一次".into());
                }
                focus = Some(serde_json::from_value(value).map_err(|e| format!("无效焦点：{e}"))?);
            } else {
                if options.is_some() {
                    return Err("--relation-options 只能提供一次".into());
                }
                options =
                    Some(serde_json::from_value(value).map_err(|e| format!("无效关系选项：{e}"))?);
            }
        } else {
            remaining.push(arg.clone());
        }
    }
    let query = super::catalog_query::parse_catalog_query_args(&remaining)?;
    if query.cursor_json.is_some() {
        return Err("catalog-scope 不接受旧查询游标；请使用完整冻结范围或 offset".into());
    }
    if focus.is_none() && options.is_some() {
        return Err("--relation-options 需要 --focus".into());
    }
    Ok(ScopeArgs {
        query,
        focus,
        relation_options: options.unwrap_or_default(),
    })
}
pub(super) fn cmd_catalog_scope(args: &ScopeArgs, out: &mut impl Write) -> Result<i32, String> {
    let query: CatalogQuery =
        match worldline_core::parse_unique_json(args.query.query_json.as_bytes())
            .and_then(|value| serde_json::from_value(value).map_err(|error| error.to_string()))
        {
            Ok(query) => query,
            Err(error) => return failure(args, "INVALID_QUERY", &error, out),
        };
    let project = match Project::open_read_only(&args.query.path) {
        Ok(project) => project,
        Err(error) => return failure(args, "IO_ERROR", &error, out),
    };
    let snapshot = match project.catalog_scope_snapshot(&query, args.query.options.max_candidates) {
        Ok(scope) => scope,
        Err(error) => return failure(args, error.code(), &error.to_string(), out),
    };
    let page = match snapshot
        .query()
        .page(args.query.options.offset, args.query.options.page_size)
    {
        Ok(page) => page,
        Err(error) => return failure(args, error.code(), &error.to_string(), out),
    };
    let relations = args
        .focus
        .as_ref()
        .map(|focus| snapshot.query_relations(focus, args.relation_options.clone()));
    if args.query.json {
        #[derive(serde::Serialize)]
        struct Response<'a> {
            ok: bool,
            schema_version: u32,
            workspace_revision: &'a str,
            scope: &'a worldline_core::catalog_scope::CatalogScopeSnapshot,
            page: &'a worldline_core::queries::CatalogSnapshotPage,
            relations: &'a Option<RelationQueryResult>,
        }
        let response = Response {
            ok: true,
            schema_version: 1,
            workspace_revision: &snapshot.query().snapshot,
            scope: &snapshot,
            page: &page,
            relations: &relations,
        };
        if !fits(&response, 32 * 1024 * 1024 - 1) {
            return failure(
                args,
                "BUDGET_EXCEEDED",
                "完整范围与结果页超过32MiB输出预算；未截断范围",
                out,
            );
        }
        serde_json::to_writer(&mut *out, &response).map_err(|e| e.to_string())?;
        writeln!(out).map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{} · {} 个完整命中 · {} 处匹配地图绑定 · {} 个无已知位置{}",
            snapshot.query().summary,
            snapshot.counts().matching_objects,
            snapshot.counts().matching_placements,
            snapshot.counts().unplaced_objects,
            if snapshot.maps_incomplete() || snapshot.query().incomplete {
                " · 来源不完整"
            } else {
                ""
            }
        )
        .map_err(|e| e.to_string())?;
        for item in &page.items {
            writeln!(
                out,
                "{}:{} · {} · {}:{}",
                item.target.kind, item.target.id, item.display, item.source.file, item.source.line
            )
            .map_err(|e| e.to_string())?;
            for placement in snapshot.placements_for(&item.target) {
                writeln!(
                    out,
                    "  {} / {} · {:?}{}{}",
                    placement.map_id,
                    placement.placement_id,
                    placement.kind,
                    if placement.visible { "" } else { " · 隐藏" },
                    if placement.locked { " · 锁定" } else { "" }
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if let Some(relations) = &relations {
            writeln!(
                out,
                "局部正式关系：{} 节点 / {} 边{}",
                relations.nodes.len(),
                relations.edges.len(),
                if relations.truncated {
                    " · 仍有续页"
                } else {
                    ""
                }
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(0)
}
fn failure(
    args: &ScopeArgs,
    code: &str,
    message: &str,
    out: &mut impl Write,
) -> Result<i32, String> {
    if args.query.json {
        writeln!(out,"{}",json!({"ok":false,"schema_version":1,"scope":null,"error":{"code":code,"message":message}})).map_err(|e|e.to_string())?;
    } else {
        writeln!(out, "查询范围失败 [{code}]：{message}").map_err(|e| e.to_string())?;
    }
    Ok(1)
}

fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("catalog_scope_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}

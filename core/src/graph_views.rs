//! 共享局部网络布局：单源读取、带基线展示事务与明确引用。
use crate::catalog::{Catalog, TargetRef};
use crate::presentation_commands::Revision;
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, valid_id};
use crate::{CompileResult, Diagnostic, RelationQueryDirection, RelationQueryOptions, Span};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphViewFilters {
    pub depth: u8,
    #[serde(default)]
    pub relation_types: Vec<String>,
    #[serde(default)]
    pub direction: RelationQueryDirection,
    #[serde(default = "default_nodes")]
    pub max_nodes: usize,
    #[serde(default = "default_edges")]
    pub max_edges: usize,
}
fn default_nodes() -> usize {
    250
}
fn default_edges() -> usize {
    500
}
impl Default for GraphViewFilters {
    fn default() -> Self {
        Self {
            depth: 1,
            relation_types: Vec::new(),
            direction: RelationQueryDirection::Both,
            max_nodes: default_nodes(),
            max_edges: default_edges(),
        }
    }
}
impl GraphViewFilters {
    pub fn query_options(&self, offset: usize) -> RelationQueryOptions {
        RelationQueryOptions {
            offset,
            depth: self.depth,
            relation_types: self.relation_types.clone(),
            direction: self.direction,
            max_nodes: self.max_nodes,
            max_edges: self.max_edges,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphViewDraft {
    pub id: String,
    pub title: String,
    pub focus: TargetRef,
    pub filters: GraphViewFilters,
    pub positions: BTreeMap<String, [f64; 2]>,
    pub hidden_relation_ids: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct GraphViewDocument {
    pub draft: GraphViewDraft,
    pub path: PathBuf,
    pub source: Value,
    pub source_hash: String,
    pub read_only: bool,
}
#[derive(Clone, Debug, Default)]
pub struct GraphViewIndex {
    pub views: BTreeMap<String, GraphViewDocument>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug, Serialize)]
pub struct GraphViewReference {
    pub view_id: String,
    pub file: String,
    pub field: String,
}
#[derive(Clone, Debug)]
pub struct GraphViewCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub original: Option<String>,
    pub draft: GraphViewDraft,
}
#[derive(Clone, Debug)]
pub struct GraphViewResult {
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
}

pub fn position_key(target: &TargetRef) -> String {
    format!("{}:{}", target.kind, target.id)
}
pub fn position_target(key: &str) -> Option<TargetRef> {
    let (kind, id) = key.split_once(':')?;
    (crate::catalog::TARGET_KINDS.contains(&kind) && !id.is_empty())
        .then(|| TargetRef::new(kind, id))
}

fn report(index: &mut GraphViewIndex, path: &std::path::Path, code: &'static str, message: String) {
    index.diagnostics.push(Diagnostic::error(
        code,
        &path.to_string_lossy(),
        Span::new(1, 1, 1),
        message,
    ));
}
fn validate_shape(draft: &GraphViewDraft) -> Result<(), String> {
    if !valid_id(&draft.id) || draft.title.trim().is_empty() {
        return Err("网络视图 ID 无效或标题为空".into());
    }
    if !crate::catalog::TARGET_KINDS.contains(&draft.focus.kind.as_str())
        || draft.focus.id.is_empty()
    {
        return Err("中心对象需要有效 kind 和非空 ID".into());
    }
    let filters = &draft.filters;
    if !(1..=2).contains(&filters.depth)
        || !(1..=250).contains(&filters.max_nodes)
        || !(1..=500).contains(&filters.max_edges)
    {
        return Err("网络查询上限必须为 1–2 层、1–250 节点、1–500 边".into());
    }
    if filters.relation_types.iter().any(|id| !valid_id(id))
        || draft.hidden_relation_ids.iter().any(|id| !valid_id(id))
    {
        return Err("网络视图中的关系类型或关系 ID 无效".into());
    }
    let unique: BTreeSet<_> = draft.hidden_relation_ids.iter().collect();
    if unique.len() != draft.hidden_relation_ids.len() {
        return Err("隐藏关系列表包含重复 ID".into());
    }
    if draft.positions.iter().any(|(key, point)| {
        position_target(key).is_none() || point.iter().any(|value| !value.is_finite())
    }) {
        return Err("网络坐标需要完整 kind:id 与两个有限数值".into());
    }
    Ok(())
}

fn references(draft: &GraphViewDraft) -> Vec<(String, TargetRef)> {
    let mut refs = vec![("focus".into(), draft.focus.clone())];
    refs.extend(
        draft.positions.keys().filter_map(|key| {
            position_target(key).map(|target| (format!("positions/{key}"), target))
        }),
    );
    refs.extend(draft.hidden_relation_ids.iter().map(|id| {
        (
            format!("hidden_relation_ids/{id}"),
            TargetRef::new("relation", id),
        )
    }));
    refs
}
fn missing_references(draft: &GraphViewDraft, catalog: &Catalog) -> Vec<String> {
    let mut missing: Vec<_> = references(draft)
        .into_iter()
        .filter(|(_, target)| catalog.object(target).is_none())
        .map(|(field, target)| format!("{field} → {}:{}", target.kind, target.id))
        .collect();
    missing.extend(
        draft
            .filters
            .relation_types
            .iter()
            .filter(|id| !catalog.relation_types.contains_key(*id))
            .map(|id| format!("filters/relation_types/{id}")),
    );
    missing
}

/// 只读取 Project 已注册缓冲；损坏、断链和未知能力分别报告，绝不覆盖原始字节。
pub fn build_graph_view_index(project: &Project, content: &CompileResult) -> GraphViewIndex {
    let mut index = GraphViewIndex::default();
    let manifest = manifest_path(&project.root);
    let Ok(document) = project.authoring_document(&manifest) else {
        return index;
    };
    if document.is_deleted() {
        return index;
    }
    let registry = parse_registry(&project.root, document.bytes());
    let mut path_counts = BTreeMap::<PathBuf, usize>::new();
    for path in registry.maps.values().chain(registry.graph_views.values()) {
        *path_counts.entry(path.clone()).or_default() += 1;
    }
    index.diagnostics.extend(registry.diagnostics);
    for (id, path) in registry.graph_views {
        if path_counts.get(&path).copied().unwrap_or_default() != 1 {
            report(
                &mut index,
                &path,
                "GRAPH001",
                "网络视图路径被多个展示注册项复用，不能安全读取或修改".into(),
            );
            continue;
        }
        let parsed = (|| -> Result<(GraphViewDraft, Value, bool), (&'static str, String)> {
            let document = project
                .authoring_document(&path)
                .map_err(|error| ("GRAPH001", error))?;
            if document.is_deleted() {
                return Err(("GRAPH001", "注册的网络视图已删除".into()));
            }
            let source = parse_unique_json(document.bytes())
                .map_err(|error| ("GRAPH001", format!("网络视图 JSON 无法解析：{error}")))?;
            if source.get("schema_version").and_then(Value::as_u64) != Some(1) {
                return Err(("GRAPH003", "网络视图版本不受支持，原文只读保留".into()));
            }
            let draft: GraphViewDraft = serde_json::from_value(source.clone())
                .map_err(|error| ("GRAPH001", format!("网络视图结构无效：{error}")))?;
            validate_shape(&draft).map_err(|error| ("GRAPH001", error))?;
            if id != draft.id {
                return Err(("GRAPH001", "网络视图 ID 与清单注册 ID 不一致".into()));
            }
            Ok((draft, source, document.is_read_only()))
        })();
        match parsed {
            Ok((draft, source, read_only)) => {
                if read_only {
                    report(
                        &mut index,
                        &path,
                        "GRAPH003",
                        "网络视图包含未知必需能力，原文只读保留".into(),
                    );
                }
                for missing in missing_references(&draft, &content.analysis.catalog) {
                    if content.has_errors() {
                        index.diagnostics.push(Diagnostic::warning(
                            "GRAPH004",
                            &path.to_string_lossy(),
                            Span::new(1, 1, 1),
                            format!("源码存在错误，网络视图引用暂时无法解析：{missing}"),
                        ));
                    } else {
                        report(
                            &mut index,
                            &path,
                            "GRAPH002",
                            format!("网络视图引用缺失：{missing}"),
                        );
                    }
                }
                index.views.insert(
                    id,
                    GraphViewDocument {
                        source_hash: project
                            .authoring_document(&path)
                            .map(|document| {
                                crate::presentation_commands::document_hash(document.bytes())
                            })
                            .unwrap_or_default(),
                        draft,
                        path,
                        source,
                        read_only,
                    },
                );
            }
            Err((code, error)) => report(&mut index, &path, code, error),
        }
    }
    crate::sort_diagnostics(&mut index.diagnostics);
    index
}
impl GraphViewIndex {
    pub fn references_to(&self, target: &TargetRef) -> Vec<GraphViewReference> {
        self.views
            .values()
            .flat_map(|view| {
                references(&view.draft)
                    .into_iter()
                    .filter(|(_, reference)| {
                        crate::deletion_content_references::affected_by_deletion(reference, target)
                    })
                    .map(|(field, _)| GraphViewReference {
                        view_id: view.draft.id.clone(),
                        file: view.path.to_string_lossy().into_owned(),
                        field,
                    })
            })
            .collect()
    }
}

/// 便捷入口；编辑器应复用现有内容快照，调用 apply_with_content。
pub fn apply(
    project: &mut Project,
    revision: &mut Revision,
    command: GraphViewCommand,
) -> Result<GraphViewResult, String> {
    let content = project.compile();
    apply_with_content(project, revision, command, &content)
}

/// 仅提交一个显式共享布局意图；任何失败均保留原工程与修订。
pub fn apply_with_content(
    project: &mut Project,
    revision: &mut Revision,
    command: GraphViewCommand,
    content: &CompileResult,
) -> Result<GraphViewResult, String> {
    if command.expected_revision != *revision
        || command.expected_baseline != project.content_baseline()
        || content.sources != project.sources()
        || content.options.language_version != project.language_version_kind()
    {
        return Err("StaleRevision：网络视图基线已过期，请保留输入并重新打开".into());
    }
    validate_shape(&command.draft)?;
    let manifest = manifest_path(&project.root);
    let document = project.authoring_document(&manifest)?;
    if document.is_read_only() || document.is_deleted() {
        return Err("网络视图需要可写的工作区清单".into());
    }
    let mut manifest_value = parse_unique_json(document.bytes()).map_err(|e| e.to_string())?;
    let registry = parse_registry(&project.root, document.bytes());
    if !registry.diagnostics.is_empty() {
        return Err("工作区清单诊断未修复，不能保存网络视图".into());
    }
    let index = build_graph_view_index(project, content);
    let old = match command.original.as_deref() {
        Some(id) if id == command.draft.id => {
            Some(index.views.get(id).ok_or("待修改的网络视图无法安全读取")?)
        }
        Some(_) => return Err("网络视图 ID 是稳定身份，不能在布局保存中改名".into()),
        None => None,
    };
    if old.is_some_and(|view| view.read_only) {
        return Err("网络视图为只读，不能覆盖".into());
    }
    if old.is_none() && registry.graph_views.contains_key(&command.draft.id) {
        return Err("网络视图 ID 已注册，不能覆盖".into());
    }
    let missing = missing_references(&command.draft, &content.analysis.catalog);
    let old_missing = old
        .map(|view| missing_references(&view.draft, &content.analysis.catalog))
        .unwrap_or_default();
    if missing.iter().any(|item| !old_missing.contains(item)) {
        return Err(format!(
            "MissingReference：网络视图引用不存在：{}",
            missing.join("、")
        ));
    }
    if content.has_errors() {
        let old_refs = old.map(|view| references(&view.draft)).unwrap_or_default();
        if references(&command.draft)
            .iter()
            .any(|item| !old_refs.contains(item))
            || old.is_none_or(|view| {
                view.draft.filters.relation_types != command.draft.filters.relation_types
            })
        {
            return Err("UnresolvedReference：源码诊断未修复，不能增加无法确认的网络引用".into());
        }
    }
    let path = old.map(|view| view.path.clone()).unwrap_or_else(|| {
        project
            .root
            .join(format!(".world/graph-views/{}.json", command.draft.id))
    });
    crate::file_access::within(&project.root, &path)?;
    if old.is_none() && registry.documents.contains_key(&path) {
        return Err("网络视图目标已被其他文档注册".into());
    }
    let mut source = old
        .map(|view| view.source.clone())
        .unwrap_or_else(|| json!({"schema_version":1}));
    let fresh = serde_json::to_value(&command.draft).map_err(|e| e.to_string())?;
    // 只覆盖已知字段；focus/filters 的未知子字段也必须保留。
    for (key, value) in fresh.as_object().ok_or("网络视图序列化无效")? {
        if (key == "focus" || key == "filters") && source.get(key).is_some_and(Value::is_object) {
            for (nested_key, nested_value) in value.as_object().ok_or("网络视图字段无效")? {
                source[key][nested_key] = nested_value.clone();
            }
        } else {
            source[key] = value.clone();
        }
    }
    let bytes = serde_json::to_vec_pretty(&source).map_err(|e| e.to_string())?;
    let mut candidate = project.clone();
    let mut changed_files = Vec::new();
    if old.is_none() {
        let relative = path
            .strip_prefix(&project.root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if manifest_value.get("graph_views").is_none() {
            manifest_value["graph_views"] = json!({});
        }
        manifest_value["graph_views"][&command.draft.id] = json!(relative);
        if manifest_value.get("required_features").is_none() {
            manifest_value["required_features"] = json!([]);
        }
        let features = manifest_value["required_features"]
            .as_array_mut()
            .ok_or("必需能力清单格式无效")?;
        if !features
            .iter()
            .any(|item| item.as_str() == Some("presentation.graph_views.v1"))
        {
            features.push(json!("presentation.graph_views.v1"));
        }
        candidate.set_authoring_document(
            &manifest,
            serde_json::to_vec_pretty(&manifest_value).map_err(|e| e.to_string())?,
        )?;
        candidate.create_authoring_document(&path, bytes)?;
        changed_files.push(manifest);
    } else {
        candidate.set_authoring_document(&path, bytes)?;
    }
    changed_files.push(path);
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(GraphViewResult {
        changed_files,
        new_revision: *revision,
    })
}

#[derive(Clone, Debug)]
pub struct DeleteGraphViewCommand {
    pub id: String,
    pub expected_revision: Revision,
    pub expected_baseline: String,
}

/// 删除共享布局及其注册项；不删除任何实体、关系或其他展示文档。
pub fn remove(
    project: &mut Project,
    revision: &mut Revision,
    command: DeleteGraphViewCommand,
) -> Result<GraphViewResult, String> {
    if command.expected_revision != *revision
        || command.expected_baseline != project.content_baseline()
    {
        return Err("StaleRevision：删除布局的基线已过期，请重新检查".into());
    }
    let manifest = manifest_path(&project.root);
    let bytes = project.authoring_document(&manifest)?.bytes();
    let registry = parse_registry(&project.root, bytes);
    if !registry.diagnostics.is_empty() {
        return Err("清单诊断未修复，不能删除共享布局".into());
    }
    let path = registry
        .graph_views
        .get(&command.id)
        .cloned()
        .ok_or("共享布局未注册")?;
    let aliases = registry
        .maps
        .values()
        .chain(registry.graph_views.values())
        .filter(|other| *other == &path)
        .count();
    if aliases != 1 {
        return Err("共享布局路径被多个注册项使用，不能删除".into());
    }
    let document = project.authoring_document(&path)?;
    if document.is_read_only() || document.is_deleted() {
        return Err("共享布局已删除或为只读".into());
    }
    let mut value = parse_unique_json(bytes).map_err(|error| error.to_string())?;
    value
        .get_mut("graph_views")
        .and_then(Value::as_object_mut)
        .ok_or("网络注册表无效")?
        .remove(&command.id);
    let mut candidate = project.clone();
    candidate.delete_authoring_document(&path)?;
    candidate.set_authoring_document(
        &manifest,
        serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )?;
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(GraphViewResult {
        changed_files: vec![manifest, path],
        new_revision: *revision,
    })
}

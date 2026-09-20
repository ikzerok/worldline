//! wl 命令行库:check / play / graph / timeline / catalog 的实现。
//! 以库形式暴露,供 main.rs 与集成测试共用。
//! `--json` 机器输出契约见 `worldline/spec/agent-protocol.md`。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use worldline_core::ast::PropertyValue;
use worldline_core::authoring::EntityDraft;
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::{
    compile_path, compile_path_with_options, CompileOptions, CompileResult, Diagnostic,
    LanguageVersion, RelationDirection, RelationDraft, RelationQueryDirection,
    RelationQueryOptions, RelationTypeDraft, Severity,
};
use worldline_runtime::{Output, Story};

/// 单个故事文件的公共参数。
struct FileArgs {
    path: PathBuf,
    json: bool,
    load: Option<PathBuf>,
    save: Option<PathBuf>,
    language_version: Option<LanguageVersion>,
}

struct CatalogArgs {
    file: FileArgs,
    tag: Option<String>,
    recursive: bool,
    kind: Option<String>,
}

struct WorkspaceArgs {
    path: PathBuf,
    json: bool,
}

struct MapsArgs {
    path: PathBuf,
    json: bool,
}

struct RelationsArgs {
    offset: usize,
    path: PathBuf,
    target: TargetRef,
    depth: u8,
    direction: RelationQueryDirection,
    relation_type: Option<String>,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationEditOperation {
    Create,
    Update,
    Delete,
}

struct RelationTypeEditArgs {
    path: PathBuf,
    operation: RelationEditOperation,
    id: String,
    display: Option<String>,
    inverse_display: Option<String>,
    clear_inverse_display: bool,
    direction: Option<RelationDirection>,
    from_kind: Option<String>,
    clear_from_kind: bool,
    to_kind: Option<String>,
    clear_to_kind: bool,
    baseline: Option<String>,
    json: bool,
}

struct RelationEditArgs {
    path: PathBuf,
    operation: RelationEditOperation,
    id: String,
    relation_type: Option<String>,
    from: Option<TargetRef>,
    to: Option<TargetRef>,
    description: Option<String>,
    source_note: Option<String>,
    clear_source_note: bool,
    scope_refs: Vec<TargetRef>,
    clear_scope_refs: bool,
    properties: Vec<(String, PropertyValue)>,
    clear_properties: bool,
    baseline: Option<String>,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromotionOperation {
    Preview,
    Commit,
}

struct PromotionArgs {
    path: PathBuf,
    operation: PromotionOperation,
    source: TargetRef,
    target: TargetRef,
    label: String,
    occurrence: u32,
    relation_id: String,
    relation_type: String,
    description: Option<String>,
    source_note: Option<String>,
    scope_refs: Vec<TargetRef>,
    properties: Vec<(String, PropertyValue)>,
    baseline: Option<String>,
    json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityOperation {
    Create,
    Update,
    Delete,
}

struct EntityArgs {
    path: PathBuf,
    operation: EntityOperation,
    id: Option<String>,
    entity_type: Option<String>,
    display: Option<String>,
    description: Option<String>,
    properties: Vec<(String, PropertyValue)>,
    baseline: Option<String>,
    json: bool,
}

struct CompileSnapshot {
    result: CompileResult,
    workspace_diagnostics: Vec<Diagnostic>,
    read_only: bool,
}

impl CompileSnapshot {
    fn plain(result: CompileResult) -> Self {
        Self {
            result,
            workspace_diagnostics: Vec::new(),
            read_only: false,
        }
    }
}

/// 返回值 = 进程退出码。
pub fn run(args: &[String], out: &mut impl Write, input: &mut impl BufRead) -> Result<i32, String> {
    let Some(cmd) = args.first() else {
        return Err("缺少子命令".into());
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "workspace" => cmd_workspace(&parse_workspace_args(rest)?, out),
        "maps" => cmd_maps(&parse_maps_args(rest)?, out),
        "relations" => {
            if rest.first().is_some_and(|arg| arg == "promote") {
                cmd_promotion(&parse_promotion_args(rest)?, out)
            } else {
                cmd_relations(&parse_relations_args(rest)?, out)
            }
        }
        "relation" => cmd_relation_edit(&parse_relation_edit_args(rest)?, out),
        "relation-type" | "relation_type" => {
            cmd_relation_type_edit(&parse_relation_type_edit_args(rest)?, out)
        }
        "check" => {
            let f = parse_file_args(cmd, rest, false)?;
            cmd_check(&f, out)
        }
        "graph" => {
            let f = parse_file_args(cmd, rest, false)?;
            cmd_graph(&f, out)
        }
        "timeline" => {
            let f = parse_file_args(cmd, rest, false)?;
            cmd_timeline(&f, out)
        }
        "catalog" => cmd_catalog(&parse_catalog_args(rest)?, out),
        "entity" => cmd_entity(&parse_entity_args(rest)?, out),
        "play" => {
            let f = parse_file_args(cmd, rest, true)?;
            cmd_play(&f, out, input)
        }
        other => Err(format!(
            "未知子命令 `{other}`(可用:workspace / maps / relations / relation / relation-type / check / play / graph / timeline / catalog / entity)"
        )),
    }
}

fn parse_workspace_args(args: &[String]) -> Result<WorkspaceArgs, String> {
    let operation = args.first().ok_or("workspace 需要 check 和目录")?;
    if operation != "check" {
        return Err(format!("未知 workspace 操作 `{operation}`(可用: check)"));
    }
    let mut path = None;
    let mut json = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json = true,
            value if value.starts_with("--") => return Err(format!("未知参数 {value}")),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("workspace check 只能提供一个目录".into());
                }
            }
        }
    }
    Ok(WorkspaceArgs {
        path: path.ok_or("workspace check 需要一个目录")?,
        json,
    })
}

fn parse_maps_args(args: &[String]) -> Result<MapsArgs, String> {
    let operation = args.first().ok_or("maps 需要 list 和目录")?;
    if operation != "list" {
        return Err(format!("未知 maps 操作 `{operation}`(可用: list)"));
    }
    let mut path = None;
    let mut json = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json = true,
            value if value.starts_with("--") => return Err(format!("未知参数 {value}")),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("maps list 只能提供一个目录".into());
                }
            }
        }
    }
    Ok(MapsArgs {
        path: path.ok_or("maps list 需要一个目录")?,
        json,
    })
}

fn parse_relations_args(args: &[String]) -> Result<RelationsArgs, String> {
    let mut path = None;
    let mut target = None;
    let mut depth = 1;
    let mut offset = 0;
    let mut direction = RelationQueryDirection::Both;
    let mut relation_type = None;
    let mut json = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" => {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                json = true;
            }
            "--target" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--target` 需要 KIND:ID")?;
                if target.replace(parse_target_ref(&value)?).is_some() {
                    return Err("参数 `--target` 只能提供一次".into());
                }
            }
            "--offset" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--offset` 需要非负整数")?;
                offset = value
                    .parse::<usize>()
                    .map_err(|_| "参数 `--offset` 需要非负整数")?;
            }
            "--depth" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--depth` 需要 1 或 2")?;
                depth = value
                    .parse::<u8>()
                    .map_err(|_| "参数 `--depth` 需要 1 或 2".to_string())?;
                if !matches!(depth, 1 | 2) {
                    return Err("参数 `--depth` 只能是 1 或 2".into());
                }
            }
            "--direction" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--direction` 需要 outgoing / incoming / both")?;
                direction = match value.as_str() {
                    "outgoing" => RelationQueryDirection::Outgoing,
                    "incoming" => RelationQueryDirection::Incoming,
                    "both" => RelationQueryDirection::Both,
                    _ => return Err("参数 `--direction` 需要 outgoing / incoming / both".into()),
                };
            }
            "--type" | "--relation-type" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--type` 需要关系类型 ID")?;
                if value.trim().is_empty() {
                    return Err("参数 `--type` 不能为空".into());
                }
                if relation_type.replace(value).is_some() {
                    return Err("参数 `--type` 只能提供一次".into());
                }
            }
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("relations 只能提供一个目录".into());
                }
            }
        }
    }
    Ok(RelationsArgs {
        offset,
        path: path.ok_or("relations 需要一个目录")?,
        target: target.ok_or("relations 需要 --target KIND:ID")?,
        depth,
        direction,
        relation_type,
        json,
    })
}

fn parse_target_ref(value: &str) -> Result<TargetRef, String> {
    let (kind, id) = value.split_once(':').ok_or("--target 格式必须为 KIND:ID")?;
    let kind = kind.trim();
    let id = id.trim();
    if kind.is_empty() || id.is_empty() || (kind != "file" && id.contains(':')) {
        return Err("--target 格式必须为非空 KIND:ID".into());
    }
    Ok(TargetRef::new(kind, id))
}

fn parse_relation_edit_operation(value: &str) -> Result<RelationEditOperation, String> {
    match value {
        "create" => Ok(RelationEditOperation::Create),
        "update" => Ok(RelationEditOperation::Update),
        "delete" => Ok(RelationEditOperation::Delete),
        other => Err(format!(
            "未知 relation 操作 `{other}`(可用: create / update / delete)"
        )),
    }
}

fn relation_flag_value<'a>(
    key: &str,
    inline: Option<&str>,
    args: &mut impl Iterator<Item = &'a String>,
) -> Result<String, String> {
    let value = inline
        .map(str::to_string)
        .or_else(|| args.next().cloned())
        .ok_or_else(|| format!("参数 `{key}` 需要一个值"))?;
    if value.trim().is_empty() {
        return Err(format!("参数 `{key}` 不能为空"));
    }
    Ok(value)
}

fn relation_clear_flag(key: &str, inline: Option<&str>) -> Result<(), String> {
    if inline.is_some() {
        return Err(format!("参数 `{key}` 不接受值"));
    }
    Ok(())
}

fn parse_relation_type_edit_args(args: &[String]) -> Result<RelationTypeEditArgs, String> {
    let mut iter = args.iter();
    let operation = parse_relation_edit_operation(
        iter.next()
            .ok_or("relation-type 需要 create / update / delete 和目录")?,
    )?;
    let path = PathBuf::from(iter.next().ok_or("relation-type 操作需要一个目录或入口")?);
    let mut id = None;
    let mut display = None;
    let mut inverse_display = None;
    let mut clear_inverse_display = false;
    let mut direction = None;
    let mut from_kind = None;
    let mut clear_from_kind = false;
    let mut to_kind = None;
    let mut clear_to_kind = false;
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" => {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                json = true;
            }
            "--id" => id = Some(relation_flag_value(key, inline, &mut iter)?),
            "--display" => display = Some(relation_flag_value(key, inline, &mut iter)?),
            "--inverse" | "--inverse-display" => {
                inverse_display = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--clear-inverse-display" => {
                relation_clear_flag(key, inline)?;
                clear_inverse_display = true;
            }
            "--direction" => {
                direction = Some(
                    match relation_flag_value(key, inline, &mut iter)?.as_str() {
                        "directed" => RelationDirection::Directed,
                        "undirected" => RelationDirection::Undirected,
                        _ => return Err("参数 `--direction` 需要 directed / undirected".into()),
                    },
                );
            }
            "--from-kind" => from_kind = Some(relation_flag_value(key, inline, &mut iter)?),
            "--clear-from-kind" => {
                relation_clear_flag(key, inline)?;
                clear_from_kind = true;
            }
            "--to-kind" => to_kind = Some(relation_flag_value(key, inline, &mut iter)?),
            "--clear-to-kind" => {
                relation_clear_flag(key, inline)?;
                clear_to_kind = true;
            }
            "--baseline" => baseline = Some(relation_flag_value(key, inline, &mut iter)?),
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => return Err(format!("未知 relation-type 参数 `{value}`")),
        }
    }
    if operation != RelationEditOperation::Update
        && (clear_inverse_display || clear_from_kind || clear_to_kind)
    {
        return Err("relation-type 的 --clear-* 参数只能用于 update".into());
    }
    if (clear_inverse_display && inverse_display.is_some())
        || (clear_from_kind && from_kind.is_some())
        || (clear_to_kind && to_kind.is_some())
    {
        return Err("同一关系类型字段不能同时设置和清空".into());
    }
    Ok(RelationTypeEditArgs {
        path,
        operation,
        id: id.ok_or("relation-type 操作需要 --id")?,
        display,
        inverse_display,
        clear_inverse_display,
        direction,
        from_kind,
        clear_from_kind,
        to_kind,
        clear_to_kind,
        baseline,
        json,
    })
}

fn parse_relation_edit_args(args: &[String]) -> Result<RelationEditArgs, String> {
    let mut iter = args.iter();
    let operation = parse_relation_edit_operation(
        iter.next()
            .ok_or("relation 需要 create / update / delete 和目录")?,
    )?;
    let path = PathBuf::from(iter.next().ok_or("relation 操作需要一个目录或入口")?);
    let mut id = None;
    let mut relation_type = None;
    let mut from = None;
    let mut to = None;
    let mut description = None;
    let mut source_note = None;
    let mut clear_source_note = false;
    let mut scope_refs = Vec::new();
    let mut clear_scope_refs = false;
    let mut properties = Vec::new();
    let mut clear_properties = false;
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" => {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                json = true;
            }
            "--id" => id = Some(relation_flag_value(key, inline, &mut iter)?),
            "--type" | "--relation-type" => {
                relation_type = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--from" => {
                from = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--to" => {
                to = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--description" => description = Some(relation_flag_value(key, inline, &mut iter)?),
            "--source-note" => source_note = Some(relation_flag_value(key, inline, &mut iter)?),
            "--clear-source-note" => {
                relation_clear_flag(key, inline)?;
                clear_source_note = true;
            }
            "--scope" | "--scope-ref" => scope_refs.push(parse_target_ref(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--clear-scope" => {
                relation_clear_flag(key, inline)?;
                clear_scope_refs = true;
            }
            "--property" => properties.push(parse_property_arg(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--clear-properties" => {
                relation_clear_flag(key, inline)?;
                clear_properties = true;
            }
            "--baseline" => baseline = Some(relation_flag_value(key, inline, &mut iter)?),
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => return Err(format!("未知 relation 参数 `{value}`")),
        }
    }
    if operation != RelationEditOperation::Update
        && (clear_source_note || clear_scope_refs || clear_properties)
    {
        return Err("relation 的 --clear-* 参数只能用于 update".into());
    }
    if (clear_source_note && source_note.is_some())
        || (clear_scope_refs && !scope_refs.is_empty())
        || (clear_properties && !properties.is_empty())
    {
        return Err("同一关系字段不能同时设置和清空".into());
    }
    Ok(RelationEditArgs {
        path,
        operation,
        id: id.ok_or("relation 操作需要 --id")?,
        relation_type,
        from,
        to,
        description,
        source_note,
        clear_source_note,
        scope_refs,
        clear_scope_refs,
        properties,
        clear_properties,
        baseline,
        json,
    })
}

fn parse_promotion_args(args: &[String]) -> Result<PromotionArgs, String> {
    let mut iter = args.iter();
    if iter.next().map(String::as_str) != Some("promote") {
        return Err("relations promote 需要 preview 或 commit".into());
    }
    let operation = match iter.next().map(String::as_str) {
        Some("preview") => PromotionOperation::Preview,
        Some("commit") => PromotionOperation::Commit,
        Some(other) => {
            return Err(format!(
                "未知 promotion 操作 `{other}`(可用: preview / commit)"
            ))
        }
        None => return Err("relations promote 需要 preview 或 commit".into()),
    };
    let path = PathBuf::from(iter.next().ok_or("relations promote 操作需要一个目录")?);
    let mut source = None;
    let mut target = None;
    let mut label = None;
    let mut occurrence = 1;
    let mut relation_id = None;
    let mut relation_type = None;
    let mut description = None;
    let mut source_note = None;
    let mut scope_refs = Vec::new();
    let mut properties = Vec::new();
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" => {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                json = true;
            }
            "--source" => {
                source = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--target" => {
                target = Some(parse_target_ref(&relation_flag_value(
                    key, inline, &mut iter,
                )?)?)
            }
            "--label" => label = Some(relation_flag_value(key, inline, &mut iter)?),
            "--occurrence" => {
                occurrence = relation_flag_value(key, inline, &mut iter)?
                    .parse::<u32>()
                    .map_err(|_| "参数 `--occurrence` 需要正整数".to_string())?;
                if occurrence == 0 {
                    return Err("参数 `--occurrence` 需要正整数".into());
                }
            }
            "--id" | "--relation-id" => {
                relation_id = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--type" | "--relation-type" => {
                relation_type = Some(relation_flag_value(key, inline, &mut iter)?)
            }
            "--description" => description = Some(relation_flag_value(key, inline, &mut iter)?),
            "--source-note" => source_note = Some(relation_flag_value(key, inline, &mut iter)?),
            "--scope" | "--scope-ref" => scope_refs.push(parse_target_ref(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--property" => properties.push(parse_property_arg(&relation_flag_value(
                key, inline, &mut iter,
            )?)?),
            "--baseline" => baseline = Some(relation_flag_value(key, inline, &mut iter)?),
            key if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            value => return Err(format!("未知 promotion 参数 `{value}`")),
        }
    }
    Ok(PromotionArgs {
        path,
        operation,
        source: source.ok_or("promotion 操作需要 --source KIND:ID")?,
        target: target.ok_or("promotion 操作需要 --target KIND:ID")?,
        label: label.ok_or("promotion 操作需要 --label 标签")?,
        occurrence,
        relation_id: relation_id.ok_or("promotion 操作需要 --id RELATION_ID")?,
        relation_type: relation_type.ok_or("promotion 操作需要 --type TYPE")?,
        description,
        source_note,
        scope_refs,
        properties,
        baseline,
        json,
    })
}

/// 解析 `<文件.wl> [--json]`;`--load=`/`--save=` 仅 play 接受。
fn parse_file_args(cmd: &str, args: &[String], session_flags: bool) -> Result<FileArgs, String> {
    let mut path: Option<PathBuf> = None;
    let mut json = false;
    let mut load = None;
    let mut save = None;
    let mut language_version = None;
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--json" => json = true,
            other if session_flags && other.starts_with("--load=") => {
                load = Some(PathBuf::from(other.trim_start_matches("--load=")));
            }
            other if session_flags && other.starts_with("--save=") => {
                save = Some(PathBuf::from(other.trim_start_matches("--save=")));
            }
            "--language-version" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "参数 `--language-version` 需要一个值".to_string())?;
                language_version = Some(parse_language_version(value)?);
            }
            other if other.starts_with("--language-version=") => {
                language_version = Some(parse_language_version(
                    other.trim_start_matches("--language-version="),
                )?);
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => {
                if path.replace(PathBuf::from(other)).is_some() {
                    return Err("只能提供一个故事文件".into());
                }
            }
        }
    }
    let Some(path) = path else {
        return Err(format!("子命令 `{cmd}` 需要一个 .wl 故事文件"));
    };
    Ok(FileArgs {
        path,
        json,
        load,
        save,
        language_version,
    })
}

fn parse_catalog_args(args: &[String]) -> Result<CatalogArgs, String> {
    let mut file_args = Vec::new();
    let mut tag = None;
    let mut kind = None;
    let mut recursive = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tag" | "--kind" => {
                let value = args
                    .next()
                    .filter(|v| !v.trim().is_empty() && !v.starts_with("--"))
                    .ok_or_else(|| format!("参数 `{arg}` 需要一个值"))?;
                let filter = if arg == "--tag" { &mut tag } else { &mut kind };
                if filter.replace(value.clone()).is_some() {
                    return Err(format!("参数 `{arg}` 只能提供一次"));
                }
            }
            "--recursive" => recursive = true,
            _ => file_args.push(arg.clone()),
        }
    }
    Ok(CatalogArgs {
        file: parse_file_args("catalog", &file_args, false)?,
        tag,
        recursive,
        kind,
    })
}

fn parse_entity_args(args: &[String]) -> Result<EntityArgs, String> {
    let mut args = args.iter();
    let operation = match args
        .next()
        .ok_or("entity 需要 create / update / delete 和目录或入口")?
        .as_str()
    {
        "create" => EntityOperation::Create,
        "update" => EntityOperation::Update,
        "delete" => EntityOperation::Delete,
        other => return Err(format!("未知 entity 操作 `{other}`")),
    };
    let path = PathBuf::from(args.next().ok_or("entity 操作需要一个目录或入口")?);
    let mut id = None;
    let mut entity_type = None;
    let mut display = None;
    let mut description = None;
    let mut properties = Vec::new();
    let mut baseline = None;
    let mut json = false;
    while let Some(arg) = args.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" => {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                json = true;
            }
            "--id" => id = Some(entity_value(key, inline, &mut args)?),
            "--kind" => {
                entity_type = Some(entity_value(key, inline, &mut args)?);
            }
            "--display" => display = Some(entity_value(key, inline, &mut args)?),
            "--description" => description = Some(entity_value(key, inline, &mut args)?),
            "--baseline" => baseline = Some(entity_value(key, inline, &mut args)?),
            "--property" => {
                let value = entity_value(key, inline, &mut args)?;
                properties.push(parse_property_arg(&value)?);
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => return Err(format!("未知 entity 参数 `{other}`")),
        }
    }
    Ok(EntityArgs {
        path,
        operation,
        id,
        entity_type,
        display,
        description,
        properties,
        baseline,
        json,
    })
}

fn entity_value<'a>(
    key: &str,
    inline: Option<&str>,
    args: &mut impl Iterator<Item = &'a String>,
) -> Result<String, String> {
    let value = inline
        .map(str::to_string)
        .or_else(|| args.next().cloned())
        .ok_or_else(|| format!("参数 `{key}` 需要一个值"))?;
    if value.trim().is_empty() {
        return Err(format!("参数 `{key}` 不能为空"));
    }
    Ok(value)
}

fn parse_property_arg(value: &str) -> Result<(String, PropertyValue), String> {
    let (name, raw) = value
        .split_once('=')
        .ok_or("--property 格式必须为 name=value")?;
    let name = name.trim();
    if name.is_empty() {
        return Err("property 名称不能为空".into());
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("property 值不能为空".into());
    }
    let parsed = serde_json::from_str::<serde_json::Value>(raw)
        .unwrap_or_else(|_| serde_json::Value::String(raw.trim_matches('"').to_string()));
    let property = match parsed {
        serde_json::Value::String(value) => PropertyValue::Str(value),
        serde_json::Value::Number(value) => {
            PropertyValue::Num(value.as_f64().ok_or("property 数值必须是有限数值")?)
        }
        serde_json::Value::Bool(value) => PropertyValue::Bool(value),
        _ => return Err("property 值只能是字符串、数值或布尔值".into()),
    };
    Ok((name.to_string(), property))
}

fn parse_language_version(value: &str) -> Result<LanguageVersion, String> {
    match value {
        "1.9" => Ok(LanguageVersion::V1_9),
        "1.10" => Ok(LanguageVersion::V1_10),
        _ => Err(format!("不支持的语言版本 `{value}`(可用: 1.9 / 1.10)")),
    }
}

fn compile_input(
    path: &Path,
    language_version: Option<LanguageVersion>,
) -> std::io::Result<CompileSnapshot> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let has_manifest = root.join(".world").join("project.json").is_file();
    if let Some(version) = language_version {
        let result = compile_path_with_options(path, CompileOptions::new(version))?;
        if has_manifest {
            let project = Project::open(path).map_err(std::io::Error::other)?;
            let workspace_diagnostics = project.authoring_diagnostics().to_vec();
            return Ok(CompileSnapshot {
                result,
                read_only: !workspace_diagnostics.is_empty(),
                workspace_diagnostics,
            });
        }
        return Ok(CompileSnapshot::plain(result));
    }
    // 工程清单是目录模式的唯一隐式版本来源。单文件旧调用继续走
    // compile_path，避免把相邻目录的清单意外应用到独立入口。
    if has_manifest {
        let mut project = Project::open(path).map_err(std::io::Error::other)?;
        let result = project.compile();
        let workspace_diagnostics = project.authoring_diagnostics().to_vec();
        return Ok(CompileSnapshot {
            result,
            read_only: !workspace_diagnostics.is_empty(),
            workspace_diagnostics,
        });
    }
    compile_path(path).map(CompileSnapshot::plain)
}

fn compile_or_fail(
    path: &Path,
    language_version: Option<LanguageVersion>,
    out: &mut impl Write,
) -> Option<CompileSnapshot> {
    match compile_input(path, language_version) {
        Ok(r) => Some(r),
        Err(e) => {
            let _ = writeln!(out, "wl: 无法读取 {}: {e}", path.display());
            None
        }
    }
}

struct WorkspaceSnapshot {
    result: CompileResult,
    map_index: worldline_core::MapIndex,
    workspace_diagnostics: Vec<Diagnostic>,
    baseline: String,
}

fn open_workspace_snapshot(path: &Path) -> Result<WorkspaceSnapshot, String> {
    let mut project = Project::open(path)?;
    let result = project.compile();
    let map_index = project.map_index();
    let mut workspace_diagnostics = project.authoring_diagnostics().to_vec();
    for diagnostic in &map_index.diagnostics {
        if !workspace_diagnostics.iter().any(|existing| {
            existing.severity == diagnostic.severity
                && existing.code == diagnostic.code
                && existing.message == diagnostic.message
                && existing.file == diagnostic.file
                && existing.span == diagnostic.span
        }) {
            workspace_diagnostics.push(diagnostic.clone());
        }
    }
    Ok(WorkspaceSnapshot {
        baseline: project.content_baseline(),
        result,
        map_index,
        workspace_diagnostics,
    })
}

fn query_payload_base(snapshot: &WorkspaceSnapshot) -> serde_json::Map<String, Value> {
    let read_only = !snapshot.workspace_diagnostics.is_empty();
    serde_json::Map::from_iter([
        ("schema_version".into(), json!(1)),
        (
            "language_version".into(),
            json!(snapshot.result.options.language_version.as_str()),
        ),
        ("workspace_revision".into(), json!(snapshot.baseline)),
        ("diagnostics".into(), json!(snapshot.result.diagnostics)),
        (
            "workspace_diagnostics".into(),
            json!(snapshot.workspace_diagnostics),
        ),
        ("read_only".into(), json!(read_only)),
        ("truncated".into(), json!(false)),
        ("continuation".into(), Value::Null),
    ])
}

fn write_query_failure(
    path: &Path,
    json_mode: bool,
    error: &str,
    out: &mut impl Write,
) -> Result<i32, String> {
    write_query_error(path, json_mode, "IO_ERROR", error, out, 2)
}

fn write_query_error(
    path: &Path,
    json_mode: bool,
    code: &str,
    error: &str,
    out: &mut impl Write,
    exit_code: i32,
) -> Result<i32, String> {
    if json_mode {
        let payload = json!({
            "ok": false,
            "schema_version": 1,
            "error": {"code": code, "message": error},
            "diagnostics": [],
            "workspace_diagnostics": [],
            "read_only": false,
            "truncated": false,
            "continuation": Value::Null,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{code}: {error}").map_err(|e| e.to_string())?;
    }
    let _ = path;
    Ok(exit_code)
}

fn cmd_workspace(args: &WorkspaceArgs, out: &mut impl Write) -> Result<i32, String> {
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(error) => return write_query_failure(&args.path, args.json, &error, out),
    };
    let read_only = !snapshot.workspace_diagnostics.is_empty();
    let ok = !snapshot.result.has_errors() && !read_only;
    if args.json {
        let mut payload = query_payload_base(&snapshot);
        payload.insert("ok".into(), json!(ok));
        payload.insert("stats".into(), json!(snapshot.result.analysis.stats));
        writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
    } else {
        let stats = snapshot.result.analysis.stats;
        writeln!(
            out,
            "{}: {} 线 / {} 角色 / {} 事件 / {} 场景 / {} 选择 / {} 字",
            args.path.display(),
            stats.storylines,
            stats.characters,
            stats.events,
            stats.scenes,
            stats.choices,
            stats.words,
        )
        .map_err(|e| e.to_string())?;
        for diagnostic in snapshot
            .result
            .diagnostics
            .iter()
            .chain(snapshot.workspace_diagnostics.iter())
        {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
        if read_only {
            writeln!(out, "工作区只读：存在工作区诊断").map_err(|e| e.to_string())?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

fn map_references(index: &worldline_core::MapIndex) -> Vec<Value> {
    index
        .placements_by_target
        .iter()
        .map(|(target, placements)| json!({"target": target, "placements": placements}))
        .collect()
}

fn cmd_maps(args: &MapsArgs, out: &mut impl Write) -> Result<i32, String> {
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(error) => return write_query_failure(&args.path, args.json, &error, out),
    };
    let ok = !snapshot.result.has_errors();
    if args.json {
        let mut payload = query_payload_base(&snapshot);
        payload.insert("ok".into(), json!(ok));
        payload.insert("maps".into(), json!(&snapshot.map_index.maps));
        payload.insert(
            "references".into(),
            json!(map_references(&snapshot.map_index)),
        );
        writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}: {} 张地图",
            args.path.display(),
            snapshot.map_index.maps.len()
        )
        .map_err(|e| e.to_string())?;
        for (id, map) in &snapshot.map_index.maps {
            writeln!(
                out,
                "{id}  {}  ({} 个标记)",
                map.title,
                map.placements.len()
            )
            .map_err(|e| e.to_string())?;
        }
        for reference in map_references(&snapshot.map_index) {
            writeln!(out, "引用: {reference}").map_err(|e| e.to_string())?;
        }
        for diagnostic in snapshot
            .result
            .diagnostics
            .iter()
            .chain(snapshot.workspace_diagnostics.iter())
        {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

fn cmd_relations(args: &RelationsArgs, out: &mut impl Write) -> Result<i32, String> {
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(error) => return write_query_failure(&args.path, args.json, &error, out),
    };
    if snapshot.result.has_errors() {
        if args.json {
            let mut payload = query_payload_base(&snapshot);
            payload.insert("ok".into(), json!(false));
            payload.insert("target".into(), json!(&args.target));
            payload.insert("depth".into(), json!(args.depth));
            payload.insert("nodes".into(), json!([]));
            payload.insert("edges".into(), json!([]));
            writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
        } else {
            print_errors_hint(&snapshot.result.diagnostics, out);
        }
        return Ok(1);
    }
    if snapshot
        .result
        .analysis
        .catalog
        .object(&args.target)
        .is_none()
    {
        return write_query_error(
            &args.path,
            args.json,
            "UNKNOWN_TARGET",
            &format!("目标对象不存在 {}:{}", args.target.kind, args.target.id),
            out,
            2,
        );
    }
    if let Some(relation_type) = args.relation_type.as_deref() {
        if !snapshot
            .result
            .analysis
            .catalog
            .relation_types
            .contains_key(relation_type)
        {
            return write_query_error(
                &args.path,
                args.json,
                "UNKNOWN_RELATION_TYPE",
                &format!("关系类型 `{relation_type}` 不存在"),
                out,
                2,
            );
        }
    }
    let query = snapshot.result.analysis.catalog.query_relations(
        &args.target,
        RelationQueryOptions {
            offset: args.offset,
            depth: args.depth,
            relation_type: args.relation_type.clone(),
            direction: args.direction,
            ..RelationQueryOptions::default()
        },
    );
    if args.json {
        let mut payload = query_payload_base(&snapshot);
        payload.insert("ok".into(), json!(true));
        let query = serde_json::to_value(query).expect("关系查询结果可序列化");
        if let Value::Object(fields) = query {
            for (key, value) in fields {
                payload.insert(key, value);
            }
        }
        writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}:{}: {} 条节点 / {} 条关系",
            args.target.kind,
            args.target.id,
            query.nodes.len(),
            query.edges.len()
        )
        .map_err(|e| e.to_string())?;
        for edge in query.edges {
            writeln!(
                out,
                "{}  {} -> {}  {}",
                edge.id, edge.from_ref.id, edge.to_ref.id, edge.label
            )
            .map_err(|e| e.to_string())?;
        }
        if query.truncated {
            writeln!(out, "结果已截断，请使用 continuation 继续查询").map_err(|e| e.to_string())?;
        }
    }
    Ok(0)
}

fn relation_failure(
    json_mode: bool,
    out: &mut impl Write,
    code: &str,
    message: String,
    result: Option<&CompileResult>,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Result<i32, String> {
    if json_mode {
        let payload = json!({
            "ok": false,
            "error": {"code": code, "message": message},
            "diagnostics": result.map_or_else(Vec::new, |value| value.diagnostics.clone()),
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
            "language_version": result.map(|value| value.options.language_version.as_str()),
            "baseline": baseline,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{code}: {message}").map_err(|e| e.to_string())?;
    }
    Ok(1)
}

fn relation_project_state(
    path: &Path,
    json_mode: bool,
    out: &mut impl Write,
) -> Result<(Project, CompileResult, String, Vec<Diagnostic>), i32> {
    let mut project = match Project::open(path) {
        Ok(project) => project,
        Err(error) => {
            let _ = relation_failure(json_mode, out, "IO_ERROR", error, None, String::new(), &[]);
            return Err(2);
        }
    };
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            let result = project.compile();
            let baseline = project.content_baseline();
            let diagnostics = project.authoring_diagnostics().to_vec();
            let _ = relation_failure(
                json_mode,
                out,
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result),
                baseline,
                &diagnostics,
            );
            return Err(2);
        }
    };
    let result = project.compile();
    let baseline = project.content_baseline();
    let diagnostics = project.authoring_diagnostics().to_vec();
    if !conflicts.is_empty() {
        let _ = relation_failure(
            json_mode,
            out,
            "CONFLICT",
            format!(
                "工程存在外部修改冲突，拒绝覆盖：{}",
                conflicts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            Some(&result),
            baseline,
            &diagnostics,
        );
        return Err(1);
    }
    Ok((project, result, baseline, diagnostics))
}

fn relation_common_checks(
    json_mode: bool,
    out: &mut impl Write,
    args_baseline: Option<&str>,
    project: &Project,
    result: &CompileResult,
    baseline: &str,
    workspace_diagnostics: &[Diagnostic],
) -> Result<(), i32> {
    if args_baseline.is_some_and(|expected| expected != baseline) {
        let _ = relation_failure(
            json_mode,
            out,
            "STALE_BASELINE",
            format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    if result.has_errors() {
        let _ = relation_failure(
            json_mode,
            out,
            "COMPILE_FAILED",
            "当前工程存在错误诊断，关系编辑未提交".into(),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    if !workspace_diagnostics.is_empty() {
        let _ = relation_failure(
            json_mode,
            out,
            "READ_ONLY",
            "工程清单或展示文档包含当前工具不支持的格式，只能只读查看".into(),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    if project.language_version_kind() != LanguageVersion::V1_10 {
        let _ = relation_failure(
            json_mode,
            out,
            "LANGUAGE_VERSION_REQUIRED",
            "关系编辑要求工程清单明确选择语言版本 1.10".into(),
            Some(result),
            baseline.to_string(),
            workspace_diagnostics,
        );
        return Err(1);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn relation_result(
    operation: &str,
    relation: Value,
    catalog: &worldline_core::catalog::Catalog,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
    json_mode: bool,
    out: &mut impl Write,
) -> Result<i32, String> {
    let payload = json!({
        "ok": true,
        "operation": operation,
        "relation": relation,
        "catalog": catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if json_mode {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "关系 {operation} 成功").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
fn relation_type_result(
    operation: &str,
    relation_type: Value,
    catalog: &worldline_core::catalog::Catalog,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
    json_mode: bool,
    out: &mut impl Write,
) -> Result<i32, String> {
    let payload = json!({
        "ok": true,
        "operation": operation,
        "relation_type": relation_type,
        "catalog": catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if json_mode {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "关系类型 {operation} 成功").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

fn cmd_relation_type_edit(
    args: &RelationTypeEditArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let (mut project, before, baseline, workspace_diagnostics) =
        match relation_project_state(&args.path, args.json, out) {
            Ok(state) => state,
            Err(code) => return Ok(code),
        };
    if let Err(code) = relation_common_checks(
        args.json,
        out,
        args.baseline.as_deref(),
        &project,
        &before,
        &baseline,
        &workspace_diagnostics,
    ) {
        return Ok(code);
    }
    let existing = before
        .analysis
        .catalog
        .relation_types
        .get(&args.id)
        .cloned();
    match args.operation {
        RelationEditOperation::Create if existing.is_some() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_TYPE_EXISTS",
                format!("关系类型 `{}` 已存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        RelationEditOperation::Update | RelationEditOperation::Delete if existing.is_none() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_TYPE_NOT_FOUND",
                format!("关系类型 `{}` 不存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        _ => {}
    }
    if args.operation == RelationEditOperation::Delete {
        if let Err(error) = project.remove_relation_type(&args.id) {
            return relation_failure(
                args.json,
                out,
                "EDIT_FAILED",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        if let Err(error) = project.save() {
            return relation_failure(
                args.json,
                out,
                "CONFLICT",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        let after = project.compile();
        return relation_type_result(
            "delete",
            Value::Null,
            &after.analysis.catalog,
            &after,
            project.content_baseline(),
            &workspace_diagnostics,
            args.json,
            out,
        );
    }
    let draft = if let Some(existing) = existing {
        RelationTypeDraft {
            id: args.id.clone(),
            display: args.display.clone().unwrap_or(existing.display),
            inverse_display: if args.clear_inverse_display {
                None
            } else {
                args.inverse_display.clone().or(existing.inverse_display)
            },
            direction: args.direction.unwrap_or(existing.direction),
            from_kind: if args.clear_from_kind {
                None
            } else {
                args.from_kind.clone().or(existing.from_kind)
            },
            to_kind: if args.clear_to_kind {
                None
            } else {
                args.to_kind.clone().or(existing.to_kind)
            },
        }
    } else {
        let Some(display) = args.display.clone() else {
            return relation_failure(
                args.json,
                out,
                "INVALID_ARGUMENT",
                "创建关系类型需要 --display".into(),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        };
        RelationTypeDraft {
            id: args.id.clone(),
            display,
            inverse_display: args.inverse_display.clone(),
            direction: args.direction.unwrap_or_default(),
            from_kind: args.from_kind.clone(),
            to_kind: args.to_kind.clone(),
        }
    };
    let original = (args.operation == RelationEditOperation::Update).then_some(args.id.as_str());
    let edit = project.write_relation_type(original, &draft);
    if let Err(error) = edit {
        return relation_failure(
            args.json,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return relation_failure(
            args.json,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    relation_type_result(
        match args.operation {
            RelationEditOperation::Create => "create",
            RelationEditOperation::Update => "update",
            RelationEditOperation::Delete => "delete",
        },
        serde_json::to_value(after.analysis.catalog.relation_types.get(&args.id))
            .expect("RelationTypeInfo 可序列化"),
        &after.analysis.catalog,
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
        args.json,
        out,
    )
}

fn cmd_relation_edit(args: &RelationEditArgs, out: &mut impl Write) -> Result<i32, String> {
    let (mut project, before, baseline, workspace_diagnostics) =
        match relation_project_state(&args.path, args.json, out) {
            Ok(state) => state,
            Err(code) => return Ok(code),
        };
    if let Err(code) = relation_common_checks(
        args.json,
        out,
        args.baseline.as_deref(),
        &project,
        &before,
        &baseline,
        &workspace_diagnostics,
    ) {
        return Ok(code);
    }
    let existing = before.analysis.catalog.relations.get(&args.id).cloned();
    match args.operation {
        RelationEditOperation::Create if existing.is_some() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_EXISTS",
                format!("关系 `{}` 已存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        RelationEditOperation::Update | RelationEditOperation::Delete if existing.is_none() => {
            return relation_failure(
                args.json,
                out,
                "RELATION_NOT_FOUND",
                format!("关系 `{}` 不存在", args.id),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
        _ => {}
    }
    if args.operation == RelationEditOperation::Delete {
        if let Err(error) = project.remove_relation(&args.id) {
            return relation_failure(
                args.json,
                out,
                "EDIT_FAILED",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        if let Err(error) = project.save() {
            return relation_failure(
                args.json,
                out,
                "CONFLICT",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
        let after = project.compile();
        return relation_result(
            "delete",
            Value::Null,
            &after.analysis.catalog,
            &after,
            project.content_baseline(),
            &workspace_diagnostics,
            args.json,
            out,
        );
    }
    let draft = if let Some(existing) = existing {
        RelationDraft {
            id: args.id.clone(),
            relation_type: args.relation_type.clone().unwrap_or(existing.relation_type),
            from: args.from.clone().unwrap_or(existing.from_ref),
            to: args.to.clone().unwrap_or(existing.to_ref),
            description: args.description.clone().unwrap_or(existing.description),
            source_note: if args.clear_source_note {
                None
            } else {
                args.source_note.clone().or(existing.source_note)
            },
            scope_refs: if args.clear_scope_refs {
                Vec::new()
            } else if args.scope_refs.is_empty() {
                existing.scope_refs
            } else {
                args.scope_refs.clone()
            },
            properties: if args.clear_properties {
                Vec::new()
            } else if args.properties.is_empty() {
                existing.properties.into_iter().collect()
            } else {
                args.properties.clone()
            },
        }
    } else {
        RelationDraft {
            id: args.id.clone(),
            relation_type: args.relation_type.clone().ok_or("创建关系需要 --type")?,
            from: args.from.clone().ok_or("创建关系需要 --from KIND:ID")?,
            to: args.to.clone().ok_or("创建关系需要 --to KIND:ID")?,
            description: args.description.clone().unwrap_or_default(),
            source_note: args.source_note.clone(),
            scope_refs: args.scope_refs.clone(),
            properties: args.properties.clone(),
        }
    };
    let original = (args.operation == RelationEditOperation::Update).then_some(args.id.as_str());
    let edit = project.write_relation(original, &draft);
    if let Err(error) = edit {
        return relation_failure(
            args.json,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return relation_failure(
            args.json,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    relation_result(
        match args.operation {
            RelationEditOperation::Create => "create",
            RelationEditOperation::Update => "update",
            RelationEditOperation::Delete => "delete",
        },
        serde_json::to_value(after.analysis.catalog.relations.get(&args.id))
            .expect("SemanticRelationInfo 可序列化"),
        &after.analysis.catalog,
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
        args.json,
        out,
    )
}

fn cmd_promotion(args: &PromotionArgs, out: &mut impl Write) -> Result<i32, String> {
    let (mut project, before, baseline, workspace_diagnostics) =
        match relation_project_state(&args.path, args.json, out) {
            Ok(state) => state,
            Err(code) => return Ok(code),
        };
    if let Err(code) = relation_common_checks(
        args.json,
        out,
        args.baseline.as_deref(),
        &project,
        &before,
        &baseline,
        &workspace_diagnostics,
    ) {
        return Ok(code);
    }
    let Some(handle) = before
        .analysis
        .catalog
        .legacy_relation_handles()
        .into_iter()
        .find(|handle| {
            handle.source == args.source
                && handle.target == args.target
                && handle.label == args.label
                && handle.occurrence == args.occurrence
        })
    else {
        return relation_failure(
            args.json,
            out,
            "LEGACY_RELATION_NOT_FOUND",
            "指定的旧人物关系句柄不存在".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    };
    let draft = RelationDraft {
        id: args.relation_id.clone(),
        relation_type: args.relation_type.clone(),
        from: handle.source.clone(),
        to: handle.target.clone(),
        description: args
            .description
            .clone()
            .unwrap_or_else(|| handle.label.clone()),
        source_note: args.source_note.clone(),
        scope_refs: args.scope_refs.clone(),
        properties: args.properties.clone(),
    };
    let preview = match project.preview_promote_legacy_relation(&handle, &draft) {
        Ok(preview) => preview,
        Err(error) => {
            return relation_failure(
                args.json,
                out,
                "EDIT_FAILED",
                error,
                Some(&before),
                baseline,
                &workspace_diagnostics,
            )
        }
    };
    if args.operation == PromotionOperation::Preview {
        let payload = json!({
            "ok": true,
            "operation": "preview",
            "preview": preview,
            "catalog": &before.analysis.catalog,
            "diagnostics": before.diagnostics,
            "language_version": before.options.language_version.as_str(),
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": false,
        });
        if args.json {
            writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        } else {
            writeln!(out, "关系提升预览成功").map_err(|e| e.to_string())?;
        }
        return Ok(0);
    }
    if let Err(error) = project.apply_legacy_relation_promotion(&preview) {
        return relation_failure(
            args.json,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return relation_failure(
            args.json,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    let payload = json!({
        "ok": true,
        "operation": "commit",
        "preview": preview,
        "catalog": &after.analysis.catalog,
        "diagnostics": after.diagnostics,
        "language_version": after.options.language_version.as_str(),
        "baseline": project.content_baseline(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if args.json {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "关系提升提交成功").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

/// 编译存在 error 时的统一出口:JSON 模式输出 compile_failed 事件,人类模式输出提示。
fn compile_failed(diags: &[Diagnostic], json: bool, out: &mut impl Write) -> Result<i32, String> {
    if json {
        let payload = json!({ "type": "compile_failed", "diagnostics": diags });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        print_errors_hint(diags, out);
        for d in diags.iter().filter(|d| d.severity == Severity::Error) {
            let _ = writeln!(out, "{d}");
        }
    }
    Ok(1)
}

fn cmd_check(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if f.json {
        let payload = json!({
            "ok": !result.has_errors() && !snapshot.read_only,
            "stats": &result.analysis.stats,
            "language_version": result.options.language_version.as_str(),
            "diagnostics": result.diagnostics,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        return Ok(if result.has_errors() || snapshot.read_only {
            1
        } else {
            0
        });
    }
    let s = result.analysis.stats;
    writeln!(
        out,
        "{}: {} 线 / {} 角色 / {} 事件 / {} 场景 / {} 选择 / {} 字",
        f.path.display(),
        s.storylines,
        s.characters,
        s.events,
        s.scenes,
        s.choices,
        s.words
    )
    .map_err(|e| e.to_string())?;
    if result.diagnostics.is_empty() && snapshot.workspace_diagnostics.is_empty() {
        writeln!(out, "诊断:无问题 ✓").map_err(|e| e.to_string())?;
    } else {
        let (mut e, mut w, mut h) = (0, 0, 0);
        for d in snapshot
            .workspace_diagnostics
            .iter()
            .chain(result.diagnostics.iter())
        {
            match d.severity {
                Severity::Error => e += 1,
                Severity::Warning => w += 1,
                Severity::Hint => h += 1,
            }
            writeln!(out, "{d}").map_err(|er| er.to_string())?;
        }
        writeln!(
            out,
            "共 {} 条(错误 {e},警告 {w},提示 {h}){}",
            result.diagnostics.len() + snapshot.workspace_diagnostics.len(),
            if snapshot.read_only {
                "；工作区只读"
            } else {
                ""
            }
        )
        .map_err(|er| er.to_string())?;
    }
    Ok(if result.has_errors() || snapshot.read_only {
        1
    } else {
        0
    })
}

fn cmd_graph(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    if f.json {
        let payload = json!({
            "graph": &result.analysis.graph,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        write!(out, "{}", result.analysis.graph.to_mermaid()).map_err(|e| e.to_string())?;
    }
    Ok(0)
}

/// `wl timeline`:故事线泳道 + 漂流过程流图(Mermaid flowchart LR)。
fn cmd_timeline(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    if f.json {
        let payload = json!({
            "stats": &result.analysis.stats,
            "anchors": &result.analysis.anchors,
            "timeline": &result.analysis.timeline,
            "graph": &result.analysis.graph,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let s = result.analysis.stats;
    writeln!(
        out,
        "%% 世界线时间线:{} 条故事线 · {} 次事件 · {} 个锚点声明",
        s.storylines,
        s.events,
        result.analysis.anchors.len()
    )
    .map_err(|e| e.to_string())?;
    write!(
        out,
        "{}",
        result.analysis.timeline.to_mermaid(&result.analysis.graph)
    )
    .map_err(|e| e.to_string())?;
    Ok(0)
}

/// `wl catalog`:直接投影 core 的对象目录与标签查询结果。
fn cmd_catalog(args: &CatalogArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&args.file.path, args.file.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    let catalog = &result.analysis.catalog;
    let mut matches = match args.tag.as_deref() {
        Some(tag) => {
            if !catalog.tags.contains_key(tag) {
                return Err(format!("未知标签 `{tag}`"));
            }
            catalog.query(tag, args.recursive)
        }
        None => catalog.objects.clone(),
    };
    if let Some(kind) = &args.kind {
        matches.retain(|object| &object.target.kind == kind);
    }
    let ok = !result.has_errors();
    if args.file.json {
        let mut payload = json!({
            "ok": ok,
            "catalog": catalog,
            "matches": matches,
            "diagnostics": result.diagnostics,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        if result.options.language_version == LanguageVersion::V1_10 || !catalog.entities.is_empty()
        {
            payload["language_version"] = json!(result.options.language_version.as_str());
        }
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}: {} 个标签 / {} 个素材 / {} 个命中对象",
            args.file.path.display(),
            catalog.tags.len(),
            catalog.assets.len(),
            matches.len(),
        )
        .map_err(|e| e.to_string())?;
        for object in matches {
            writeln!(
                out,
                "{} {}  {}  ({}:{})",
                object.target.kind, object.target.id, object.display, object.file, object.line,
            )
            .map_err(|e| e.to_string())?;
        }
        for diagnostic in &result.diagnostics {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
        for diagnostic in &snapshot.workspace_diagnostics {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
        if snapshot.read_only {
            writeln!(out, "工作区只读：清单包含当前工具不支持的能力").map_err(|e| e.to_string())?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

fn cmd_entity(args: &EntityArgs, out: &mut impl Write) -> Result<i32, String> {
    let mut project = Project::open(&args.path)?;
    let before = project.compile();
    let baseline = project.content_baseline();
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    if let Some(expected) = &args.baseline {
        if expected != &baseline {
            return entity_failure(
                args,
                out,
                "STALE_BASELINE",
                format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
                Some(&before),
                baseline,
                &workspace_diagnostics,
            );
        }
    }
    if before.has_errors() {
        return entity_failure(
            args,
            out,
            "COMPILE_FAILED",
            "当前工程存在错误诊断，实体编辑未提交".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if !project.authoring_diagnostics().is_empty() {
        return entity_failure(
            args,
            out,
            "READ_ONLY",
            "工程清单包含当前工具不支持的格式或必需能力，只能只读查看".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if project.language_version_kind() != LanguageVersion::V1_10 {
        return entity_failure(
            args,
            out,
            "LANGUAGE_VERSION_REQUIRED",
            "实体编辑要求工程清单明确选择语言版本 1.10".into(),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let entry = canonical_entry(&args.path)?;
    let id = args.id.as_deref().ok_or("entity 操作需要 --id")?;
    let existing = before.analysis.catalog.entities.get(id).cloned();
    let original = match args.operation {
        EntityOperation::Create => None,
        EntityOperation::Update | EntityOperation::Delete => Some(id),
    };
    if matches!(args.operation, EntityOperation::Create) && existing.is_some() {
        return entity_failure(
            args,
            out,
            "ENTITY_EXISTS",
            format!("实体 `{id}` 已存在"),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if matches!(
        args.operation,
        EntityOperation::Update | EntityOperation::Delete
    ) && existing.is_none()
    {
        return entity_failure(
            args,
            out,
            "ENTITY_NOT_FOUND",
            format!("实体 `{id}` 不存在"),
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let draft = if args.operation == EntityOperation::Delete {
        None
    } else {
        Some(entity_draft_from_args(args, existing.as_ref())?)
    };
    let operation = match args.operation {
        EntityOperation::Create => "create",
        EntityOperation::Update => "update",
        EntityOperation::Delete => "delete",
    };
    let edit = project.edit(|candidate| match args.operation {
        EntityOperation::Create => candidate.write_entity(&entry, None, draft.as_ref().unwrap()),
        EntityOperation::Update => {
            candidate.write_entity(&entry, original, draft.as_ref().unwrap())
        }
        EntityOperation::Delete => candidate.remove_entity(id),
    });
    if let Err(error) = edit {
        return entity_failure(
            args,
            out,
            "EDIT_FAILED",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    if let Err(error) = project.save() {
        return entity_failure(
            args,
            out,
            "CONFLICT",
            error,
            Some(&before),
            baseline,
            &workspace_diagnostics,
        );
    }
    let after = project.compile();
    let after_baseline = project.content_baseline();
    let entity = if id.is_empty() {
        None
    } else {
        after.analysis.catalog.entities.get(id)
    };
    let entity_value =
        entity.map(|value| serde_json::to_value(value).expect("EntityInfo 可序列化"));
    let payload = json!({
        "ok": true,
        "operation": operation,
        "entity": entity_value,
        "catalog": &after.analysis.catalog,
        "language_version": after.options.language_version.as_str(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
        "baseline": after_baseline,
    });
    if args.json {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "实体 {operation} 成功: {id}").map_err(|e| e.to_string())?;
        writeln!(out, "基线: {after_baseline}").map_err(|e| e.to_string())?;
    }
    Ok(0)
}

fn entity_draft_from_args(
    args: &EntityArgs,
    existing: Option<&worldline_core::catalog::EntityInfo>,
) -> Result<EntityDraft, String> {
    let id = args.id.clone().ok_or("entity 操作需要 --id")?;
    let entity_type = args
        .entity_type
        .clone()
        .or_else(|| existing.map(|entity| entity.entity_type.clone()))
        .ok_or("创建实体需要 --kind")?;
    let display = args
        .display
        .clone()
        .or_else(|| existing.map(|entity| entity.display.clone()))
        .unwrap_or_else(|| id.clone());
    let description = args
        .description
        .clone()
        .or_else(|| existing.map(|entity| entity.description.clone()))
        .unwrap_or_default();
    let properties = if args.properties.is_empty() {
        existing
            .map(|entity| {
                entity
                    .properties
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        let mut merged = existing
            .map(|entity| {
                entity
                    .properties
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        for (key, value) in &args.properties {
            merged.insert(key.clone(), value.clone());
        }
        merged.into_iter().collect()
    };
    Ok(EntityDraft {
        id,
        entity_type,
        display,
        description,
        properties,
    })
}

fn canonical_entry(path: &Path) -> Result<PathBuf, String> {
    let entry = if path.is_dir() {
        path.join("world.wl")
    } else {
        path.to_path_buf()
    };
    std::fs::canonicalize(&entry)
        .map_err(|error| format!("无法定位工程入口 {}: {error}", entry.display()))
}

fn entity_failure(
    args: &EntityArgs,
    out: &mut impl Write,
    code: &str,
    message: String,
    result: Option<&CompileResult>,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Result<i32, String> {
    if args.json {
        let payload = json!({
            "ok": false,
            "error": { "code": code, "message": message },
            "diagnostics": result.map_or_else(Vec::new, |value| value.diagnostics.clone()),
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
            "language_version": result.map(|value| value.options.language_version.as_str()),
            "baseline": baseline,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{code}: {message}").map_err(|e| e.to_string())?;
    }
    Ok(1)
}

fn print_errors_hint(diags: &[Diagnostic], out: &mut impl Write) {
    let _ = writeln!(
        out,
        "存在 {} 个错误,先运行 `wl check` 修复后再导出关系图",
        diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    );
}

fn cmd_play(f: &FileArgs, out: &mut impl Write, input: &mut impl BufRead) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    let mut story = match f.load.as_deref() {
        Some(p) => {
            let save = std::fs::read_to_string(p)
                .map_err(|e| format!("无法读取存档 {}: {e}", p.display()))?;
            match Story::load(&result.program, &result.analysis, &save) {
                Ok(story) => story,
                Err(error) => return play_start_failure(f, out, format!("存档载入失败:{error}")),
            }
        }
        None => match Story::new(&result.program, &result.analysis) {
            Ok(story) => story,
            Err(error) => return play_start_failure(f, out, format!("故事启动失败:{error}")),
        },
    };
    if f.json {
        play_json(&mut story, f.save.as_deref(), out, input)
    } else {
        play_human(&mut story, f.save.as_deref(), out, input)
    }
}

fn play_start_failure(f: &FileArgs, out: &mut impl Write, message: String) -> Result<i32, String> {
    if f.json {
        let payload = json!({
            "type": "run_error",
            "message": message,
            "node": Value::Null,
            "line": Value::Null,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{message}").map_err(|e| e.to_string())?;
    }
    Ok(1)
}

/// 人类模式:剧情文本 + 编号选项菜单,stdin 读 1 起序号。
fn play_human(
    story: &mut Story,
    save: Option<&Path>,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<i32, String> {
    let mut buf = String::new();
    let code = loop {
        match story.continue_story() {
            Ok(outputs) => {
                let mut ended = false;
                for o in outputs {
                    match o {
                        Output::Text {
                            content, new_line, ..
                        } => {
                            if new_line {
                                writeln!(out).map_err(|e| e.to_string())?;
                            }
                            write!(out, "{content}").map_err(|e| e.to_string())?;
                        }
                        Output::Ended => ended = true,
                    }
                }
                out.flush().map_err(|e| e.to_string())?;
                if ended {
                    writeln!(out).map_err(|e| e.to_string())?;
                    writeln!(out, "—— 世界线收束,故事结束 ——").map_err(|e| e.to_string())?;
                    break 0;
                }
            }
            Err(e) => {
                writeln!(out, "运行错误:{e}").map_err(|er| er.to_string())?;
                break 1;
            }
        }
        let choices = story.choices().to_vec();
        if choices.is_empty() {
            continue;
        }
        writeln!(out).map_err(|e| e.to_string())?;
        for (i, c) in choices.iter().enumerate() {
            writeln!(out, "  {}) {}", i + 1, c.label).map_err(|e| e.to_string())?;
        }
        write!(out, "> ").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        buf.clear();
        if input.read_line(&mut buf).map_err(|e| e.to_string())? == 0 {
            writeln!(out).map_err(|e| e.to_string())?;
            writeln!(out, "(输入结束,退出)").map_err(|e| e.to_string())?;
            break 0;
        }
        let sel: Option<usize> = buf.trim().parse::<usize>().ok().map(|n| n.wrapping_sub(1));
        match sel.and_then(|i| choices.get(i).map(|_| i)) {
            Some(i) => {
                if let Err(e) = story.choose(i) {
                    writeln!(out, "{e}").map_err(|er| er.to_string())?;
                }
            }
            None => {
                writeln!(out, "(输入序号无效)").map_err(|e| e.to_string())?;
            }
        }
    };
    write_save(story, save)?;
    Ok(code)
}

/// 选项列表的机器视图(index 0 起,规范 agent-protocol.md §2.4)。
fn choices_json(story: &Story) -> Vec<serde_json::Value> {
    story
        .choices()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut choice = json!(c);
            choice["index"] = json!(i);
            choice
        })
        .collect()
}

/// JSON 模式(规范 agent-protocol.md §2.4):每回合一行事件;
/// 暂停时从 stdin 读一行 0 起序号(取值 = choices[].index)。
fn play_json(
    story: &mut Story,
    save: Option<&Path>,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<i32, String> {
    let mut buf = String::new();
    let code = loop {
        match story.continue_story() {
            Ok(outputs) => {
                let ended = outputs.iter().any(|o| matches!(o, Output::Ended));
                let outs: Vec<serde_json::Value> = outputs
                    .iter()
                    .map(|o| serde_json::to_value(o).expect("Output 序列化不失败"))
                    .collect();
                let payload = json!({
                    "type": if ended { "ended" } else { "turn" },
                    "outputs": outs,
                    "choices": choices_json(story),
                    "state": story.state_view(),
                });
                writeln!(out, "{payload}").map_err(|e| e.to_string())?;
                if ended {
                    break 0;
                }
            }
            Err(e) => {
                let payload = json!({
                    "type": "run_error",
                    "message": e.message,
                    "node": e.node,
                    "line": e.line,
                });
                writeln!(out, "{payload}").map_err(|er| er.to_string())?;
                break 1;
            }
        }
        if story.choices().is_empty() {
            continue;
        }
        out.flush().map_err(|e| e.to_string())?;
        buf.clear();
        if input.read_line(&mut buf).map_err(|e| e.to_string())? == 0 {
            let payload = json!({ "type": "eof", "state": story.state_view() });
            writeln!(out, "{payload}").map_err(|e| e.to_string())?;
            break 0;
        }
        match buf.trim().parse::<usize>() {
            Ok(i) if i < story.choices().len() => {
                if let Err(e) = story.choose(i) {
                    let payload = json!({
                        "type": "run_error",
                        "message": e.message,
                        "node": e.node,
                        "line": e.line,
                    });
                    writeln!(out, "{payload}").map_err(|er| er.to_string())?;
                    break 1;
                }
            }
            _ => {
                let payload = json!({ "type": "invalid_choice", "message": "(输入序号无效)" });
                writeln!(out, "{payload}").map_err(|e| e.to_string())?;
            }
        }
    };
    write_save(story, save)?;
    Ok(code)
}

/// `--save=<path>`:退出前把当前状态写为存档 JSON(暂停态语义见 spec/semantics.md §7)。
fn write_save(story: &Story, save: Option<&Path>) -> Result<(), String> {
    let Some(p) = save else {
        return Ok(());
    };
    let json = story.save().map_err(|e| format!("存档生成失败:{e}"))?;
    std::fs::write(p, json).map_err(|e| format!("无法写入存档 {}: {e}", p.display()))
}

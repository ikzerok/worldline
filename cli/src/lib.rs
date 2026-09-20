//! wl 命令行库:check / play / graph / timeline / catalog 的实现。
//! 以库形式暴露,供 main.rs 与集成测试共用。
//! `--json` 机器输出契约见 `worldline/spec/agent-protocol.md`。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use worldline_core::ast::PropertyValue;
use worldline_core::authoring::EntityDraft;
use worldline_core::project::Project;
use worldline_core::{
    compile_path, compile_path_with_options, CompileOptions, CompileResult, Diagnostic,
    LanguageVersion, Severity,
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
            "未知子命令 `{other}`(可用:check / play / graph / timeline / catalog / entity)"
        )),
    }
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
        "catalog": after.analysis.catalog,
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

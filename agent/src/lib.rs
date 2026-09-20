//! wl-agent —— worldline 机器协议入口。
//! 契约见 `worldline/spec/agent-protocol.md`:stdio 行分帧 JSON-RPC 2.0,
//! 单线程顺序处理;故事层失败以 `{"ok": false}` 结果表达,协议违规才用 error。

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use worldline_core::ast::PropertyValue;
use worldline_core::authoring::EntityDraft;
use worldline_core::project::Project;
use worldline_core::{
    compile_path_with_options, compile_source, compile_source_with_options, Analysis,
    CompileOptions, CompileResult, Diagnostic, LanguageVersion, Program,
};
use worldline_runtime::Story;

/// 协议版本:方法表或错误语义发生不兼容变更时递增。
pub const PROTOCOL: u64 = 1;

/// 协议层错误(JSON-RPC error);故事层失败不走这里。
struct ProtoError {
    code: i32,
    message: String,
    data: Value,
}

impl ProtoError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        ProtoError {
            code,
            message: message.into(),
            data: Value::Null,
        }
    }
}

/// 编译产物:泄漏为 `'static` 供 `Story` 借用(与 worldedit 试玩同款模式;
/// 设计面向短生命周期 agent 进程,不做回收,见 ADR-006)。
struct StoryUnit {
    program: &'static Program,
    analysis: &'static Analysis,
    language_version: LanguageVersion,
}

/// 一个进行中的故事实例。
struct Session {
    #[allow(dead_code)] // 保留归属信息,便于未来多路复用与调试
    story_id: String,
    story: Story<'static>,
}

#[derive(Default)]
struct Server {
    stories: HashMap<String, StoryUnit>,
    sessions: HashMap<String, Session>,
    projects: HashMap<String, ProjectUnit>,
    next_story: u64,
    next_session: u64,
    next_project: u64,
    shutdown: bool,
}

struct ProjectUnit {
    project: Project,
    entry: PathBuf,
}

struct CompileInput {
    result: CompileResult,
    workspace_diagnostics: Vec<Diagnostic>,
}

impl CompileInput {
    fn plain(result: CompileResult) -> Self {
        Self {
            result,
            workspace_diagnostics: Vec::new(),
        }
    }
}

/// 驱动一轮协议会话:逐行读请求、逐行写响应;EOF 或 `shutdown` 后返回退出码。
pub fn run(reader: &mut impl BufRead, writer: &mut impl Write) -> i32 {
    let mut server = Server::default();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return 0;
        }
        let Some(resp) = server.handle(line.trim()) else {
            continue;
        };
        if writeln!(writer, "{resp}").is_err() {
            return 0;
        }
        let _ = writer.flush();
        if server.shutdown {
            return 0;
        }
    }
}

impl Server {
    /// 处理一行消息;通知(无 id)不产生响应。
    fn handle(&mut self, line: &str) -> Option<String> {
        self.dispatch(line).map(|v| v.to_string())
    }

    fn dispatch(&mut self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(err(Value::Null, -32700, "解析失败", json!(e.to_string()))),
        };
        if !msg.is_object() {
            return Some(err(
                Value::Null,
                -32600,
                "无效请求",
                json!("消息必须是 JSON 对象"),
            ));
        }
        if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(err(
                msg.get("id").cloned().unwrap_or(Value::Null),
                -32600,
                "无效请求",
                json!("jsonrpc 必须为 \"2.0\""),
            ));
        }
        let id = msg.get("id").cloned();
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            return id.map(|id| err(id, -32600, "无效请求", json!("缺少 method")));
        };
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let outcome = self.call(method, &params);
        let id = id?;
        Some(match outcome {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => err(id, e.code, &e.message, e.data),
        })
    }

    fn call(&mut self, method: &str, params: &Value) -> Result<Value, ProtoError> {
        match method {
            "initialize" => Ok(json!({
                "protocol": PROTOCOL,
                "server": "wl-agent",
                "version": env!("CARGO_PKG_VERSION"),
            })),
            "compile" => self.compile(params),
            "analyze" => {
                let unit = self.story(param_str(params, "story_id")?)?;
                Ok(json!({
                    "graph": unit.analysis.graph,
                    "anchors": unit.analysis.anchors,
                    "symbols": unit.analysis.symbols,
                    "stats": unit.analysis.stats,
                    "world": unit.analysis.world,
                    "timeline": unit.analysis.timeline,
                    "catalog": unit.analysis.catalog,
                    "language_version": unit.language_version.as_str(),
                }))
            }
            "export" => {
                let unit = self.story(param_str(params, "story_id")?)?;
                let text = match param_str(params, "format")? {
                    "graph_mermaid" => unit.analysis.graph.to_mermaid(),
                    "timeline_mermaid" => unit.analysis.timeline.to_mermaid(&unit.analysis.graph),
                    other => {
                        return Err(ProtoError::new(-32602, format!("未知导出格式 `{other}`")))
                    }
                };
                Ok(json!({ "text": text }))
            }
            "session.open" => self.session_open(params),
            "session.continue" => {
                let s = self.session(params)?;
                match s.story.continue_story() {
                    Ok(outputs) => {
                        let outs: Vec<Value> = outputs
                            .iter()
                            .map(|o| serde_json::to_value(o).expect("Output 序列化不失败"))
                            .collect();
                        Ok(json!({
                            "outputs": outs,
                            "choices": choices_json(&s.story),
                            "state": s.story.state_view(),
                            "paused": s.story.is_paused(),
                            "ended": s.story.is_ended(),
                        }))
                    }
                    Err(e) => Ok(json!({ "ok": false, "run_error": e })),
                }
            }
            "session.choose" => {
                let s = self.session(params)?;
                let index = params
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| ProtoError::new(-32602, "需要整数参数 `index`(0 起)"))?
                    as usize;
                let len = s.story.choices().len();
                if index >= len {
                    return Err(ProtoError::new(
                        -32602,
                        format!("选择越界:index {index},共 {len} 项"),
                    ));
                }
                match s.story.choose(index) {
                    Ok(()) => Ok(json!({
                        "state": s.story.state_view(),
                        "paused": s.story.is_paused(),
                        "ended": s.story.is_ended(),
                        "choices": choices_json(&s.story),
                    })),
                    Err(e) => Ok(json!({ "ok": false, "run_error": e })),
                }
            }
            "session.state" => {
                let s = self.session(params)?;
                Ok(json!({ "state": s.story.state_view() }))
            }
            "session.save" => {
                let s = self.session(params)?;
                match s.story.save() {
                    Ok(save) => Ok(json!({ "save": save })),
                    Err(e) => Ok(json!({ "ok": false, "run_error": e })),
                }
            }
            "session.restart" => {
                let s = self.session(params)?;
                match s.story.restart() {
                    Ok(()) => Ok(json!({ "state": s.story.state_view() })),
                    Err(e) => Ok(json!({ "ok": false, "run_error": e })),
                }
            }
            "session.close" => {
                let id = param_str(params, "session_id")?;
                if self.sessions.remove(id).is_none() {
                    return Err(ProtoError::new(-32602, format!("未知 session_id `{id}`")));
                }
                Ok(json!({ "closed": true }))
            }
            "project.open" => self.project_open(params),
            "project.analyze" => self.project_analyze(params),
            "entity.create" => self.entity_mutation(params, EntityOperation::Create),
            "entity.update" => self.entity_mutation(params, EntityOperation::Update),
            "entity.delete" => self.entity_mutation(params, EntityOperation::Delete),
            "shutdown" => {
                self.shutdown = true;
                Ok(json!({ "bye": true }))
            }
            other => Err(ProtoError::new(-32601, format!("未知方法 `{other}`"))),
        }
    }

    fn compile(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let options = compile_options(params)?;
        let input = if let Some(p) = params.get("path").and_then(Value::as_str) {
            match compile_path_input(Path::new(p), params, options) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(json!({
                        "ok": false,
                        "error": {
                            "code": "IO_ERROR",
                            "message": format!("无法读取 {p}:{e}"),
                        },
                        "workspace_diagnostics": [],
                        "read_only": false,
                    }))
                }
            }
        } else if let Some(src) = params.get("source").and_then(Value::as_str) {
            let name = params
                .get("file_name")
                .and_then(Value::as_str)
                .unwrap_or("未命名.wl");
            if params.get("language_version").is_some()
                || params
                    .get("options")
                    .and_then(|value| value.get("language_version"))
                    .is_some()
            {
                CompileInput::plain(compile_source_with_options(name, src, options))
            } else {
                CompileInput::plain(compile_source(name, src))
            }
        } else {
            return Err(ProtoError::new(-32602, "需要 `path` 或 `source` 参数"));
        };
        let CompileInput {
            result,
            workspace_diagnostics,
        } = input;
        let read_only = !workspace_diagnostics.is_empty();
        if result.has_errors() {
            return Ok(json!({
                "ok": false,
                "diagnostics": result.diagnostics,
                "language_version": result.options.language_version.as_str(),
                "workspace_diagnostics": workspace_diagnostics,
                "read_only": read_only,
            }));
        }
        self.next_story += 1;
        let story_id = format!("s{}", self.next_story);
        let program: &'static Program = Box::leak(Box::new(result.program));
        let analysis: &'static Analysis = Box::leak(Box::new(result.analysis));
        self.stories.insert(
            story_id.clone(),
            StoryUnit {
                program,
                analysis,
                language_version: result.options.language_version,
            },
        );
        Ok(json!({
            "ok": true,
            "story_id": story_id,
            "fingerprint": analysis.fingerprint,
            "stats": analysis.stats,
            "diagnostics": result.diagnostics,
            "language_version": result.options.language_version.as_str(),
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": read_only,
        }))
    }

    fn story(&self, id: &str) -> Result<&StoryUnit, ProtoError> {
        self.stories
            .get(id)
            .ok_or_else(|| ProtoError::new(-32602, format!("未知 story_id `{id}`")))
    }

    fn session(&mut self, params: &Value) -> Result<&mut Session, ProtoError> {
        let id = param_str(params, "session_id")?;
        self.sessions
            .get_mut(id)
            .ok_or_else(|| ProtoError::new(-32602, format!("未知 session_id `{id}`")))
    }

    fn session_open(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let story_id = param_str(params, "story_id")?.to_string();
        let (program, analysis) = {
            let unit = self.story(&story_id)?;
            (unit.program, unit.analysis)
        };
        let opened = match params.get("save").and_then(Value::as_str) {
            Some(save) => Story::load(program, analysis, save),
            None => Story::new(program, analysis),
        };
        let story = match opened {
            Ok(s) => s,
            Err(e) => return Ok(json!({ "ok": false, "run_error": e })),
        };
        let state = story.state_view();
        self.next_session += 1;
        let session_id = format!("c{}", self.next_session);
        self.sessions
            .insert(session_id.clone(), Session { story_id, story });
        Ok(json!({ "session_id": session_id, "state": state }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityOperation {
    Create,
    Update,
    Delete,
}

fn compile_options(params: &Value) -> Result<CompileOptions, ProtoError> {
    let version = params.get("language_version").or_else(|| {
        params
            .get("options")
            .and_then(|value| value.get("language_version"))
    });
    let Some(version) = version else {
        return Ok(CompileOptions::default());
    };
    let version = version
        .as_str()
        .ok_or_else(|| ProtoError::new(-32602, "`language_version` 必须是字符串"))?;
    match version {
        "1.9" => Ok(CompileOptions::new(LanguageVersion::V1_9)),
        "1.10" => Ok(CompileOptions::new(LanguageVersion::V1_10)),
        _ => Err(ProtoError::new(
            -32602,
            format!("不支持的语言版本 `{version}`(可用: 1.9 / 1.10)"),
        )),
    }
}

fn compile_path_input(
    path: &Path,
    params: &Value,
    options: CompileOptions,
) -> Result<CompileInput, std::io::Error> {
    let explicit = params.get("language_version").is_some()
        || params
            .get("options")
            .and_then(|value| value.get("language_version"))
            .is_some();
    if !explicit {
        let root = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        };
        if root.join(".world").join("project.json").is_file() {
            let mut project = Project::open(path).map_err(std::io::Error::other)?;
            return Ok(CompileInput {
                result: project.compile(),
                workspace_diagnostics: project.authoring_diagnostics().to_vec(),
            });
        }
    }
    let result = compile_path_with_options(path, options)?;
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    if root.join(".world").join("project.json").is_file() {
        let project = Project::open(path).map_err(std::io::Error::other)?;
        return Ok(CompileInput {
            result,
            workspace_diagnostics: project.authoring_diagnostics().to_vec(),
        });
    }
    Ok(CompileInput::plain(result))
}

impl Server {
    fn project_open(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let path = param_str(params, "path")?;
        let path = Path::new(path);
        let entry = match project_entry(path) {
            Ok(entry) => entry,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        let mut project = match Project::open(path) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        let result = project.compile();
        let baseline = project.content_baseline();
        self.next_project += 1;
        let project_id = format!("p{}", self.next_project);
        let response = project_view(
            &result,
            Some(project_id.clone()),
            baseline,
            project.authoring_diagnostics(),
        );
        self.projects
            .insert(project_id, ProjectUnit { project, entry });
        Ok(response)
    }

    fn project_analyze(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let id = param_str(params, "project_id")?;
        let unit = self
            .projects
            .get_mut(id)
            .ok_or_else(|| ProtoError::new(-32602, format!("未知 project_id `{id}`")))?;
        let conflicts = match unit.project.refresh() {
            Ok(conflicts) => conflicts,
            Err(error) => {
                let result = unit.project.compile();
                return Ok(project_failure_with_workspace(
                    "IO_ERROR",
                    format!("刷新工程失败：{error}"),
                    Some(&result.diagnostics),
                    Some(unit.project.content_baseline()),
                    Some(result.options.language_version.as_str()),
                    unit.project.authoring_diagnostics(),
                ));
            }
        };
        let mut project = unit.project.clone();
        let result = project.compile();
        let mut response = project_view(
            &result,
            None,
            unit.project.content_baseline(),
            unit.project.authoring_diagnostics(),
        );
        if !conflicts.is_empty() {
            response["conflicts"] = json!(conflicts);
        }
        Ok(response)
    }

    fn entity_mutation(
        &mut self,
        params: &Value,
        operation: EntityOperation,
    ) -> Result<Value, ProtoError> {
        let expected = params
            .get("baseline")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| ProtoError::new(-32602, "`baseline` 必须是字符串"))
            })
            .transpose()?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return mutate_entity_project(
                &mut unit.project,
                &unit.entry,
                params,
                operation,
                expected,
            );
        }
        let path = param_str(params, "path")?;
        let entry = match project_entry(Path::new(path)) {
            Ok(entry) => entry,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        mutate_entity_project(&mut project, &entry, params, operation, expected)
    }
}

fn project_view(
    result: &CompileResult,
    project_id: Option<String>,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    let mut payload = json!({
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "catalog": result.analysis.catalog,
        "diagnostics": result.diagnostics,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
    });
    payload["ok"] = json!(!result.has_errors());
    if let Some(project_id) = project_id {
        payload["project_id"] = json!(project_id);
    }
    payload
}

fn project_failure(
    code: &str,
    message: String,
    diagnostics: Option<&Vec<worldline_core::Diagnostic>>,
    baseline: Option<String>,
    language_version: Option<&str>,
) -> Value {
    project_failure_with_workspace(code, message, diagnostics, baseline, language_version, &[])
}

fn project_failure_with_workspace(
    code: &str,
    message: String,
    diagnostics: Option<&Vec<Diagnostic>>,
    baseline: Option<String>,
    language_version: Option<&str>,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": false,
        "error": { "code": code, "message": message },
        "diagnostics": diagnostics.cloned().unwrap_or_default(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "baseline": baseline,
        "language_version": language_version,
    })
}

fn mutate_entity_project(
    project: &mut Project,
    entry: &Path,
    params: &Value,
    operation: EntityOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            let result = project.compile();
            return Ok(project_failure_with_workspace(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            ));
        }
    };
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    let before = project.compile();
    let baseline = project.content_baseline();
    if !conflicts.is_empty() {
        return Ok(project_failure_with_workspace(
            "CONFLICT",
            format!(
                "工程存在外部修改冲突，拒绝覆盖：{}",
                conflicts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if expected.is_some_and(|value| value != baseline) {
        return Ok(project_failure_with_workspace(
            "STALE_BASELINE",
            format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.has_errors() {
        return Ok(project_failure_with_workspace(
            "COMPILE_FAILED",
            "当前工程存在错误诊断，实体编辑未提交".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if !project.authoring_diagnostics().is_empty() {
        return Ok(project_failure_with_workspace(
            "READ_ONLY",
            "工程清单包含当前工具不支持的格式或必需能力，只能只读查看".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.options.language_version != LanguageVersion::V1_10
        || project.language_version_kind() != LanguageVersion::V1_10
    {
        return Ok(project_failure_with_workspace(
            "LANGUAGE_VERSION_REQUIRED",
            "实体编辑要求工程清单明确选择语言版本 1.10".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let id = entity_id(params, operation)?;
    let existing = before.analysis.catalog.entities.get(&id).cloned();
    if operation == EntityOperation::Create && existing.is_some() {
        return Ok(project_failure_with_workspace(
            "ENTITY_EXISTS",
            format!("实体 `{id}` 已存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if matches!(operation, EntityOperation::Update | EntityOperation::Delete) && existing.is_none()
    {
        return Ok(project_failure_with_workspace(
            "ENTITY_NOT_FOUND",
            format!("实体 `{id}` 不存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let draft = if operation == EntityOperation::Delete {
        None
    } else {
        Some(entity_draft(params, existing.as_ref())?)
    };
    let snapshot = project.clone();
    let edit = project.edit(|candidate| match operation {
        EntityOperation::Create => candidate.write_entity(entry, None, draft.as_ref().unwrap()),
        EntityOperation::Update => {
            candidate.write_entity(entry, Some(id.as_str()), draft.as_ref().unwrap())
        }
        EntityOperation::Delete => candidate.remove_entity(&id),
    });
    if let Err(error) = edit {
        *project = snapshot;
        return Ok(project_failure_with_workspace(
            "EDIT_FAILED",
            error,
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if let Err(error) = project.save() {
        *project = snapshot;
        return Ok(project_failure_with_workspace(
            "CONFLICT",
            error,
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let after = project.compile();
    let after_baseline = project.content_baseline();
    let operation_name = match operation {
        EntityOperation::Create => "create",
        EntityOperation::Update => "update",
        EntityOperation::Delete => "delete",
    };
    Ok(json!({
        "ok": true,
        "operation": operation_name,
        "entity": if operation == EntityOperation::Delete {
            Value::Null
        } else {
            serde_json::to_value(after.analysis.catalog.entities.get(&id))
                .expect("EntityInfo 可序列化")
        },
        "catalog": after.analysis.catalog,
        "language_version": after.options.language_version.as_str(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "baseline": after_baseline,
    }))
}

fn entity_id(params: &Value, operation: EntityOperation) -> Result<String, ProtoError> {
    let id = match operation {
        EntityOperation::Delete => params.get("id"),
        EntityOperation::Create | EntityOperation::Update => {
            params.get("entity").and_then(|value| value.get("id"))
        }
    }
    .and_then(Value::as_str)
    .ok_or_else(|| ProtoError::new(-32602, "实体参数需要字符串 `id`"))?;
    if id.is_empty() {
        return Err(ProtoError::new(-32602, "实体 `id` 不能为空"));
    }
    Ok(id.to_string())
}

fn entity_draft(
    params: &Value,
    existing: Option<&worldline_core::catalog::EntityInfo>,
) -> Result<EntityDraft, ProtoError> {
    let entity = params
        .get("entity")
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `entity`"))?;
    if !entity.is_object() {
        return Err(ProtoError::new(-32602, "`entity` 必须是对象"));
    }
    let id = entity
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.id.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "实体参数需要字符串 `id`"))?;
    let entity_type = entity
        .get("entity_type")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.entity_type.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "实体参数需要字符串 `entity_type`"))?;
    let display = entity
        .get("display")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.display.as_str()))
        .unwrap_or(id);
    let description = entity
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| params.get("description").and_then(Value::as_str))
        .or_else(|| existing.map(|value| value.description.as_str()))
        .unwrap_or_default();
    let mut properties = existing
        .map(|value| {
            value
                .properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    if let Some(values) = entity.get("properties") {
        let values = values
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "实体 `properties` 必须是对象"))?;
        for (key, value) in values {
            properties.insert(key.clone(), property_value(value)?);
        }
    }
    Ok(EntityDraft {
        id: id.into(),
        entity_type: entity_type.into(),
        display: display.into(),
        description: description.into(),
        properties: properties.into_iter().collect(),
    })
}

fn property_value(value: &Value) -> Result<PropertyValue, ProtoError> {
    match value {
        Value::String(value) => Ok(PropertyValue::Str(value.clone())),
        Value::Number(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(PropertyValue::Num)
            .ok_or_else(|| ProtoError::new(-32602, "property 数值必须是有限数值")),
        Value::Bool(value) => Ok(PropertyValue::Bool(*value)),
        _ => Err(ProtoError::new(
            -32602,
            "property 值只能是字符串、数值或布尔值",
        )),
    }
}

fn project_entry(path: &Path) -> Result<PathBuf, String> {
    let entry = if path.is_dir() {
        path.join("world.wl")
    } else {
        path.to_path_buf()
    };
    std::fs::canonicalize(&entry)
        .map_err(|error| format!("无法定位工程入口 {}: {error}", entry.display()))
}

/// 选项列表的机器视图(index 0 起,规范 agent-protocol.md §2.4)。
fn choices_json(story: &Story) -> Vec<Value> {
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

fn param_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, ProtoError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, format!("需要字符串参数 `{key}`")))
}

fn err(id: Value, code: i32, message: &str, data: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message, "data": data },
    })
}

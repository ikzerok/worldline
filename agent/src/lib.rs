//! wl-agent —— worldline 机器协议入口。
//! 契约见 `worldline/spec/agent-protocol.md`:stdio 行分帧 JSON-RPC 2.0,
//! 单线程顺序处理;故事层失败以 `{"ok": false}` 结果表达,协议违规才用 error。

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use worldline_core::ast::PropertyValue;
use worldline_core::authoring::EntityDraft;
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::{
    compile_path_with_options, compile_source, compile_source_with_options, Analysis,
    CompileOptions, CompileResult, Diagnostic, LanguageVersion, LegacyRelationHandle, Program,
    RelationDirection, RelationDraft, RelationPromotionPreview, RelationQueryDirection,
    RelationQueryOptions, RelationTypeDraft,
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

struct WorkspaceSnapshot {
    result: CompileResult,
    map_index: worldline_core::MapIndex,
    workspace_diagnostics: Vec<Diagnostic>,
    baseline: String,
    conflicts: Vec<PathBuf>,
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
                    "catalog": &unit.analysis.catalog,
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
            "workspace.check" => self.workspace_check(params),
            "maps.list" => self.maps_list(params),
            "relation.query" | "relations.query" => self.relation_query(params),
            "relation.type.create" | "relation_type.create" => {
                self.relation_type_mutation(params, RelationTypeOperation::Create)
            }
            "relation.type.update" | "relation_type.update" => {
                self.relation_type_mutation(params, RelationTypeOperation::Update)
            }
            "relation.type.delete" | "relation_type.delete" => {
                self.relation_type_mutation(params, RelationTypeOperation::Delete)
            }
            "relation.create" | "relations.create" => {
                self.relation_mutation(params, RelationOperation::Create)
            }
            "relation.update" | "relations.update" => {
                self.relation_mutation(params, RelationOperation::Update)
            }
            "relation.delete" | "relations.delete" => {
                self.relation_mutation(params, RelationOperation::Delete)
            }
            "relation.promote.preview"
            | "relation.promotion.preview"
            | "relations.promote.preview" => {
                self.relation_promotion(params, PromotionOperation::Preview)
            }
            "relation.promote.commit"
            | "relation.promotion.commit"
            | "relations.promote.commit" => {
                self.relation_promotion(params, PromotionOperation::Commit)
            }
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

    fn relation_query(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let target = relation_target(params)?;
        let options = relation_query_options(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
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
            let result = unit.project.compile();
            if result.has_errors() {
                return Ok(relation_compile_failure(
                    &result,
                    unit.project.authoring_diagnostics(),
                    Some(unit.project.content_baseline()),
                ));
            }
            let baseline = unit.project.content_baseline();
            let mut response = relation_query_value(
                &result.analysis,
                &result.diagnostics,
                unit.project.authoring_diagnostics(),
                result.options.language_version,
                Some(&baseline),
                &target,
                options,
            )?;
            if !conflicts.is_empty() {
                response["conflicts"] = json!(conflicts);
            }
            return Ok(response);
        }
        let story_id = param_str(params, "story_id")?;
        let unit = self.story(story_id)?;
        relation_query_value(
            unit.analysis,
            &[],
            &[],
            unit.language_version,
            None,
            &target,
            options,
        )
    }
}

fn relation_target(params: &Value) -> Result<TargetRef, ProtoError> {
    let value = params
        .get("target")
        .ok_or_else(|| ProtoError::new(-32602, "关系查询需要 `target`"))?;
    relation_target_value(value, "target")
}

fn relation_query_options(params: &Value) -> Result<RelationQueryOptions, ProtoError> {
    let offset = match params.get("offset") {
        None => 0,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| ProtoError::new(-32602, "`offset` 必须是非负整数"))?
            .try_into()
            .map_err(|_| ProtoError::new(-32602, "`offset` 超出平台整数范围"))?,
    };
    let depth = match params.get("depth") {
        None => 1,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| ProtoError::new(-32602, "`depth` 必须是整数 1 或 2"))?
            .try_into()
            .map_err(|_| ProtoError::new(-32602, "`depth` 必须是整数 1 或 2"))?,
    };
    if !matches!(depth, 1 | 2) {
        return Err(ProtoError::new(-32602, "`depth` 只能是 1 或 2"));
    }
    let direction = match params.get("direction").and_then(Value::as_str) {
        None | Some("both") => RelationQueryDirection::Both,
        Some("outgoing") => RelationQueryDirection::Outgoing,
        Some("incoming") => RelationQueryDirection::Incoming,
        Some(value) => {
            return Err(ProtoError::new(
                -32602,
                format!("未知关系方向 `{value}`(可用: outgoing / incoming / both)"),
            ))
        }
    };
    let relation_type = params
        .get("relation_type")
        .or_else(|| params.get("type"))
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| ProtoError::new(-32602, "`relation_type` 必须是字符串"))
        })
        .transpose()?;
    Ok(RelationQueryOptions {
        offset,
        depth,
        relation_type,
        direction,
        ..RelationQueryOptions::default()
    })
}

fn relation_compile_failure(
    result: &CompileResult,
    workspace_diagnostics: &[Diagnostic],
    baseline: Option<String>,
) -> Value {
    json!({
        "ok": false,
        "schema_version": 1,
        "diagnostics": result.diagnostics,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "language_version": result.options.language_version.as_str(),
        "workspace_revision": baseline,
        "truncated": false,
        "continuation": Value::Null,
    })
}

fn relation_query_value(
    analysis: &Analysis,
    diagnostics: &[Diagnostic],
    workspace_diagnostics: &[Diagnostic],
    language_version: LanguageVersion,
    baseline: Option<&str>,
    target: &TargetRef,
    options: RelationQueryOptions,
) -> Result<Value, ProtoError> {
    if analysis.catalog.object(target).is_none() {
        return Err(ProtoError::new(
            -32602,
            format!("关系查询目标不存在 {}:{}", target.kind, target.id),
        ));
    }
    if let Some(relation_type) = options.relation_type.as_deref() {
        if !analysis.catalog.relation_types.contains_key(relation_type) {
            return Err(ProtoError::new(
                -32602,
                format!("未知关系类型 `{relation_type}`"),
            ));
        }
    }
    let query = analysis.catalog.query_relations(target, options);
    let mut payload = serde_json::to_value(query)
        .expect("关系查询结果可序列化")
        .as_object()
        .cloned()
        .expect("关系查询结果必须是对象");
    payload.insert("ok".into(), json!(true));
    payload.insert("language_version".into(), json!(language_version.as_str()));
    payload.insert(
        "workspace_revision".into(),
        baseline.map_or(Value::Null, |value| json!(value)),
    );
    payload.insert("diagnostics".into(), json!(diagnostics));
    payload.insert("workspace_diagnostics".into(), json!(workspace_diagnostics));
    payload.insert("read_only".into(), json!(!workspace_diagnostics.is_empty()));
    Ok(Value::Object(payload))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntityOperation {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationTypeOperation {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationOperation {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromotionOperation {
    Preview,
    Commit,
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
        let snapshot = match refreshed_workspace(&mut unit.project) {
            Ok(snapshot) => snapshot,
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
        let mut response = project_view(
            &snapshot.result,
            None,
            snapshot.baseline,
            &snapshot.workspace_diagnostics,
        );
        response["maps"] = json!(&snapshot.map_index.maps);
        response["references"] = json!(map_references(&snapshot.map_index));
        if !snapshot.conflicts.is_empty() {
            response["conflicts"] = json!(snapshot.conflicts);
        }
        Ok(response)
    }

    fn workspace_check(&mut self, params: &Value) -> Result<Value, ProtoError> {
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(workspace_check_project(&mut unit.project));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(query_failure(
                    "IO_ERROR",
                    error.to_string(),
                    None,
                    None,
                    None,
                    &[],
                ))
            }
        };
        Ok(workspace_check_project(&mut project))
    }

    fn maps_list(&mut self, params: &Value) -> Result<Value, ProtoError> {
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(maps_list_project(&mut unit.project));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(query_failure(
                    "IO_ERROR",
                    error.to_string(),
                    None,
                    None,
                    None,
                    &[],
                ))
            }
        };
        Ok(maps_list_project(&mut project))
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

    fn relation_type_mutation(
        &mut self,
        params: &Value,
        operation: RelationTypeOperation,
    ) -> Result<Value, ProtoError> {
        let expected = baseline_param(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return mutate_relation_type_project(&mut unit.project, params, operation, expected);
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        mutate_relation_type_project(&mut project, params, operation, expected)
    }

    fn relation_mutation(
        &mut self,
        params: &Value,
        operation: RelationOperation,
    ) -> Result<Value, ProtoError> {
        let expected = baseline_param(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return mutate_relation_project(&mut unit.project, params, operation, expected);
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        mutate_relation_project(&mut project, params, operation, expected)
    }

    fn relation_promotion(
        &mut self,
        params: &Value,
        operation: PromotionOperation,
    ) -> Result<Value, ProtoError> {
        let expected = baseline_param(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return promote_relation_project(&mut unit.project, params, operation, expected);
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        promote_relation_project(&mut project, params, operation, expected)
    }
}

fn refreshed_workspace(project: &mut Project) -> Result<WorkspaceSnapshot, String> {
    let conflicts = project.refresh().map_err(|error| error.to_string())?;
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
        conflicts,
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

fn query_failure(
    code: &str,
    message: String,
    diagnostics: Option<&Vec<Diagnostic>>,
    baseline: Option<String>,
    language_version: Option<&str>,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": false,
        "schema_version": 1,
        "error": {"code": code, "message": message},
        "language_version": language_version,
        "workspace_revision": baseline,
        "diagnostics": diagnostics.cloned().unwrap_or_default(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "truncated": false,
        "continuation": Value::Null,
    })
}

fn map_references(index: &worldline_core::MapIndex) -> Vec<Value> {
    index
        .placements_by_target
        .iter()
        .map(|(target, placements)| json!({"target": target, "placements": placements}))
        .collect()
}

fn workspace_check_project(project: &mut Project) -> Value {
    let snapshot = match refreshed_workspace(project) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let result = project.compile();
            return query_failure(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            );
        }
    };
    let ok = !snapshot.result.has_errors();
    let mut payload = query_payload_base(&snapshot);
    payload.insert("ok".into(), json!(ok));
    payload.insert("stats".into(), json!(snapshot.result.analysis.stats));
    if !snapshot.conflicts.is_empty() {
        payload.insert("conflicts".into(), json!(snapshot.conflicts));
    }
    Value::Object(payload)
}

fn maps_list_project(project: &mut Project) -> Value {
    let snapshot = match refreshed_workspace(project) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let result = project.compile();
            let mut response = query_failure(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            );
            response["maps"] = json!({});
            response["references"] = json!([]);
            return response;
        }
    };
    let mut payload = query_payload_base(&snapshot);
    payload.insert("ok".into(), json!(!snapshot.result.has_errors()));
    payload.insert("maps".into(), json!(&snapshot.map_index.maps));
    payload.insert(
        "references".into(),
        json!(map_references(&snapshot.map_index)),
    );
    if !snapshot.conflicts.is_empty() {
        payload.insert("conflicts".into(), json!(snapshot.conflicts));
    }
    Value::Object(payload)
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
        "catalog": &result.analysis.catalog,
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

fn baseline_param(params: &Value) -> Result<Option<&str>, ProtoError> {
    params
        .get("baseline")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| ProtoError::new(-32602, "`baseline` 必须是字符串"))
        })
        .transpose()
}

/// 关系写入共享工作区边界:刷新外部文件、检查内容基线、故事诊断、工作区诊断
/// 和语言版本。所有关系写入入口都在这里之后才调用 core 的 Project 编辑 API。
fn prepare_relation_project(
    project: &mut Project,
    expected: Option<&str>,
) -> Result<(CompileResult, String, Vec<Diagnostic>), Value> {
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            let result = project.compile();
            return Err(project_failure_with_workspace(
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
        return Err(project_failure_with_workspace(
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
        return Err(project_failure_with_workspace(
            "STALE_BASELINE",
            format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.has_errors() {
        return Err(project_failure_with_workspace(
            "COMPILE_FAILED",
            "当前工程存在错误诊断，关系编辑未提交".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if !workspace_diagnostics.is_empty() {
        return Err(project_failure_with_workspace(
            "READ_ONLY",
            "工程清单或展示文档包含当前工具不支持的格式，只能只读查看".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.options.language_version != LanguageVersion::V1_10
        || project.language_version_kind() != LanguageVersion::V1_10
    {
        return Err(project_failure_with_workspace(
            "LANGUAGE_VERSION_REQUIRED",
            "关系编辑要求工程清单明确选择语言版本 1.10".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    Ok((before, baseline, workspace_diagnostics))
}

fn relation_type_id(
    params: &Value,
    operation: RelationTypeOperation,
) -> Result<String, ProtoError> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| {
            params
                .get("relation_type")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| ProtoError::new(-32602, "关系类型参数需要字符串 `id`"))?;
    if id.is_empty() {
        return Err(ProtoError::new(-32602, "关系类型 `id` 不能为空"));
    }
    if operation == RelationTypeOperation::Delete {
        return Ok(id.to_string());
    }
    if params
        .get("relation_type")
        .and_then(Value::as_object)
        .is_none()
    {
        return Err(ProtoError::new(-32602, "需要对象参数 `relation_type`"));
    }
    Ok(id.to_string())
}

fn relation_type_draft(
    params: &Value,
    existing: Option<&worldline_core::RelationTypeInfo>,
) -> Result<RelationTypeDraft, ProtoError> {
    let object = params
        .get("relation_type")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `relation_type`"))?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.id.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系类型参数需要字符串 `id`"))?;
    let display = object
        .get("display")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.display.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系类型参数需要字符串 `display`"))?;
    let inverse_display = if object.contains_key("inverse_display") {
        nullable_string(object.get("inverse_display"), "inverse_display")?
    } else {
        existing.and_then(|value| value.inverse_display.clone())
    };
    let direction = if let Some(value) = object.get("direction") {
        relation_direction(value)?
    } else {
        existing.map_or(RelationDirection::Directed, |value| value.direction)
    };
    let from_kind = if object.contains_key("from_kind") {
        nullable_string(object.get("from_kind"), "from_kind")?
    } else {
        existing.and_then(|value| value.from_kind.clone())
    };
    let to_kind = if object.contains_key("to_kind") {
        nullable_string(object.get("to_kind"), "to_kind")?
    } else {
        existing.and_then(|value| value.to_kind.clone())
    };
    Ok(RelationTypeDraft {
        id: id.to_string(),
        display: display.to_string(),
        inverse_display,
        direction,
        from_kind,
        to_kind,
    })
}

fn nullable_string(value: Option<&Value>, key: &str) -> Result<Option<String>, ProtoError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(ProtoError::new(
            -32602,
            format!("`{key}` 必须是字符串或 null"),
        )),
    }
}

fn relation_direction(value: &Value) -> Result<RelationDirection, ProtoError> {
    match value.as_str() {
        Some("directed") => Ok(RelationDirection::Directed),
        Some("undirected") => Ok(RelationDirection::Undirected),
        Some(value) => Err(ProtoError::new(
            -32602,
            format!("未知关系方向 `{value}`(可用: directed / undirected)"),
        )),
        None => Err(ProtoError::new(-32602, "`direction` 必须是字符串")),
    }
}

fn mutate_relation_type_project(
    project: &mut Project,
    params: &Value,
    operation: RelationTypeOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let (before, baseline, workspace_diagnostics) =
        match prepare_relation_project(project, expected) {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        };
    let id = relation_type_id(params, operation)?;
    let existing = before.analysis.catalog.relation_types.get(&id).cloned();
    if operation == RelationTypeOperation::Create && existing.is_some() {
        return Ok(project_failure_with_workspace(
            "RELATION_TYPE_EXISTS",
            format!("关系类型 `{id}` 已存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if matches!(
        operation,
        RelationTypeOperation::Update | RelationTypeOperation::Delete
    ) && existing.is_none()
    {
        return Ok(project_failure_with_workspace(
            "RELATION_TYPE_NOT_FOUND",
            format!("关系类型 `{id}` 不存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let snapshot = project.clone();
    if operation == RelationTypeOperation::Delete {
        if let Err(error) = project.remove_relation_type(&id) {
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
    } else {
        let draft = relation_type_draft(params, existing.as_ref())?;
        let original = (operation == RelationTypeOperation::Update).then_some(id.as_str());
        if let Err(error) = project.write_relation_type(original, &draft) {
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
    let relation_type = if operation == RelationTypeOperation::Delete {
        Value::Null
    } else {
        serde_json::to_value(after.analysis.catalog.relation_types.get(&id))
            .expect("RelationTypeInfo 可序列化")
    };
    Ok(relation_type_success(
        relation_type,
        operation_name(operation),
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
    ))
}

fn relation_id(params: &Value, operation: RelationOperation) -> Result<String, ProtoError> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| {
            params
                .get("relation")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| ProtoError::new(-32602, "关系参数需要字符串 `id`"))?;
    if id.is_empty() {
        return Err(ProtoError::new(-32602, "关系 `id` 不能为空"));
    }
    if operation != RelationOperation::Delete
        && params.get("relation").and_then(Value::as_object).is_none()
    {
        return Err(ProtoError::new(-32602, "需要对象参数 `relation`"));
    }
    Ok(id.to_string())
}

fn relation_target_value(value: &Value, key: &str) -> Result<TargetRef, ProtoError> {
    let (kind, id) = if let Some(text) = value.as_str() {
        text.split_once(':')
            .ok_or_else(|| ProtoError::new(-32602, format!("`{key}` 字符串格式必须为 KIND:ID")))?
    } else {
        let object = value.as_object().ok_or_else(|| {
            ProtoError::new(-32602, format!("`{key}` 必须是 KIND:ID 字符串或对象"))
        })?;
        (
            object
                .get("kind")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtoError::new(-32602, format!("`{key}.kind` 必须是字符串")))?,
            object
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtoError::new(-32602, format!("`{key}.id` 必须是字符串")))?,
        )
    };
    let kind = kind.trim();
    let id = id.trim();
    if kind.is_empty() || id.is_empty() || (kind != "file" && id.contains(':')) {
        return Err(ProtoError::new(
            -32602,
            format!("`{key}` 必须包含非空 KIND 和 ID"),
        ));
    }
    Ok(TargetRef::new(kind, id))
}

fn relation_draft(
    params: &Value,
    existing: Option<&worldline_core::SemanticRelationInfo>,
) -> Result<RelationDraft, ProtoError> {
    let object = params
        .get("relation")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `relation`"))?;
    relation_draft_object(object, existing, "relation")
}

fn relation_draft_object(
    object: &serde_json::Map<String, Value>,
    existing: Option<&worldline_core::SemanticRelationInfo>,
    label: &str,
) -> Result<RelationDraft, ProtoError> {
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.id.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系参数需要字符串 `id`"))?;
    let relation_type = object
        .get("relation_type")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.relation_type.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系参数需要字符串 `relation_type`"))?;
    let from = if let Some(value) = object.get("from") {
        relation_target_value(value, &format!("{label}.from"))?
    } else {
        existing
            .map(|value| value.from_ref.clone())
            .ok_or_else(|| ProtoError::new(-32602, "创建关系需要 `from`"))?
    };
    let to = if let Some(value) = object.get("to") {
        relation_target_value(value, &format!("{label}.to"))?
    } else {
        existing
            .map(|value| value.to_ref.clone())
            .ok_or_else(|| ProtoError::new(-32602, "创建关系需要 `to`"))?
    };
    let description = object
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.description.as_str()))
        .unwrap_or_default();
    let source_note = if object.contains_key("source_note") {
        nullable_string(object.get("source_note"), "source_note")?
    } else {
        existing.and_then(|value| value.source_note.clone())
    };
    let scope_refs = if let Some(value) = object.get("scope_refs") {
        let values = value
            .as_array()
            .ok_or_else(|| ProtoError::new(-32602, format!("`{label}.scope_refs` 必须是数组")))?;
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                relation_target_value(value, &format!("{label}.scope_refs[{index}]"))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        existing.map_or_else(Vec::new, |value| value.scope_refs.clone())
    };
    let properties = if let Some(value) = object.get("properties") {
        relation_properties_value(value, label)?
    } else {
        existing.map_or_else(Vec::new, |value| {
            value
                .properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
    };
    Ok(RelationDraft {
        id: id.to_string(),
        relation_type: relation_type.to_string(),
        from,
        to,
        description: description.to_string(),
        source_note,
        scope_refs,
        properties,
    })
}

fn relation_properties_value(
    value: &Value,
    label: &str,
) -> Result<Vec<(String, PropertyValue)>, ProtoError> {
    if let Some(values) = value.as_object() {
        return values
            .iter()
            .map(|(key, value)| Ok((key.clone(), property_value(value)?)))
            .collect::<Result<Vec<_>, ProtoError>>();
    }
    let Some(values) = value.as_array() else {
        return Err(ProtoError::new(
            -32602,
            format!("`{label}.properties` 必须是对象或键值数组"),
        ));
    };
    values
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let pair = item.as_array().ok_or_else(|| {
                ProtoError::new(
                    -32602,
                    format!("`{label}.properties[{index}]` 必须是 [name, value]"),
                )
            })?;
            if pair.len() != 2 {
                return Err(ProtoError::new(
                    -32602,
                    format!("`{label}.properties[{index}]` 必须是 [name, value]"),
                ));
            }
            let name = pair[0].as_str().ok_or_else(|| {
                ProtoError::new(
                    -32602,
                    format!("`{label}.properties[{index}][0]` 必须是字符串"),
                )
            })?;
            Ok((name.to_string(), property_value(&pair[1])?))
        })
        .collect()
}

fn mutate_relation_project(
    project: &mut Project,
    params: &Value,
    operation: RelationOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let (before, baseline, workspace_diagnostics) =
        match prepare_relation_project(project, expected) {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        };
    let id = relation_id(params, operation)?;
    let existing = before.analysis.catalog.relations.get(&id).cloned();
    if operation == RelationOperation::Create && existing.is_some() {
        return Ok(project_failure_with_workspace(
            "RELATION_EXISTS",
            format!("关系 `{id}` 已存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if matches!(
        operation,
        RelationOperation::Update | RelationOperation::Delete
    ) && existing.is_none()
    {
        return Ok(project_failure_with_workspace(
            "RELATION_NOT_FOUND",
            format!("关系 `{id}` 不存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let snapshot = project.clone();
    if operation == RelationOperation::Delete {
        if let Err(error) = project.remove_relation(&id) {
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
    } else {
        let draft = relation_draft(params, existing.as_ref())?;
        let original = (operation == RelationOperation::Update).then_some(id.as_str());
        if let Err(error) = project.write_relation(original, &draft) {
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
    let relation = if operation == RelationOperation::Delete {
        Value::Null
    } else {
        serde_json::to_value(after.analysis.catalog.relations.get(&id))
            .expect("SemanticRelationInfo 可序列化")
    };
    Ok(relation_success(
        relation,
        operation_name(operation),
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
    ))
}

fn operation_name<T>(operation: T) -> &'static str
where
    T: IntoOperationName,
{
    operation.into_operation_name()
}

trait IntoOperationName {
    fn into_operation_name(self) -> &'static str;
}

impl IntoOperationName for RelationOperation {
    fn into_operation_name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

impl IntoOperationName for RelationTypeOperation {
    fn into_operation_name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

fn relation_success(
    relation: Value,
    operation: &str,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": true,
        "operation": operation,
        "relation": relation,
        "catalog": &result.analysis.catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    })
}

fn relation_type_success(
    relation_type: Value,
    operation: &str,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": true,
        "operation": operation,
        "relation_type": relation_type,
        "catalog": &result.analysis.catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    })
}

fn promotion_legacy_handle(
    params: &Value,
    catalog: &worldline_core::Catalog,
) -> Result<LegacyRelationHandle, ProtoError> {
    let object = params
        .get("legacy")
        .or_else(|| params.get("handle"))
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `legacy`"))?;
    let source = object
        .get("source")
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要 `source`"))
        .and_then(|value| relation_target_value(value, "legacy.source"))?;
    let target = object
        .get("target")
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要 `target`"))
        .and_then(|value| relation_target_value(value, "legacy.target"))?;
    let label = object
        .get("label")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要字符串 `label`"))?;
    let occurrence = object
        .get("occurrence")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要整数 `occurrence`"))?;
    let occurrence =
        u32::try_from(occurrence).map_err(|_| ProtoError::new(-32602, "`occurrence` 超出范围"))?;
    catalog
        .legacy_relation_handles()
        .into_iter()
        .find(|handle| {
            handle.source == source
                && handle.target == target
                && handle.label == label
                && handle.occurrence == occurrence
        })
        .ok_or_else(|| ProtoError::new(-32602, "指定的旧人物关系句柄不存在"))
}

fn promotion_preview_value(params: &Value) -> Result<RelationPromotionPreview, ProtoError> {
    let value = params
        .get("preview")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "提交关系提升需要对象参数 `preview`"))?;
    let handle = value
        .get("handle")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要对象参数 `preview.handle`"))?;
    let source = handle
        .get("source")
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要 `source`"))
        .and_then(|value| relation_target_value(value, "preview.handle.source"))?;
    let target = handle
        .get("target")
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要 `target`"))
        .and_then(|value| relation_target_value(value, "preview.handle.target"))?;
    let label = handle
        .get("label")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要字符串 `label`"))?;
    let occurrence = handle
        .get("occurrence")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要整数 `occurrence`"))?;
    let occurrence =
        u32::try_from(occurrence).map_err(|_| ProtoError::new(-32602, "`occurrence` 超出范围"))?;
    let file = handle
        .get("file")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要字符串 `file`"))?;
    let line = handle
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要整数 `line`"))?;
    let line = u32::try_from(line).map_err(|_| ProtoError::new(-32602, "`line` 超出范围"))?;
    let relation_id = value
        .get("relation_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `relation_id`"))?;
    let content_baseline = value
        .get("content_baseline")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `content_baseline`"))?;
    let draft_value = value
        .get("draft")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要对象参数 `draft`"))?;
    let draft = relation_draft_object(draft_value, None, "preview.draft")?;
    let relation_type = value
        .get("relation_type")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `relation_type`"))?;
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `description`"))?;
    let source_note = nullable_string(value.get("source_note"), "source_note")?;
    let before_fingerprint = value
        .get("before_fingerprint")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要整数 `before_fingerprint`"))?;
    let after_fingerprint = value
        .get("after_fingerprint")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要整数 `after_fingerprint`"))?;
    let fingerprint_changed = value
        .get("fingerprint_changed")
        .and_then(Value::as_bool)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要布尔值 `fingerprint_changed`"))?;
    Ok(RelationPromotionPreview {
        handle: LegacyRelationHandle {
            source,
            target,
            label: label.to_string(),
            occurrence,
            file: file.to_string(),
            line,
        },
        content_baseline: content_baseline.to_string(),
        draft,
        relation_id: relation_id.to_string(),
        relation_type: relation_type.to_string(),
        description: description.to_string(),
        source_note,
        before_fingerprint,
        after_fingerprint,
        fingerprint_changed,
    })
}

fn promotion_draft(
    params: &Value,
    handle: &LegacyRelationHandle,
) -> Result<RelationDraft, ProtoError> {
    let source = params
        .get("relation")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `relation`"))?
        .clone();
    let mut object = source;
    object.insert(
        "from".into(),
        json!({"kind": handle.source.kind, "id": handle.source.id}),
    );
    object.insert(
        "to".into(),
        json!({"kind": handle.target.kind, "id": handle.target.id}),
    );
    if !object.contains_key("relation_type") {
        if let Some(relation_type) = object.remove("type") {
            object.insert("relation_type".into(), relation_type);
        }
    }
    if !object.contains_key("description") {
        object.insert("description".into(), json!(handle.label));
    }
    relation_draft_object(&object, None, "relation")
}

fn promote_relation_project(
    project: &mut Project,
    params: &Value,
    operation: PromotionOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let (before, baseline, workspace_diagnostics) =
        match prepare_relation_project(project, expected) {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        };
    let (preview, relation_id) =
        if operation == PromotionOperation::Commit && params.get("preview").is_some() {
            let preview = promotion_preview_value(params)?;
            let relation_id = preview.relation_id.clone();
            (preview, relation_id)
        } else {
            let handle = match promotion_legacy_handle(params, &before.analysis.catalog) {
                Ok(handle) => handle,
                Err(error) => {
                    return Ok(project_failure_with_workspace(
                        "LEGACY_RELATION_NOT_FOUND",
                        error.message,
                        Some(&before.diagnostics),
                        Some(baseline),
                        Some(before.options.language_version.as_str()),
                        &workspace_diagnostics,
                    ));
                }
            };
            let draft = promotion_draft(params, &handle)?;
            let preview = match project.preview_promote_legacy_relation(&handle, &draft) {
                Ok(preview) => preview,
                Err(error) => {
                    return Ok(project_failure_with_workspace(
                        "EDIT_FAILED",
                        error,
                        Some(&before.diagnostics),
                        Some(baseline),
                        Some(before.options.language_version.as_str()),
                        &workspace_diagnostics,
                    ));
                }
            };
            let relation_id = draft.id;
            (preview, relation_id)
        };
    if operation == PromotionOperation::Preview {
        return Ok(json!({
            "ok": true,
            "operation": "preview",
            "preview": preview,
            "catalog": &before.analysis.catalog,
            "diagnostics": before.diagnostics,
            "language_version": before.options.language_version.as_str(),
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": false,
        }));
    }
    let snapshot = project.clone();
    if let Err(error) = project.apply_legacy_relation_promotion(&preview) {
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
    Ok(json!({
        "ok": true,
        "operation": "commit",
        "preview": preview,
        "relation": serde_json::to_value(after.analysis.catalog.relations.get(&relation_id)).expect("SemanticRelationInfo 可序列化"),
        "catalog": &after.analysis.catalog,
        "diagnostics": after.diagnostics,
        "language_version": after.options.language_version.as_str(),
        "baseline": project.content_baseline(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    }))
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
        "catalog": &after.analysis.catalog,
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

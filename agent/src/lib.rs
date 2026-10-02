//! wl-agent —— worldline 机器协议入口。
//! 契约见 `worldline/spec/agent-protocol.md`:stdio 行分帧 JSON-RPC 2.0,
//! 单线程顺序处理;故事层失败以 `{"ok": false}` 结果表达,协议违规才用 error。

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use worldline_core::ast::PropertyValue;
use worldline_core::authoring::EntityDraft;
use worldline_core::authoring_intents::AuthoringIntent;
use worldline_core::catalog::TargetRef;
use worldline_core::localization::{LocalizationExchange, LocalizationSelection};
use worldline_core::project::Project;
use worldline_core::queries::{CatalogQuery, CatalogQueryCursor, CatalogQueryOptions};
use worldline_core::reader_export::ReaderExportSelection;
use worldline_core::{
    compile_path_with_options, compile_source, compile_source_with_options, Analysis,
    CompileOptions, CompileResult, Diagnostic, LanguageVersion, LegacyRelationHandle, Program,
    RelationDirection, RelationDraft, RelationPromotionPreview, RelationQueryDirection,
    RelationQueryOptions, RelationTypeDraft,
};
use worldline_runtime::{ReplayBudget, ReplayCancellation, ReplayTrace, Story};
#[path = "lib/authoring_intents.rs"]
mod authoring_intents;
#[path = "lib/catalog.rs"]
mod catalog;
#[path = "lib/entities.rs"]
mod entities;
#[path = "lib/localization.rs"]
mod localization;
#[path = "lib/markdown_import.rs"]
mod markdown_import;
#[path = "lib/projects.rs"]
mod projects;
#[path = "lib/reader_exports.rs"]
mod reader_exports;
#[path = "lib/relation_common.rs"]
mod relation_common;
#[path = "lib/relation_drafts.rs"]
mod relation_drafts;
#[path = "lib/relation_mutations.rs"]
mod relation_mutations;
#[path = "lib/relation_promotions.rs"]
mod relation_promotions;
#[path = "lib/relation_queries.rs"]
mod relation_queries;
#[path = "lib/relation_types.rs"]
mod relation_types;
#[path = "lib/scene.rs"]
mod scene;
#[path = "lib/sessions.rs"]
mod sessions;
#[path = "lib/source_edit.rs"]
mod source_edit;

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
    choice_presentation: bool,
    bounded_continue: bool,
    cancel_next: bool,
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
    scene_revision: worldline_core::presentation_commands::Revision,
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
        let msg: Value = match worldline_core::parse_unique_json(line.as_bytes()) {
            Ok(v) => v,
            Err(error) => {
                return Some(err(Value::Null, -32700, "解析失败", json!(error)));
            }
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
                "capabilities": [
                    worldline_runtime::CHOICE_PRESENTATION_CAPABILITY,
                    worldline_runtime::BOUNDED_CONTINUE_CAPABILITY,
                    worldline_core::scene_protocol::CAPABILITY,
                ],
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
            "trace.replay" => self.trace_replay(params),
            "session.continue" => self.session_continue(params),
            "session.cancel" => self.session_cancel(params),
            "session.choose" => self.session_choose(params),
            "session.state" => {
                let s = self.session(params)?;
                Ok(json!({ "state": s.story.state_view() }))
            }
            "session.trace" => {
                let s = self.session(params)?;
                Ok(json!({ "trace": s.story.replay_trace() }))
            }
            "session.checkpoint" => {
                let s = self.session(params)?;
                match s.story.checkpoint() {
                    Ok(checkpoint) => Ok(json!({ "checkpoint": checkpoint })),
                    Err(error) => Ok(json!({ "ok": false, "run_error": error })),
                }
            }
            "session.explain_choices" => {
                let include_evidence = match params.get("include_evidence") {
                    Some(_) => param_bool(params, "include_evidence")?,
                    None => false,
                };
                let s = self.session(params)?;
                if include_evidence {
                    return Ok(json!({ "choices": s.story.choice_evidence().unwrap_or(&[]) }));
                }
                match s.story.explain_choices() {
                    Ok(choices) => Ok(json!({ "choices": choices })),
                    Err(error) => Ok(json!({ "ok": false, "run_error": error })),
                }
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
                s.cancel_next = false;
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
            "scene.svg.preview" => self.scene(params, "svg-preview"),
            "scene.preview" => self.scene(params, "preview"),
            "scene.apply" => self.scene(params, "apply"),
            "scene.export" => self.scene(params, "export"),
            "source.edit.preview" => self.source_edit(params, false),
            "source.edit.apply" => self.source_edit(params, true),
            "schema.index" => self.schema_index(params),
            "schema.edit.preview" => self.schema_edit(params, false),
            "schema.edit.apply" => self.schema_edit(params, true),
            "authoring.intent.preview" => self.authoring_intent(params, false),
            "authoring.intent.apply" => self.authoring_intent(params, true),
            "markdown.import.preview" => self.markdown_import(params, false),
            "markdown.import.apply" => self.markdown_import(params, true),
            "catalog.query" => self.catalog_query(params),
            "reader.export.preview" | "reader.preview" => self.reader_export(params, false),
            "reader.export.apply" | "reader.export" => self.reader_export(params, true),
            "localization.export.preview" => self.localization_export(params, false),
            "localization.export.apply" => self.localization_export(params, true),
            "localization.import.preview" => self.localization_import(params, false),
            "localization.import.apply" => self.localization_import(params, true),
            "relation.query" | "relations.query" => self.relation_query(params),
            "relation.project" => self.relation_project(params),
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

fn param_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, ProtoError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, format!("需要字符串参数 `{key}`")))
}

fn optional_u64(params: &Value, key: &str) -> Result<Option<u64>, ProtoError> {
    match params.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| ProtoError::new(-32602, format!("参数 `{key}` 必须是非负 64 位整数"))),
    }
}

fn param_bool(params: &Value, key: &str) -> Result<bool, ProtoError> {
    params
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| ProtoError::new(-32602, format!("需要布尔参数 `{key}`")))
}

fn err(id: Value, code: i32, message: &str, data: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message, "data": data },
    })
}

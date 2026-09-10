//! wl-agent —— worldline 机器协议入口。
//! 契约见 `worldline/spec/agent-protocol.md`:stdio 行分帧 JSON-RPC 2.0,
//! 单线程顺序处理;故事层失败以 `{"ok": false}` 结果表达,协议违规才用 error。

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{json, Value};
use worldline_core::{compile_path, compile_source, Analysis, Program};
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
    next_story: u64,
    next_session: u64,
    shutdown: bool,
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
            "shutdown" => {
                self.shutdown = true;
                Ok(json!({ "bye": true }))
            }
            other => Err(ProtoError::new(-32601, format!("未知方法 `{other}`"))),
        }
    }

    fn compile(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let result = if let Some(p) = params.get("path").and_then(Value::as_str) {
            match compile_path(Path::new(p)) {
                Ok(r) => r,
                Err(e) => return Ok(json!({ "ok": false, "error": format!("无法读取 {p}:{e}") })),
            }
        } else if let Some(src) = params.get("source").and_then(Value::as_str) {
            let name = params
                .get("file_name")
                .and_then(Value::as_str)
                .unwrap_or("未命名.wl");
            compile_source(name, src)
        } else {
            return Err(ProtoError::new(-32602, "需要 `path` 或 `source` 参数"));
        };
        if result.has_errors() {
            return Ok(json!({ "ok": false, "diagnostics": result.diagnostics }));
        }
        self.next_story += 1;
        let story_id = format!("s{}", self.next_story);
        let program: &'static Program = Box::leak(Box::new(result.program));
        let analysis: &'static Analysis = Box::leak(Box::new(result.analysis));
        self.stories
            .insert(story_id.clone(), StoryUnit { program, analysis });
        Ok(json!({
            "ok": true,
            "story_id": story_id,
            "fingerprint": analysis.fingerprint,
            "stats": analysis.stats,
            "diagnostics": result.diagnostics,
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

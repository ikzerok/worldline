//! 对已打开 Project 的只读 pair 投影及完整 JSON-RPC 行字节保护。
use super::*;
use worldline_runtime::{compare_routes, RouteComparisonOptions, RouteStatus};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;
const ALLOWED: &[&str] = &["project_id", "left_trace", "right_trace", "max_steps", "time_budget_ms"];

fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"comparison":null,"error":{"code":code,"message":message.into()}})
}
fn output_failure() -> Value { failure("output_limit", "路线对照响应超过字节限制") }
fn result(id: &Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
fn protocol_failure(id: Value, code: i32) -> Value {
    err(id, code, "路线对照协议错误详情超过字节限制", Value::Null)
}

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    let id = message.get("id").cloned();
    if let Some(id) = &id {
        if !fits(&result(id, output_failure()), MAX_RESPONSE_BYTES - 1)
            || !fits(&protocol_failure(id.clone(), -32602), MAX_RESPONSE_BYTES - 1)
        {
            return Some(err(Value::Null, -32600, "路线对照请求标识超过响应字节限制", json!("request_id_exceeds_response_budget")));
        }
    }
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        let id = id.unwrap_or(Value::Null);
        return Some(bounded(err(id.clone(), -32600, "无效请求", json!("jsonrpc 必须为 \"2.0\"")), &id, Some(-32600)));
    }
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let outcome = server.compare_routes(&params);
    let id = id?;
    let (response, protocol) = match outcome {
        Ok(value) => (result(&id, value), None),
        Err(error) => (err(id.clone(), error.code, &error.message, error.data), Some(error.code)),
    };
    Some(bounded(response, &id, protocol))
}
fn bounded(response: Value, id: &Value, protocol: Option<i32>) -> Value {
    if fits(&response, MAX_RESPONSE_BYTES - 1) { return response; }
    match protocol {
        Some(code) => protocol_failure(id.clone(), code),
        None => result(id, output_failure()),
    }
}

impl Server {
    fn compare_routes(&self, params: &Value) -> Result<Value, ProtoError> {
        let object = params.as_object().ok_or_else(|| ProtoError::new(-32602, "路线对照参数必须是对象"))?;
        if let Some(key) = object.keys().find(|key| !ALLOWED.contains(&key.as_str())) {
            return Err(ProtoError::new(-32602, format!("未知路线对照参数 `{key}`")));
        }
        let project_id = param_str(params, "project_id")?;
        if project_id.trim().is_empty() { return Err(ProtoError::new(-32602, "project_id不能为空")); }
        let mut options = RouteComparisonOptions::default();
        options.budget.max_steps = optional_u64(params, "max_steps")?.unwrap_or(options.budget.max_steps);
        options.budget.time_budget_ms = optional_u64(params, "time_budget_ms")?.unwrap_or(options.budget.time_budget_ms);
        options.validate().map_err(|error| ProtoError::new(-32602, error.message))?;
        let left = trace(params, "left_trace", options.max_trace_bytes)?;
        let right = trace(params, "right_trace", options.max_trace_bytes)?;
        let unit = self.projects.get(project_id).ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
        let snapshot = match unit.project.compile_read_only() {
            Ok(snapshot) => snapshot,
            Err(message) => return Ok(failure("invalid_snapshot", message)),
        };
        if snapshot.has_errors() {
            let mut value = failure("COMPILE_FAILED", "当前稿件编译失败");
            value["diagnostics"] = json!(snapshot.diagnostics);
            value["workspace_diagnostics"] = json!(unit.project.authoring_diagnostics());
            return Ok(value);
        }
        match compare_routes(&snapshot, &left, &right, options, &ReplayCancellation::new()) {
            Ok(comparison) => {
                let ok = comparison.left.status == RouteStatus::Replayed && comparison.right.status == RouteStatus::Replayed;
                // runtime 流式预检后的 DTO 最多 1 MiB；不复制或重推导其中的语义字段。
                Ok(json!({"ok":ok,"comparison":comparison}))
            }
            Err(error) if matches!(error.code.as_str(), "invalid_options" | "input_limit" | "invalid_trace") => {
                Err(ProtoError::new(-32602, error.message))
            }
            Err(error) => Ok(failure(&error.code, error.message)),
        }
    }
}
fn trace(params: &Value, key: &str, limit: usize) -> Result<ReplayTrace, ProtoError> {
    let value = params.get(key).ok_or_else(|| ProtoError::new(-32602, format!("缺少 `{key}` DTO")))?;
    if !fits(value, limit) { return Err(ProtoError::new(-32602, format!("{key}超过trace输入字节限制"))); }
    serde_json::from_value(value.clone()).map_err(|error| ProtoError::new(-32602, format!("{key} DTO无效：{error}")))
}
fn fits(value: &Value, limit: usize) -> bool {
    serde_json::to_writer(&mut Counter { remaining: limit }, value).is_ok()
}
struct Counter { remaining: usize }
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining { return Err(std::io::Error::other("JSON超额")); }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

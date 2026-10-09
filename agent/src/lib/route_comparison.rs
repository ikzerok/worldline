//! 对已打开 Project 的只读 pair 投影及完整 JSON-RPC 行字节保护。
use super::*;
use worldline_runtime::{compare_routes, RouteComparisonOptions, RouteStatus};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;
const ALLOWED: &[&str] = &[
    "project_id",
    "left_trace",
    "right_trace",
    "max_steps",
    "time_budget_ms",
];

#[derive(serde::Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
}
#[derive(serde::Serialize)]
struct Failure<'a> {
    ok: bool,
    comparison: Option<&'a Value>,
    error: ErrorDetail<'a>,
}
#[derive(serde::Serialize)]
struct DiagnosticFailure<'a> {
    #[serde(flatten)]
    failure: Failure<'a>,
    diagnostics: &'a [Diagnostic],
    workspace_diagnostics: &'a [Diagnostic],
}
fn failure_payload<'a>(code: &'a str, message: &'a str) -> Failure<'a> {
    Failure {
        ok: false,
        comparison: None,
        error: ErrorDetail { code, message },
    }
}
fn failure(code: &str, message: &str, budget: usize) -> Value {
    bounded_value(&failure_payload(code, message), budget)
}
fn diagnostic_failure(
    code: &str,
    message: &str,
    diagnostics: &[Diagnostic],
    workspace: &[Diagnostic],
    budget: usize,
) -> Value {
    bounded_value(
        &DiagnosticFailure {
            failure: failure_payload(code, message),
            diagnostics,
            workspace_diagnostics: workspace,
        },
        budget,
    )
}
fn bounded_value(value: &impl serde::Serialize, budget: usize) -> Value {
    if !fits(value, budget) {
        return output_failure();
    }
    serde_json::to_value(value).unwrap_or_else(|_| output_failure())
}
fn output_failure() -> Value {
    json!({"ok":false,"comparison":null,"error":{"code":"output_limit","message":"路线对照响应超过字节限制"}})
}
fn result(id: &Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
fn protocol_failure(id: Value, code: i32) -> Value {
    err(id, code, "路线对照协议错误详情超过字节限制", Value::Null)
}

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    // 先借用并计量 id，不因巨大标识构造同样巨大的错误外壳副本。
    if let Some(id) = message.get("id") {
        let business =
            encoded_size(&result(&Value::Null, output_failure()), MAX_RESPONSE_BYTES).unwrap();
        let protocol =
            encoded_size(&protocol_failure(Value::Null, -32602), MAX_RESPONSE_BYTES).unwrap();
        let overhead = business.max(protocol) - 4; // 替换 null 的四个 bytes。
        if !fits(id, MAX_RESPONSE_BYTES - 1 - overhead) {
            return Some(err(
                Value::Null,
                -32600,
                "路线对照请求标识超过响应字节限制",
                json!("request_id_exceeds_response_budget"),
            ));
        }
    }
    let id = message.get("id").cloned();
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        let id = id.unwrap_or(Value::Null);
        return Some(bounded(
            err(
                id.clone(),
                -32600,
                "无效请求",
                json!("jsonrpc 必须为 \"2.0\""),
            ),
            &id,
            Some(-32600),
        ));
    }
    let empty = json!({});
    let params = message.get("params").unwrap_or(&empty);
    let id_bytes = id
        .as_ref()
        .map_or(4, |id| encoded_size(id, MAX_RESPONSE_BYTES).unwrap());
    let envelope = encoded_size(&result(&Value::Null, Value::Null), MAX_RESPONSE_BYTES).unwrap()
        - 8
        + id_bytes;
    let outcome = server.compare_routes(params, MAX_RESPONSE_BYTES - 1 - envelope);
    let id = id?;
    let (response, protocol) = match outcome {
        Ok(value) => (result(&id, value), None),
        Err(error) => (bounded_protocol(&id, &error), Some(error.code)),
    };
    Some(bounded(response, &id, protocol))
}
fn bounded(response: Value, id: &Value, protocol: Option<i32>) -> Value {
    if fits(&response, MAX_RESPONSE_BYTES - 1) {
        return response;
    }
    match protocol {
        Some(code) => protocol_failure(id.clone(), code),
        None => result(id, output_failure()),
    }
}

#[derive(serde::Serialize)]
struct ProtocolDetail<'a> {
    code: i32,
    message: &'a str,
    data: &'a Value,
}
#[derive(serde::Serialize)]
struct ProtocolEnvelope<'a> {
    jsonrpc: &'static str,
    id: &'a Value,
    error: ProtocolDetail<'a>,
}
fn bounded_protocol(id: &Value, error: &ProtoError) -> Value {
    let payload = ProtocolEnvelope {
        jsonrpc: "2.0",
        id,
        error: ProtocolDetail {
            code: error.code,
            message: &error.message,
            data: &error.data,
        },
    };
    if !fits(&payload, MAX_RESPONSE_BYTES - 1) {
        return protocol_failure(id.clone(), error.code);
    }
    serde_json::to_value(payload).unwrap_or_else(|_| protocol_failure(id.clone(), error.code))
}

impl Server {
    fn compare_routes(&self, params: &Value, response_budget: usize) -> Result<Value, ProtoError> {
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "路线对照参数必须是对象"))?;
        if object.keys().any(|key| !ALLOWED.contains(&key.as_str())) {
            return Err(ProtoError::new(-32602, "含未知路线对照参数"));
        }
        let project_id = param_str(params, "project_id")?;
        if project_id.trim().is_empty() {
            return Err(ProtoError::new(-32602, "project_id不能为空"));
        }
        let mut options = RouteComparisonOptions::default();
        options.budget.max_steps =
            optional_u64(params, "max_steps")?.unwrap_or(options.budget.max_steps);
        options.budget.time_budget_ms =
            optional_u64(params, "time_budget_ms")?.unwrap_or(options.budget.time_budget_ms);
        options
            .validate()
            .map_err(|error| ProtoError::new(-32602, error.message))?;
        let left = trace(params, "left_trace", options.max_trace_bytes)?;
        let right = trace(params, "right_trace", options.max_trace_bytes)?;
        let unit = self
            .projects
            .get(project_id)
            .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
        let snapshot = match unit.project.compile_read_only() {
            Ok(snapshot) => snapshot,
            Err(message) => {
                return Ok(diagnostic_failure(
                    "invalid_snapshot",
                    &message,
                    &[],
                    unit.project.authoring_diagnostics(),
                    response_budget,
                ));
            }
        };
        if snapshot.has_errors() {
            return Ok(diagnostic_failure(
                "COMPILE_FAILED",
                "当前稿件编译失败",
                &snapshot.diagnostics,
                unit.project.authoring_diagnostics(),
                response_budget,
            ));
        }
        let trace = if left.presentation.is_some() {
            &left
        } else {
            &right
        };
        let presentation = match localization_session::for_trace(Some(&unit.project), trace) {
            Ok(value) => value,
            Err(error) => {
                return Ok(failure(
                    error["error"]["code"]
                        .as_str()
                        .unwrap_or("LOCALIZATION_PREPARE_FAILED"),
                    error["error"]["message"]
                        .as_str()
                        .unwrap_or("译文展示准备失败"),
                    response_budget,
                ))
            }
        };
        let outcome = match &presentation {
            Some(presentation) => worldline_runtime::compare_routes_with_presentation(
                &snapshot,
                &left,
                &right,
                options,
                &ReplayCancellation::new(),
                presentation,
            ),
            None => compare_routes(
                &snapshot,
                &left,
                &right,
                options,
                &ReplayCancellation::new(),
            ),
        };
        match outcome {
            Ok(comparison) => {
                let ok = comparison.left.status == RouteStatus::Replayed
                    && comparison.right.status == RouteStatus::Replayed;
                // runtime 流式预检后的 DTO 最多 1 MiB；不复制或重推导其中的语义字段。
                Ok(json!({"ok":ok,"comparison":comparison}))
            }
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "invalid_options" | "input_limit" | "invalid_trace"
                ) =>
            {
                Err(ProtoError::new(-32602, error.message))
            }
            Err(error) => Ok(failure(&error.code, &error.message, response_budget)),
        }
    }
}
fn trace(params: &Value, key: &str, limit: usize) -> Result<ReplayTrace, ProtoError> {
    let value = params
        .get(key)
        .ok_or_else(|| ProtoError::new(-32602, format!("缺少 `{key}` DTO")))?;
    if !fits(value, limit) {
        return Err(ProtoError::new(
            -32602,
            format!("{key}超过trace输入字节限制"),
        ));
    }
    serde_json::from_value(value.clone())
        .map_err(|error| ProtoError::new(-32602, format!("{key} DTO无效：{error}")))
}
fn fits<T: serde::Serialize + ?Sized>(value: &T, limit: usize) -> bool {
    encoded_size(value, limit).is_some()
}
fn encoded_size<T: serde::Serialize + ?Sized>(value: &T, limit: usize) -> Option<usize> {
    let mut counter = Counter { remaining: limit };
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(limit - counter.remaining)
}
struct Counter {
    remaining: usize,
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("JSON超额"));
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

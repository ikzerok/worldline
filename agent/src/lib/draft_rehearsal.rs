//! 草稿试演是只读请求；完整机器外壳有独立预算，不能隐式保存。
use super::*;
use worldline_runtime::draft_rehearsal::{
    run_draft_rehearsal, DraftRehearsalRunRequest, MAX_DRAFT_REHEARSAL_REQUEST_BYTES,
    MAX_DRAFT_REHEARSAL_RESULT_BYTES,
};

const MAX_RESPONSE_BYTES: usize = MAX_DRAFT_REHEARSAL_RESULT_BYTES + 4096;
const MAX_ID_BYTES: usize = 2048;

pub(super) fn dispatch(server: &Server, message: &Value) -> Option<Value> {
    let borrowed_id = message.get("id");
    if borrowed_id.is_some_and(|id| !fits(id, MAX_ID_BYTES)) {
        return Some(err(
            Value::Null,
            -32600,
            "试演请求标识超过字节限制",
            Value::Null,
        ));
    }
    let id = borrowed_id.cloned();
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(err(
            id.unwrap_or(Value::Null),
            -32600,
            "jsonrpc 必须为 2.0",
            Value::Null,
        ));
    }
    let empty = json!({});
    let outcome = run(server, message.get("params").unwrap_or(&empty));
    let id = id?;
    let response = match outcome {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => err(id.clone(), error.code, &error.message, error.data),
    };
    if fits(&response, MAX_RESPONSE_BYTES - 1) {
        Some(response)
    } else {
        Some(json!({"jsonrpc":"2.0","id":id,"result":output_failure()}))
    }
}

fn run(server: &Server, params: &Value) -> Result<Value, ProtoError> {
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "试演参数必须是对象"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "project_id" | "request"))
    {
        return Err(ProtoError::new(-32602, "含未知试演参数"));
    }
    let project_id = param_str(params, "project_id")?;
    if project_id.is_empty() || project_id.len() > 256 {
        return Err(ProtoError::new(-32602, "project_id 长度无效"));
    }
    let value = params
        .get("request")
        .ok_or_else(|| ProtoError::new(-32602, "缺少 request DTO"))?;
    if !fits(value, MAX_DRAFT_REHEARSAL_REQUEST_BYTES) {
        return Err(ProtoError::new(-32602, "试演请求超过 32 MiB 字节限制"));
    }
    let request: DraftRehearsalRunRequest = serde_json::from_value(value.clone())
        .map_err(|error| ProtoError::new(-32602, format!("试演 DTO 无效：{error}")))?;
    request
        .validate()
        .map_err(|message| ProtoError::new(-32602, message))?;
    let unit = server
        .projects
        .get(project_id)
        .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
    let result = run_draft_rehearsal(&unit.project, &request, &ReplayCancellation::new())
        .map_err(|message| ProtoError::new(-32602, message))?;
    #[derive(serde::Serialize)]
    struct Envelope<'a> {
        ok: bool,
        applied: bool,
        saved: bool,
        result: &'a worldline_runtime::draft_rehearsal::DraftRehearsalRunResult,
    }
    let payload = Envelope {
        ok: result.ok,
        applied: false,
        saved: false,
        result: &result,
    };
    if !fits(&payload, MAX_DRAFT_REHEARSAL_RESULT_BYTES + 1024) {
        return Ok(output_failure());
    }
    serde_json::to_value(payload).map_err(|_| ProtoError::new(-32603, "无法编码试演结果"))
}

fn output_failure() -> Value {
    json!({"ok":false,"applied":false,"saved":false,"error":{"code":"OUTPUT_LIMIT","message":"草稿试演响应超过字节限制；未交付不完整证据"}})
}
fn fits<T: serde::Serialize + ?Sized>(value: &T, bytes: usize) -> bool {
    serde_json::to_writer(&mut Counter(bytes), value).is_ok()
}
struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0 {
            return Err(std::io::Error::other("试演JSON超额"));
        }
        self.0 -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

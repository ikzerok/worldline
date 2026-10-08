use super::*;

pub(super) const MAX_PARAMS: usize = 32 * 1024 * 1024;
pub(super) const MAX_PAYLOAD: usize = 32 * 1024 * 1024;
pub(super) const MAX_RESPONSE: usize = MAX_PAYLOAD + 4096;
const MAX_ID: usize = 2048;

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    let borrowed_id = message.get("id");
    if borrowed_id
        .is_some_and(|id| !fits(id, MAX_ID) || !(id.is_null() || id.is_string() || id.is_number()))
    {
        return Some(err(
            Value::Null,
            -32600,
            "外改请求标识无效或超过2048字节",
            Value::Null,
        ));
    }
    let id = borrowed_id.cloned();
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(err(
            id.unwrap_or(Value::Null),
            -32600,
            "jsonrpc必须为2.0",
            Value::Null,
        ));
    }
    if !fits(message, MAX_PARAMS + 4096) {
        return id.map(|id| err(id, -32602, "外改请求整体超过字节预算", Value::Null));
    }
    let empty = json!({});
    let params = message.get("params").unwrap_or(&empty);
    let operation = match message.get("method").and_then(Value::as_str) {
        Some("reconciliation.capture") => "capture",
        Some("reconciliation.preview") => "preview",
        Some("reconciliation.apply") => "apply",
        _ => return id.map(|id| err(id, -32601, "未知外改方法", Value::Null)),
    };
    let outcome = server.reconciliation(params, operation);
    let id = id?;
    let response = match outcome {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => err(id.clone(), error.code, &error.message, Value::Null),
    };
    if fits(&response, MAX_RESPONSE - 1) {
        return Some(response);
    }
    let applied = response
        .pointer("/result/applied")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let saved = response
        .pointer("/result/saved")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(json!({"jsonrpc":"2.0","id":id,"result":output_limit(applied,saved)}))
}

pub(super) fn check_params(params: &Value) -> Result<(), ProtoError> {
    if !fits(params, MAX_PARAMS) {
        return Err(ProtoError::new(-32602, "外改参数超过32 MiB编码预算"));
    }
    let id = param_str(params, "project_id")?;
    if id.is_empty() || id.len() > 256 {
        return Err(ProtoError::new(-32602, "project_id长度无效"));
    }
    if let Some(choices) = params.pointer("/request/choices").and_then(Value::as_array) {
        if choices.len() > 4096 {
            return Err(ProtoError::new(-32602, "外改选择超过4096文件预算"));
        }
        for choice in choices {
            if let Some(path) = choice.get("path").and_then(Value::as_str) {
                if path.len() > 4096 || path.split(['/', '\\']).count() > 128 {
                    return Err(ProtoError::new(-32602, "外改路径超过4096字节或128层预算"));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn encode(value: &impl serde::Serialize) -> Result<Value, Value> {
    if !fits(value, MAX_PAYLOAD) {
        return Err(output_limit(false, false));
    }
    serde_json::to_value(value).map_err(|_| output_limit(false, false))
}

pub(super) fn output_limit(applied: bool, saved: bool) -> Value {
    json!({"ok":false,"applied":applied,"saved":saved,"error":{"code":"OUTPUT_LIMIT",
        "message":"外改完整响应超过32 MiB编码预算；不返回截断候选"}})
}

pub(super) fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("reconciliation_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}

//! 有界只读作者审稿 RPC。业务失败不冒充协议错误。
use super::*;
use worldline_core::manuscript::{review_projection, MAX_REVIEW_JSON_BYTES};
const MAX_RESPONSE: usize = MAX_REVIEW_JSON_BYTES + 4096;

pub(super) fn dispatch(server: &Server, message: &Value) -> Option<Value> {
    let id = message.get("id");
    if id.is_some_and(|id| !fits(id, 3072)) {
        return Some(err(
            Value::Null,
            -32600,
            "审稿请求标识超过3072字节",
            Value::Null,
        ));
    }
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(err(
            id.cloned().unwrap_or(Value::Null),
            -32600,
            "无效请求",
            Value::Null,
        ));
    }
    let empty = json!({});
    let outcome = execute(server, message.get("params").unwrap_or(&empty));
    let id = id?.clone();
    let response = match outcome {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => err(id.clone(), error.code, &error.message, Value::Null),
    };
    if fits(&response, MAX_RESPONSE - 1) {
        return Some(response);
    }
    Some(json!({"jsonrpc":"2.0","id":id,"result":failure("review_limit", "审稿响应超过字节预算")}))
}
fn execute(server: &Server, params: &Value) -> Result<Value, ProtoError> {
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "审稿参数必须是对象"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "project_id" | "target"))
    {
        return Err(ProtoError::new(-32602, "未知审稿参数"));
    }
    let id = param_str(params, "project_id")?;
    let target = params
        .get("target")
        .ok_or_else(|| ProtoError::new(-32602, "缺少target"))?;
    if !fits(target, 4096) {
        return Err(ProtoError::new(-32602, "目标参数超过4096字节"));
    }
    let object = target
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "target必须是kind/id对象"))?;
    if object.len() != 2 {
        return Err(ProtoError::new(-32602, "target只能包含kind和id"));
    }
    let kind = param_str(target, "kind")?;
    let target_id = param_str(target, "id")?;
    if kind.is_empty() || target_id.is_empty() {
        return Err(ProtoError::new(-32602, "目标身份不能为空"));
    }
    let unit = server
        .projects
        .get(id)
        .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
    let result = match unit.project.compile_read_only() {
        Ok(result) => result,
        Err(message) => return Ok(failure("invalid_snapshot", &message)),
    };
    Ok(
        match review_projection(&result, &TargetRef::new(kind, target_id)) {
            Ok(review) => json!({"ok":true,"review":review,"error":null}),
            Err(error) => json!({"ok":false,"review":null,"error":error}),
        },
    )
}
fn failure(code: &str, message: &str) -> Value {
    json!({"ok":false,"review":null,"error":{"code":code,"message":message}})
}
fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("review_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}

#[cfg(test)]
mod tests;

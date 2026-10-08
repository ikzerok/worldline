//! 与Rust/CLI共用范围及材料；大响应不进入通用小响应截断路径。
use super::*;
use worldline_core::manuscript::{
    generate_manuscript_delivery, parse_manuscript_delivery_request, parse_manuscript_query_drafts,
    MAX_MANUSCRIPT_DELIVERY_RESPONSE_BYTES, MAX_MANUSCRIPT_QUERY_INPUT_BYTES,
};

pub(super) fn dispatch(server: &Server, message: &Value) -> Option<Value> {
    let id = message.get("id");
    if id.is_some_and(|id| !fits(id, 3072)) {
        return Some(err(
            Value::Null,
            -32600,
            "交付请求标识超过3072字节",
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
    let result = execute(server, message.get("params").unwrap_or(&empty));
    let id = id?.clone();
    let response = match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => err(id.clone(), error.code, &error.message, Value::Null),
    };
    if fits(&response, MAX_MANUSCRIPT_DELIVERY_RESPONSE_BYTES) {
        return Some(response);
    }
    Some(
        json!({"jsonrpc":"2.0","id":id,"result":failure("BUDGET_EXCEEDED", "完整响应超过32MiB预算；未截断交付")}),
    )
}
fn execute(server: &Server, params: &Value) -> Result<Value, ProtoError> {
    if !fits(params, MAX_MANUSCRIPT_QUERY_INPUT_BYTES) {
        return Err(ProtoError::new(-32602, "交付参数超过4MiB预算"));
    }
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "交付参数必须为对象"))?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "project_id" | "request" | "drafts"))
    {
        return Err(ProtoError::new(-32602, "未知交付参数"));
    }
    let id = param_str(params, "project_id")?;
    let request = params
        .get("request")
        .ok_or_else(|| ProtoError::new(-32602, "缺少request DTO"))?;
    let request = parse_manuscript_delivery_request(&request.to_string())
        .map_err(|error| ProtoError::new(-32602, error))?;
    let drafts = parse_manuscript_query_drafts(
        &params
            .get("drafts")
            .cloned()
            .unwrap_or_else(|| json!([]))
            .to_string(),
    )
    .map_err(|error| ProtoError::new(-32602, error))?;
    let unit = server
        .projects
        .get(id)
        .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
    let result = unit
        .project
        .manuscript_delivery_snapshot(&[], &drafts, &request)
        .and_then(|snapshot| generate_manuscript_delivery(snapshot, &mut |_| true));
    Ok(match result {
        Ok(report) => {
            json!({"ok":report.complete(),"report":report,"error":if report.complete() { Value::Null } else {
            json!({"code":"INCOMPLETE_DELIVERY","message":"范围或章节不完整；没有可交付Markdown"})
        },"applied":false,"saved":false,"delivered":false})
        }
        Err(error) => failure(&error.code, error.message),
    })
}
fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"report":null,"error":{"code":code,"message":message.into()},"applied":false,"saved":false,"delivered":false})
}
fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("delivery_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}

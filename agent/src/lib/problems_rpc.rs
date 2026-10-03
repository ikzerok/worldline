//! 只约束 project.problems 的完整 JSON-RPC 行；其他方法维持旧协议。
use super::*;

fn business_failure() -> Value {
    failure("BUDGET_EXCEEDED", "工程问题响应超过字节预算".into())
}
fn protocol_failure(id: Value, code: i32) -> Value {
    err(id, code, "工程问题错误详情超过字节预算", Value::Null)
}
fn result(id: &Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
fn wire_size(value: &Value) -> usize {
    value.to_string().len().saturating_add(1)
}

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    let id = message.get("id").cloned();
    if let Some(id) = &id {
        // Preflight the actual escaped identifier, before refresh/open/compile.
        if wire_size(&result(id, business_failure())) > MAX_RESPONSE_BYTES
            || wire_size(&protocol_failure(id.clone(), -32602)) > MAX_RESPONSE_BYTES
        {
            return Some(err(
                Value::Null,
                -32600,
                "工程问题请求标识超过响应字节预算",
                json!("request_id_exceeds_response_budget"),
            ));
        }
    }
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        let id = id.unwrap_or(Value::Null);
        let error = err(
            id.clone(),
            -32600,
            "无效请求",
            json!("jsonrpc 必须为 \"2.0\""),
        );
        return Some(bounded(error, &id, Some(-32600)));
    }
    let response_budget = id.as_ref().map_or(MAX_RESPONSE_BYTES, |id| {
        // The null result's four bytes are replaced by the method result.
        MAX_RESPONSE_BYTES - (wire_size(&result(id, Value::Null)) - 4)
    });
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let outcome = server.project_problems(&params, response_budget);
    let id = id?; // Notifications execute the same read path without a response.
    let (response, protocol_code) = match outcome {
        Ok(value) => (result(&id, value), None),
        Err(error) => (
            err(id.clone(), error.code, &error.message, error.data),
            Some(error.code),
        ),
    };
    Some(bounded(response, &id, protocol_code))
}
fn bounded(response: Value, id: &Value, protocol_code: Option<i32>) -> Value {
    if wire_size(&response) <= MAX_RESPONSE_BYTES {
        return response;
    }
    match protocol_code {
        Some(code) => protocol_failure(id.clone(), code),
        None => result(id, business_failure()),
    }
}

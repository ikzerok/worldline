//! 同 core 的条件作者协议；应用仅修改已打开 Project 内存。
use super::*;
use worldline_core::condition_authoring::{
    parse_condition_authoring_request, parse_condition_selector, MAX_CONDITION_OUTPUT_BYTES,
    MAX_CONDITION_REQUEST_BYTES,
};

const MAX_RESPONSE: usize = MAX_CONDITION_OUTPUT_BYTES + 4096;

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    if let Some(error) = dialogue::envelope_error(message) {
        return Some(error);
    }
    if message
        .get("id")
        .is_some_and(|id| !id.is_null() && !id.is_string() && !id.is_number())
    {
        return Some(err(Value::Null, -32600, "请求标识类型无效", Value::Null));
    }
    let empty = json!({});
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let operation = match message["method"].as_str() {
        Some("condition.authoring.query") => "query",
        Some("condition.authoring.preview") => "preview",
        Some("condition.authoring.apply") => "apply",
        _ => unreachable!("explicit method routing"),
    };
    let outcome = execute(
        server,
        message.get("params").unwrap_or(&empty),
        operation,
        &id,
    );
    message.get("id")?;
    let response = match outcome {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => err(id.clone(), error.code, &error.message, Value::Null),
    };
    Some(bounded_response(id, response))
}

fn bounded_response(id: Value, response: Value) -> Value {
    if fits(&response) {
        return response;
    }
    if let Some(code) = response
        .get("error")
        .and_then(|error| error["code"].as_i64())
    {
        return err(id, code as i32, "条件协议错误详情超过响应预算", Value::Null);
    }
    let mut failed = failure("OUTPUT_LIMIT", "完整条件响应超过额度；未截断");
    for key in ["applied", "saved"] {
        if let Some(actual) = response.get("result").and_then(|result| result.get(key)) {
            failed[key] = actual.clone();
        }
    }
    json!({"jsonrpc":"2.0","id":id,"result":failed})
}

fn execute(
    server: &mut Server,
    params: &Value,
    operation: &str,
    id: &Value,
) -> Result<Value, ProtoError> {
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "条件作者参数必须是对象"))?;
    if !dialogue::fits(params, MAX_CONDITION_REQUEST_BYTES + 8192) {
        return Err(ProtoError::new(-32602, "条件作者规范化参数超过预算"));
    }
    if object.keys().any(|key| match operation {
        "query" => !matches!(key.as_str(), "project_id" | "selector"),
        "preview" => !matches!(key.as_str(), "project_id" | "request"),
        "apply" => !matches!(key.as_str(), "project_id" | "request" | "plan_digest"),
        _ => true,
    }) {
        return Err(ProtoError::new(-32602, "未知条件作者参数"));
    }
    let project_id = param_str(params, "project_id")?;
    if project_id.is_empty() {
        return Err(ProtoError::new(-32602, "project_id 不能为空"));
    }
    if operation == "query" {
        let selector = parse_condition_selector(
            &params
                .get("selector")
                .ok_or_else(|| ProtoError::new(-32602, "需要 selector DTO"))?
                .to_string(),
        )
        .map_err(|error| ProtoError::new(-32602, error.to_string()))?;
        let unit = server
            .projects
            .get(project_id)
            .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
        return Ok(match unit.project.condition_authoring(&selector) {
            Ok(projection) => json!({"ok":true,"projection":projection,
                "baseline":unit.project.content_baseline(),"applied":false,"saved":false,"error":null}),
            Err(message) => failure("CONDITION_AUTHORING_REJECTED", message),
        });
    }
    let request = parse_condition_authoring_request(
        &params
            .get("request")
            .ok_or_else(|| ProtoError::new(-32602, "需要 request DTO"))?
            .to_string(),
    )
    .map_err(|error| ProtoError::new(-32602, error.to_string()))?;
    let digest = if operation == "apply" {
        let digest = param_str(params, "plan_digest")?;
        if digest.is_empty() || digest.len() > 256 {
            return Err(ProtoError::new(-32602, "无效 plan_digest"));
        }
        Some(digest)
    } else {
        None
    };
    let unit = server
        .projects
        .get_mut(project_id)
        .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
    let plan = match unit.project.preview_condition_authoring(&request) {
        Ok(plan) => plan,
        Err(message) => return Ok(failure("CONDITION_AUTHORING_REJECTED", message)),
    };
    if digest.is_some_and(|digest| digest != plan.plan_digest) {
        return Ok(failure(
            "CONDITION_AUTHORING_REJECTED",
            "计划摘要已过期或不符；没有应用",
        ));
    }
    let changed = digest.is_some() && plan.changed;
    let mut payload = json!({"ok":true,"operation":operation,"plan":plan,
        "baseline":unit.project.content_baseline(),"applied":changed,"saved":false,"error":null});
    // 成功响应（含原 id 与 LF）必须先通过预算，避免写入后才发现不能完整回报。
    if !fits(&json!({"jsonrpc":"2.0","id":id,"result":payload})) {
        return Ok(failure("OUTPUT_LIMIT", "完整条件响应超过额度；没有应用"));
    }
    if let Some(digest) = digest {
        if let Err(message) = unit.project.apply_condition_authoring(&request, digest) {
            return Ok(failure("CONDITION_AUTHORING_REJECTED", message));
        }
        if changed {
            unit.scene_revision.content_generation =
                unit.scene_revision.content_generation.wrapping_add(1);
        }
    }
    payload["baseline"] = json!(unit.project.content_baseline());
    Ok(payload)
}

fn fits(value: &impl serde::Serialize) -> bool {
    dialogue::fits(value, MAX_RESPONSE - 1)
}

fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message.into()},
        "applied":false,"saved":false})
}

#[cfg(test)]
#[path = "condition_authoring/tests.rs"]
mod tests;

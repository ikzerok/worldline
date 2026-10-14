//! 同 core 的资料约束协议；应用仅修改已打开 Project 内存。
use super::*;
use worldline_core::schemas::{
    parse_schema_authoring_request, parse_schema_authoring_target,
    MAX_SCHEMA_AUTHORING_OUTPUT_BYTES, MAX_SCHEMA_AUTHORING_REQUEST_BYTES,
};

const MAX_RESPONSE: usize = MAX_SCHEMA_AUTHORING_OUTPUT_BYTES + 4096;

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    if let Some(error) = dialogue::envelope_error(message) {
        return Some(error);
    }
    let empty = json!({});
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let operation = match message["method"].as_str() {
        Some("schema.authoring.query") => "query",
        Some("schema.authoring.preview") => "preview",
        Some("schema.authoring.apply") => "apply",
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
    Some(if fits(&response) {
        response
    } else {
        json!({"jsonrpc":"2.0","id":id,"result":failure("OUTPUT_LIMIT","完整资料响应超过额度；未截断")})
    })
}

fn execute(
    server: &mut Server,
    params: &Value,
    operation: &str,
    id: &Value,
) -> Result<Value, ProtoError> {
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "资料约束参数必须是对象"))?;
    if !dialogue::fits(params, MAX_SCHEMA_AUTHORING_REQUEST_BYTES + 8192) {
        return Err(ProtoError::new(-32602, "资料约束规范化参数超过预算"));
    }
    if object.keys().any(|key| match operation {
        "query" => !matches!(key.as_str(), "project_id" | "target"),
        "preview" => !matches!(key.as_str(), "project_id" | "request"),
        "apply" => !matches!(key.as_str(), "project_id" | "request" | "plan_digest"),
        _ => true,
    }) {
        return Err(ProtoError::new(-32602, "未知资料约束参数"));
    }
    let project_id = param_str(params, "project_id")?;
    if operation == "query" {
        let target = parse_schema_authoring_target(
            &params
                .get("target")
                .ok_or_else(|| ProtoError::new(-32602, "需要 target DTO"))?
                .to_string(),
        )
        .map_err(|error| ProtoError::new(-32602, error.to_string()))?;
        let unit = server
            .projects
            .get(project_id)
            .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
        return Ok(match unit.project.schema_authoring(&target) {
            Ok(projection) => json!({"ok":true,"projection":projection,
                "baseline":unit.project.content_baseline(),"applied":false,"saved":false,"error":null}),
            Err(message) => failure("SCHEMA_AUTHORING_REJECTED", message),
        });
    }
    let request = parse_schema_authoring_request(
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
    let plan = match unit.project.preview_schema_authoring(&request) {
        Ok(plan) => plan,
        Err(message) => return Ok(failure("SCHEMA_AUTHORING_REJECTED", message)),
    };
    if digest.is_some_and(|digest| digest != plan.plan_digest) {
        return Ok(failure(
            "SCHEMA_AUTHORING_REJECTED",
            "计划摘要已过期或不符；没有应用",
        ));
    }
    let changed = digest.is_some() && plan.changed;
    let mut payload = json!({"ok":true,"operation":operation,"plan":plan,
        "baseline":unit.project.content_baseline(),"applied":changed,"saved":false,"error":null});
    // 成功响应（含原 id 与 LF）必须先通过预算，避免写入后才发现不能完整回报。
    if !fits(&json!({"jsonrpc":"2.0","id":id,"result":payload})) {
        return Ok(failure("OUTPUT_LIMIT", "完整资料响应超过额度；没有应用"));
    }
    if let Some(digest) = digest {
        if let Err(message) = unit.project.apply_schema_authoring(&request, digest) {
            return Ok(failure("SCHEMA_AUTHORING_REJECTED", message));
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
#[path = "schema_authoring/tests.rs"]
mod tests;

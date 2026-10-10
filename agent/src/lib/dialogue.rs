//! 有状态对白计划；只修改本 server 的 Project，不隐式保存或刷新。
use super::*;
use worldline_core::manuscript::{parse_dialogue_edit_request, parse_dialogue_target};

pub(super) const MAX_RESPONSE: usize = 64 * 1024 * 1024;

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    if let Some(error) = envelope_error(message) {
        return Some(error);
    }
    let empty = json!({});
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let operation = match message["method"].as_str() {
        Some("dialogue.query") => "query",
        Some("dialogue.edit.preview") => "preview",
        Some("dialogue.edit.apply") => "apply",
        _ => unreachable!("explicit method routing"),
    };
    let outcome = execute(
        server,
        message.get("params").unwrap_or(&empty),
        operation,
        &id,
    );
    message.get("id")?;
    Some(response(id, outcome))
}

fn execute(
    server: &mut Server,
    params: &Value,
    operation: &str,
    id: &Value,
) -> Result<Value, ProtoError> {
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "对白参数必须是对象"))?;
    if !fits(params, 64 * 1024 + 8192) {
        return Err(ProtoError::new(-32602, "对白规范化参数超过预算"));
    }
    if object.keys().any(|key| match operation {
        "query" => !matches!(key.as_str(), "project_id" | "target"),
        "preview" => !matches!(key.as_str(), "project_id" | "request"),
        "apply" => !matches!(key.as_str(), "project_id" | "request" | "plan_digest"),
        _ => true,
    }) {
        return Err(ProtoError::new(-32602, "未知对白参数"));
    }
    let project_id = param_str(params, "project_id")?;
    if operation == "query" {
        let target = parse_dialogue_target(
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
        let buffer = match unit.project.open_writing_buffer(&target) {
            Ok(buffer) => buffer,
            Err(message) => return Ok(failure("SOURCE_UNAVAILABLE", message)),
        };
        return Ok(
            match unit.project.project_dialogue_buffer(&buffer, &target) {
                Ok(projection) => json!({"ok":true,"projection":projection,
                "baseline":unit.project.content_baseline(),"applied":false,"saved":false,"error":null}),
                Err(error) => failure(&error.code, error.message),
            },
        );
    }
    let request = parse_dialogue_edit_request(
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
    let buffer = match unit.project.open_writing_buffer(&request.target) {
        Ok(buffer) => buffer,
        Err(message) => return Ok(failure("SOURCE_UNAVAILABLE", message)),
    };
    let plan = match unit.project.preview_dialogue_edit(&buffer, &request) {
        Ok(plan) => plan,
        Err(error) => return Ok(failure(&error.code, error.message)),
    };
    if digest.is_some_and(|digest| digest != plan.plan_digest) {
        return Ok(failure(
            "STALE_DRAFT",
            "预览摘要已过期，请重新预览；没有应用",
        ));
    }
    let changed = digest.is_some() && !plan.no_change;
    let mut payload = json!({"ok":true,"operation":operation,"plan":plan,
        "baseline":unit.project.content_baseline(),"applied":changed,"saved":false,"error":null});
    // 完整未来成功 envelope（含请求 id）先通过预算，才允许任何内存提交。
    if !fits(
        &json!({"jsonrpc":"2.0","id":id,"result":payload}),
        MAX_RESPONSE,
    ) {
        return Ok(failure(
            "BUDGET_EXCEEDED",
            "完整对白响应超过64MiB；没有应用",
        ));
    }
    if digest.is_some() {
        if let Err(error) = unit.project.apply_dialogue_edit(&buffer, &plan) {
            return Ok(failure(&error.code, error.message));
        }
        if changed {
            unit.scene_revision.content_generation =
                unit.scene_revision.content_generation.wrapping_add(1);
            if plan.migration.is_some() {
                unit.scene_revision.presentation_generation =
                    unit.scene_revision.presentation_generation.wrapping_add(1);
            }
        }
    }
    payload["baseline"] = json!(unit.project.content_baseline());
    Ok(payload)
}

pub(super) fn envelope_error(message: &Value) -> Option<Value> {
    if message.get("id").is_some_and(|id| !fits(id, 3072)) {
        return Some(err(
            Value::Null,
            -32600,
            "请求标识超过3072字节",
            Value::Null,
        ));
    }
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(err(
            message.get("id").cloned().unwrap_or(Value::Null),
            -32600,
            "无效请求",
            Value::Null,
        ));
    }
    None
}

pub(super) fn response(id: Value, outcome: Result<Value, ProtoError>) -> Value {
    let response = match outcome {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => err(id.clone(), error.code, &error.message, Value::Null),
    };
    if fits(&response, MAX_RESPONSE) {
        return response;
    }
    let mut failed = failure("BUDGET_EXCEEDED", "完整响应超过64MiB；未截断");
    for key in ["applied", "saved", "delivered"] {
        if let Some(value) = response.get("result").and_then(|result| result.get(key)) {
            failed[key] = value.clone();
        }
    }
    json!({"jsonrpc":"2.0","id":id,"result":failed})
}

pub(super) fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message.into()},
        "applied":false,"saved":false,"delivered":false})
}

pub(super) fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("response_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}

#[cfg(test)]
#[path = "dialogue/tests.rs"]
mod tests;

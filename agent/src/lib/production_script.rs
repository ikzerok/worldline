//! 制作台本 RPC 返回 core 的精确字节，不自动写文件或发送第三方。
use super::dialogue::{envelope_error, failure, fits, response};
use super::*;
use worldline_core::manuscript::parse_manuscript_query_drafts;
use worldline_core::production_script::{
    parse_production_export_options, parse_production_script_request,
};

pub(super) fn dispatch(server: &Server, message: &Value) -> Option<Value> {
    if let Some(error) = envelope_error(message) {
        return Some(error);
    }
    let empty = json!({});
    let export = message["method"] == "production.script.export";
    let outcome = execute(server, message.get("params").unwrap_or(&empty), export);
    let id = message.get("id")?.clone();
    Some(response(id, outcome))
}

fn execute(server: &Server, params: &Value, export: bool) -> Result<Value, ProtoError> {
    if !fits(params, 4 * 1024 * 1024) {
        return Err(ProtoError::new(-32602, "台本规范化参数超过4MiB预算"));
    }
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "台本参数必须为对象"))?;
    if object.keys().any(|key| {
        !matches!(key.as_str(), "project_id" | "request" | "drafts")
            && !(export && key == "options")
            && !(!export && matches!(key.as_str(), "offset" | "limit"))
    }) {
        return Err(ProtoError::new(-32602, "未知台本参数"));
    }
    let id = param_str(params, "project_id")?;
    let request = parse_production_script_request(
        &params
            .get("request")
            .ok_or_else(|| ProtoError::new(-32602, "缺少 request DTO"))?
            .to_string(),
    )
    .map_err(|message| ProtoError::new(-32602, message))?;
    let drafts = parse_manuscript_query_drafts(
        &params
            .get("drafts")
            .cloned()
            .unwrap_or_else(|| json!([]))
            .to_string(),
    )
    .map_err(|message| ProtoError::new(-32602, message))?;
    let options = if export {
        Some(
            parse_production_export_options(
                &params
                    .get("options")
                    .ok_or_else(|| ProtoError::new(-32602, "缺少 options DTO"))?
                    .to_string(),
            )
            .map_err(|message| ProtoError::new(-32602, message))?,
        )
    } else {
        None
    };
    let offset = number(params, "offset", 0)?;
    let limit = number(params, "limit", 50)?;
    let unit = server
        .projects
        .get(id)
        .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
    let snapshot = match unit
        .project
        .production_script_snapshot(&[], &drafts, &request)
    {
        Ok(snapshot) => snapshot,
        Err(error) => return Ok(failure(&error.code, error.message)),
    };
    let Some(options) = options else {
        return Ok(match snapshot.page(offset, limit) {
            Ok(page) => json!({"ok":true,"page":page,"baseline":unit.project.content_baseline(),
                "applied":false,"saved":false,"delivered":false,"error":null}),
            Err(error) => failure(&error.code, error.message),
        });
    };
    Ok(match snapshot.export(&options) {
        Ok(artifact) => match std::str::from_utf8(artifact.bytes()) {
            Ok(text) => {
                json!({"ok":true,"artifact":{"format":artifact.format(),"snapshot_key":artifact.snapshot_key(),
                "text":text,"byte_count":artifact.bytes().len()},"baseline":unit.project.content_baseline(),
                "applied":false,"saved":false,"delivered":false,"error":null})
            }
            Err(_) => failure("INVALID_ARTIFACT", "core 台本不是有效UTF-8；没有材料"),
        },
        Err(error) => failure(&error.code, error.message),
    })
}

fn number(params: &Value, key: &str, default: usize) -> Result<usize, ProtoError> {
    match params.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|number| usize::try_from(number).ok())
            .ok_or_else(|| ProtoError::new(-32602, format!("{key} 必须为范围内的非负整数"))),
    }
}

#[cfg(test)]
#[path = "production_script/tests.rs"]
mod tests;

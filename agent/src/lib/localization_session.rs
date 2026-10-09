//! RPC capability decoding and core-only locale preparation.
use super::*;
use worldline_core::localization::{
    LocalizationPresentationRequest, LocalizationPresentationSnapshot,
};

pub(super) fn requested(
    params: &Value,
) -> Result<Option<LocalizationPresentationRequest>, ProtoError> {
    let Some(value) = params.get("localization") else {
        return Ok(None);
    };
    let enabled = params
        .get("capabilities")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().any(|value| {
                value.as_str() == Some(worldline_runtime::LOCALIZATION_PRESENTATION_CAPABILITY)
            })
        });
    if !enabled {
        return Err(ProtoError::new(
            -32602,
            "locale 会话需先申请 runtime.localization.v1",
        ));
    }
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|error| ProtoError::new(-32602, format!("localization DTO 无效：{error}")))
}

pub(super) fn prepare(
    project: Option<&Project>,
    request: &LocalizationPresentationRequest,
) -> Result<LocalizationPresentationSnapshot, Value> {
    let project = project.ok_or_else(|| json!({"ok":false,"error":{
        "code":"LOCALIZATION_WORKSPACE_REQUIRED","message":"locale 体验需要从有本地化文档的工程编译"}}))?;
    project
        .prepare_localization_presentation(request)
        .map_err(|error| json!({"ok":false,"error":error}))
}

pub(super) fn for_trace(
    project: Option<&Project>,
    trace: &ReplayTrace,
) -> Result<Option<LocalizationPresentationSnapshot>, Value> {
    trace
        .presentation
        .as_ref()
        .map(|identity| prepare(project, &identity.request))
        .transpose()
}

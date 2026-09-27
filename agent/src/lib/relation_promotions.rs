use super::projects::{project_failure, project_failure_with_workspace};
use super::relation_common::{baseline_param, prepare_relation_project};
use super::relation_drafts::{relation_draft_object, relation_target_value};
use super::relation_types::nullable_string;
use super::*;
impl Server {
    pub(super) fn relation_promotion(
        &mut self,
        params: &Value,
        operation: PromotionOperation,
    ) -> Result<Value, ProtoError> {
        let expected = baseline_param(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return promote_relation_project(&mut unit.project, params, operation, expected);
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        promote_relation_project(&mut project, params, operation, expected)
    }
}
fn promotion_legacy_handle(
    params: &Value,
    catalog: &worldline_core::Catalog,
) -> Result<LegacyRelationHandle, ProtoError> {
    let object = params
        .get("legacy")
        .or_else(|| params.get("handle"))
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `legacy`"))?;
    let source = object
        .get("source")
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要 `source`"))
        .and_then(|value| relation_target_value(value, "legacy.source"))?;
    let target = object
        .get("target")
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要 `target`"))
        .and_then(|value| relation_target_value(value, "legacy.target"))?;
    let label = object
        .get("label")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要字符串 `label`"))?;
    let occurrence = object
        .get("occurrence")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "旧关系句柄需要整数 `occurrence`"))?;
    let occurrence =
        u32::try_from(occurrence).map_err(|_| ProtoError::new(-32602, "`occurrence` 超出范围"))?;
    catalog
        .legacy_relation_handles()
        .into_iter()
        .find(|handle| {
            handle.source == source
                && handle.target == target
                && handle.label == label
                && handle.occurrence == occurrence
        })
        .ok_or_else(|| ProtoError::new(-32602, "指定的旧人物关系句柄不存在"))
}

fn promotion_preview_value(params: &Value) -> Result<RelationPromotionPreview, ProtoError> {
    let value = params
        .get("preview")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "提交关系提升需要对象参数 `preview`"))?;
    let handle = value
        .get("handle")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要对象参数 `preview.handle`"))?;
    let source = handle
        .get("source")
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要 `source`"))
        .and_then(|value| relation_target_value(value, "preview.handle.source"))?;
    let target = handle
        .get("target")
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要 `target`"))
        .and_then(|value| relation_target_value(value, "preview.handle.target"))?;
    let label = handle
        .get("label")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要字符串 `label`"))?;
    let occurrence = handle
        .get("occurrence")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要整数 `occurrence`"))?;
    let occurrence =
        u32::try_from(occurrence).map_err(|_| ProtoError::new(-32602, "`occurrence` 超出范围"))?;
    let file = handle
        .get("file")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要字符串 `file`"))?;
    let line = handle
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升句柄需要整数 `line`"))?;
    let line = u32::try_from(line).map_err(|_| ProtoError::new(-32602, "`line` 超出范围"))?;
    let relation_id = value
        .get("relation_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `relation_id`"))?;
    let content_baseline = value
        .get("content_baseline")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `content_baseline`"))?;
    let draft_value = value
        .get("draft")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要对象参数 `draft`"))?;
    let draft = relation_draft_object(draft_value, None, "preview.draft")?;
    let relation_type = value
        .get("relation_type")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `relation_type`"))?;
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要字符串 `description`"))?;
    let source_note = nullable_string(value.get("source_note"), "source_note")?;
    let before_fingerprint = value
        .get("before_fingerprint")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要整数 `before_fingerprint`"))?;
    let after_fingerprint = value
        .get("after_fingerprint")
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要整数 `after_fingerprint`"))?;
    let fingerprint_changed = value
        .get("fingerprint_changed")
        .and_then(Value::as_bool)
        .ok_or_else(|| ProtoError::new(-32602, "提升预览需要布尔值 `fingerprint_changed`"))?;
    Ok(RelationPromotionPreview {
        handle: LegacyRelationHandle {
            source,
            target,
            label: label.to_string(),
            occurrence,
            file: file.to_string(),
            line,
        },
        content_baseline: content_baseline.to_string(),
        draft,
        relation_id: relation_id.to_string(),
        relation_type: relation_type.to_string(),
        description: description.to_string(),
        source_note,
        before_fingerprint,
        after_fingerprint,
        fingerprint_changed,
    })
}

fn promotion_draft(
    params: &Value,
    handle: &LegacyRelationHandle,
) -> Result<RelationDraft, ProtoError> {
    let source = params
        .get("relation")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `relation`"))?
        .clone();
    let mut object = source;
    object.insert(
        "from".into(),
        json!({"kind": handle.source.kind, "id": handle.source.id}),
    );
    object.insert(
        "to".into(),
        json!({"kind": handle.target.kind, "id": handle.target.id}),
    );
    if !object.contains_key("relation_type") {
        if let Some(relation_type) = object.remove("type") {
            object.insert("relation_type".into(), relation_type);
        }
    }
    if !object.contains_key("description") {
        object.insert("description".into(), json!(handle.label));
    }
    relation_draft_object(&object, None, "relation")
}

fn promote_relation_project(
    project: &mut Project,
    params: &Value,
    operation: PromotionOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let (before, baseline, workspace_diagnostics) =
        match prepare_relation_project(project, expected) {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        };
    let (preview, relation_id) =
        if operation == PromotionOperation::Commit && params.get("preview").is_some() {
            let preview = promotion_preview_value(params)?;
            let relation_id = preview.relation_id.clone();
            (preview, relation_id)
        } else {
            let handle = match promotion_legacy_handle(params, &before.analysis.catalog) {
                Ok(handle) => handle,
                Err(error) => {
                    return Ok(project_failure_with_workspace(
                        "LEGACY_RELATION_NOT_FOUND",
                        error.message,
                        Some(&before.diagnostics),
                        Some(baseline),
                        Some(before.options.language_version.as_str()),
                        &workspace_diagnostics,
                    ));
                }
            };
            let draft = promotion_draft(params, &handle)?;
            let preview = match project.preview_promote_legacy_relation(&handle, &draft) {
                Ok(preview) => preview,
                Err(error) => {
                    return Ok(project_failure_with_workspace(
                        "EDIT_FAILED",
                        error,
                        Some(&before.diagnostics),
                        Some(baseline),
                        Some(before.options.language_version.as_str()),
                        &workspace_diagnostics,
                    ));
                }
            };
            let relation_id = draft.id;
            (preview, relation_id)
        };
    if operation == PromotionOperation::Preview {
        return Ok(json!({
            "ok": true,
            "operation": "preview",
            "preview": preview,
            "catalog": &before.analysis.catalog,
            "diagnostics": before.diagnostics,
            "language_version": before.options.language_version.as_str(),
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": false,
        }));
    }
    let snapshot = project.clone();
    if let Err(error) = project.apply_legacy_relation_promotion(&preview) {
        *project = snapshot;
        return Ok(project_failure_with_workspace(
            "EDIT_FAILED",
            error,
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if let Err(error) = project.save() {
        *project = snapshot;
        return Ok(project_failure_with_workspace(
            "CONFLICT",
            error,
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let after = project.compile();
    Ok(json!({
        "ok": true,
        "operation": "commit",
        "preview": preview,
        "relation": serde_json::to_value(after.analysis.catalog.relations.get(&relation_id)).expect("SemanticRelationInfo 可序列化"),
        "catalog": &after.analysis.catalog,
        "diagnostics": after.diagnostics,
        "language_version": after.options.language_version.as_str(),
        "baseline": project.content_baseline(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    }))
}

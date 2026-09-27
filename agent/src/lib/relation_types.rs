use super::projects::{project_failure, project_failure_with_workspace};
use super::relation_common::{
    baseline_param, operation_name, prepare_relation_project, relation_type_success,
};
use super::*;
impl Server {
    pub(super) fn relation_type_mutation(
        &mut self,
        params: &Value,
        operation: RelationTypeOperation,
    ) -> Result<Value, ProtoError> {
        let expected = baseline_param(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return mutate_relation_type_project(&mut unit.project, params, operation, expected);
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        mutate_relation_type_project(&mut project, params, operation, expected)
    }
}
fn relation_type_id(
    params: &Value,
    operation: RelationTypeOperation,
) -> Result<String, ProtoError> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| {
            params
                .get("relation_type")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| ProtoError::new(-32602, "关系类型参数需要字符串 `id`"))?;
    if id.is_empty() {
        return Err(ProtoError::new(-32602, "关系类型 `id` 不能为空"));
    }
    if operation == RelationTypeOperation::Delete {
        return Ok(id.to_string());
    }
    if params
        .get("relation_type")
        .and_then(Value::as_object)
        .is_none()
    {
        return Err(ProtoError::new(-32602, "需要对象参数 `relation_type`"));
    }
    Ok(id.to_string())
}

fn relation_type_draft(
    params: &Value,
    existing: Option<&worldline_core::RelationTypeInfo>,
) -> Result<RelationTypeDraft, ProtoError> {
    let object = params
        .get("relation_type")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `relation_type`"))?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.id.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系类型参数需要字符串 `id`"))?;
    let display = object
        .get("display")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.display.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系类型参数需要字符串 `display`"))?;
    let inverse_display = if object.contains_key("inverse_display") {
        nullable_string(object.get("inverse_display"), "inverse_display")?
    } else {
        existing.and_then(|value| value.inverse_display.clone())
    };
    let direction = if let Some(value) = object.get("direction") {
        relation_direction(value)?
    } else {
        existing.map_or(RelationDirection::Directed, |value| value.direction)
    };
    let from_kind = if object.contains_key("from_kind") {
        nullable_string(object.get("from_kind"), "from_kind")?
    } else {
        existing.and_then(|value| value.from_kind.clone())
    };
    let to_kind = if object.contains_key("to_kind") {
        nullable_string(object.get("to_kind"), "to_kind")?
    } else {
        existing.and_then(|value| value.to_kind.clone())
    };
    Ok(RelationTypeDraft {
        id: id.to_string(),
        display: display.to_string(),
        inverse_display,
        direction,
        from_kind,
        to_kind,
    })
}

pub(super) fn nullable_string(
    value: Option<&Value>,
    key: &str,
) -> Result<Option<String>, ProtoError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(ProtoError::new(
            -32602,
            format!("`{key}` 必须是字符串或 null"),
        )),
    }
}

fn relation_direction(value: &Value) -> Result<RelationDirection, ProtoError> {
    match value.as_str() {
        Some("directed") => Ok(RelationDirection::Directed),
        Some("undirected") => Ok(RelationDirection::Undirected),
        Some(value) => Err(ProtoError::new(
            -32602,
            format!("未知关系方向 `{value}`(可用: directed / undirected)"),
        )),
        None => Err(ProtoError::new(-32602, "`direction` 必须是字符串")),
    }
}

fn mutate_relation_type_project(
    project: &mut Project,
    params: &Value,
    operation: RelationTypeOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let (before, baseline, workspace_diagnostics) =
        match prepare_relation_project(project, expected) {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        };
    let id = relation_type_id(params, operation)?;
    let existing = before.analysis.catalog.relation_types.get(&id).cloned();
    if operation == RelationTypeOperation::Create && existing.is_some() {
        return Ok(project_failure_with_workspace(
            "RELATION_TYPE_EXISTS",
            format!("关系类型 `{id}` 已存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if matches!(
        operation,
        RelationTypeOperation::Update | RelationTypeOperation::Delete
    ) && existing.is_none()
    {
        return Ok(project_failure_with_workspace(
            "RELATION_TYPE_NOT_FOUND",
            format!("关系类型 `{id}` 不存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let snapshot = project.clone();
    if operation == RelationTypeOperation::Delete {
        if let Err(error) = project.remove_relation_type(&id) {
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
    } else {
        let draft = relation_type_draft(params, existing.as_ref())?;
        let original = (operation == RelationTypeOperation::Update).then_some(id.as_str());
        if let Err(error) = project.write_relation_type(original, &draft) {
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
    let relation_type = if operation == RelationTypeOperation::Delete {
        Value::Null
    } else {
        serde_json::to_value(after.analysis.catalog.relation_types.get(&id))
            .expect("RelationTypeInfo 可序列化")
    };
    Ok(relation_type_success(
        relation_type,
        operation_name(operation),
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
    ))
}

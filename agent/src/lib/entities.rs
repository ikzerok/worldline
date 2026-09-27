use super::projects::{project_entry, project_failure, project_failure_with_workspace};
use super::*;
impl Server {
    pub(super) fn entity_mutation(
        &mut self,
        params: &Value,
        operation: EntityOperation,
    ) -> Result<Value, ProtoError> {
        let expected = params
            .get("baseline")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| ProtoError::new(-32602, "`baseline` 必须是字符串"))
            })
            .transpose()?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return mutate_entity_project(
                &mut unit.project,
                &unit.entry,
                params,
                operation,
                expected,
            );
        }
        let path = param_str(params, "path")?;
        let entry = match project_entry(Path::new(path)) {
            Ok(entry) => entry,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        mutate_entity_project(&mut project, &entry, params, operation, expected)
    }
}
fn mutate_entity_project(
    project: &mut Project,
    entry: &Path,
    params: &Value,
    operation: EntityOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            let result = project.compile();
            return Ok(project_failure_with_workspace(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            ));
        }
    };
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    let before = project.compile();
    let baseline = project.content_baseline();
    if !conflicts.is_empty() {
        return Ok(project_failure_with_workspace(
            "CONFLICT",
            format!(
                "工程存在外部修改冲突，拒绝覆盖：{}",
                conflicts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if expected.is_some_and(|value| value != baseline) {
        return Ok(project_failure_with_workspace(
            "STALE_BASELINE",
            format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.has_errors() {
        return Ok(project_failure_with_workspace(
            "COMPILE_FAILED",
            "当前工程存在错误诊断，实体编辑未提交".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if !project.authoring_diagnostics().is_empty() {
        return Ok(project_failure_with_workspace(
            "READ_ONLY",
            "工程清单包含当前工具不支持的格式或必需能力，只能只读查看".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.options.language_version != LanguageVersion::V1_10
        || project.language_version_kind() != LanguageVersion::V1_10
    {
        return Ok(project_failure_with_workspace(
            "LANGUAGE_VERSION_REQUIRED",
            "实体编辑要求工程清单明确选择语言版本 1.10".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let id = entity_id(params, operation)?;
    let existing = before.analysis.catalog.entities.get(&id).cloned();
    if operation == EntityOperation::Create && existing.is_some() {
        return Ok(project_failure_with_workspace(
            "ENTITY_EXISTS",
            format!("实体 `{id}` 已存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if matches!(operation, EntityOperation::Update | EntityOperation::Delete) && existing.is_none()
    {
        return Ok(project_failure_with_workspace(
            "ENTITY_NOT_FOUND",
            format!("实体 `{id}` 不存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let draft = if operation == EntityOperation::Delete {
        None
    } else {
        Some(entity_draft(params, existing.as_ref())?)
    };
    let snapshot = project.clone();
    let edit = project.edit(|candidate| match operation {
        EntityOperation::Create => candidate.write_entity(entry, None, draft.as_ref().unwrap()),
        EntityOperation::Update => {
            candidate.write_entity(entry, Some(id.as_str()), draft.as_ref().unwrap())
        }
        EntityOperation::Delete => candidate.remove_entity(&id),
    });
    if let Err(error) = edit {
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
    let after_baseline = project.content_baseline();
    let operation_name = match operation {
        EntityOperation::Create => "create",
        EntityOperation::Update => "update",
        EntityOperation::Delete => "delete",
    };
    Ok(json!({
        "ok": true,
        "operation": operation_name,
        "entity": if operation == EntityOperation::Delete {
            Value::Null
        } else {
            serde_json::to_value(after.analysis.catalog.entities.get(&id))
                .expect("EntityInfo 可序列化")
        },
        "catalog": &after.analysis.catalog,
        "language_version": after.options.language_version.as_str(),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
        "baseline": after_baseline,
    }))
}

fn entity_id(params: &Value, operation: EntityOperation) -> Result<String, ProtoError> {
    let id = match operation {
        EntityOperation::Delete => params.get("id"),
        EntityOperation::Create | EntityOperation::Update => {
            params.get("entity").and_then(|value| value.get("id"))
        }
    }
    .and_then(Value::as_str)
    .ok_or_else(|| ProtoError::new(-32602, "实体参数需要字符串 `id`"))?;
    if id.is_empty() {
        return Err(ProtoError::new(-32602, "实体 `id` 不能为空"));
    }
    Ok(id.to_string())
}

fn entity_draft(
    params: &Value,
    existing: Option<&worldline_core::catalog::EntityInfo>,
) -> Result<EntityDraft, ProtoError> {
    let entity = params
        .get("entity")
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `entity`"))?;
    if !entity.is_object() {
        return Err(ProtoError::new(-32602, "`entity` 必须是对象"));
    }
    let id = entity
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.id.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "实体参数需要字符串 `id`"))?;
    let entity_type = entity
        .get("entity_type")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.entity_type.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "实体参数需要字符串 `entity_type`"))?;
    let display = entity
        .get("display")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.display.as_str()))
        .unwrap_or(id);
    let description = entity
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| params.get("description").and_then(Value::as_str))
        .or_else(|| existing.map(|value| value.description.as_str()))
        .unwrap_or_default();
    let mut properties = existing
        .map(|value| {
            value
                .properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    if let Some(values) = entity.get("properties") {
        let values = values
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "实体 `properties` 必须是对象"))?;
        for (key, value) in values {
            properties.insert(key.clone(), property_value(value)?);
        }
    }
    Ok(EntityDraft {
        id: id.into(),
        entity_type: entity_type.into(),
        display: display.into(),
        description: description.into(),
        properties: properties.into_iter().collect(),
    })
}

pub(super) fn property_value(value: &Value) -> Result<PropertyValue, ProtoError> {
    match value {
        Value::String(value) => Ok(PropertyValue::Str(value.clone())),
        Value::Number(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(PropertyValue::Num)
            .ok_or_else(|| ProtoError::new(-32602, "property 数值必须是有限数值")),
        Value::Bool(value) => Ok(PropertyValue::Bool(*value)),
        _ => Err(ProtoError::new(
            -32602,
            "property 值只能是字符串、数值或布尔值",
        )),
    }
}

use super::projects::{project_failure, project_failure_with_workspace};
use super::relation_common::{
    baseline_param, operation_name, prepare_relation_project, relation_success,
};
use super::relation_drafts::{relation_draft, relation_id};
use super::*;
impl Server {
    pub(super) fn relation_mutation(
        &mut self,
        params: &Value,
        operation: RelationOperation,
    ) -> Result<Value, ProtoError> {
        let expected = baseline_param(params)?;
        if let Some(project_id) = params.get("project_id").and_then(Value::as_str) {
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return mutate_relation_project(&mut unit.project, params, operation, expected);
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(project_failure("IO_ERROR", error, None, None, None)),
        };
        mutate_relation_project(&mut project, params, operation, expected)
    }
}
fn mutate_relation_project(
    project: &mut Project,
    params: &Value,
    operation: RelationOperation,
    expected: Option<&str>,
) -> Result<Value, ProtoError> {
    let (before, baseline, workspace_diagnostics) =
        match prepare_relation_project(project, expected) {
            Ok(value) => value,
            Err(failure) => return Ok(failure),
        };
    let id = relation_id(params, operation)?;
    let existing = before.analysis.catalog.relations.get(&id).cloned();
    if operation == RelationOperation::Create && existing.is_some() {
        return Ok(project_failure_with_workspace(
            "RELATION_EXISTS",
            format!("关系 `{id}` 已存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if matches!(
        operation,
        RelationOperation::Update | RelationOperation::Delete
    ) && existing.is_none()
    {
        return Ok(project_failure_with_workspace(
            "RELATION_NOT_FOUND",
            format!("关系 `{id}` 不存在"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    let snapshot = project.clone();
    if operation == RelationOperation::Delete {
        if let Err(error) = project.remove_relation(&id) {
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
        let draft = relation_draft(params, existing.as_ref())?;
        let original = (operation == RelationOperation::Update).then_some(id.as_str());
        if let Err(error) = project.write_relation(original, &draft) {
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
    let relation = if operation == RelationOperation::Delete {
        Value::Null
    } else {
        serde_json::to_value(after.analysis.catalog.relations.get(&id))
            .expect("SemanticRelationInfo 可序列化")
    };
    Ok(relation_success(
        relation,
        operation_name(operation),
        &after,
        project.content_baseline(),
        &workspace_diagnostics,
    ))
}

use super::*;
use worldline_core::source_edit::SourceEditRequest;
impl Server {
    pub(super) fn source_edit(&mut self, params: &Value, apply: bool) -> Result<Value, ProtoError> {
        self.source_or_schema_edit(params, apply, false)
    }
    pub(super) fn schema_edit(&mut self, params: &Value, apply: bool) -> Result<Value, ProtoError> {
        self.source_or_schema_edit(params, apply, true)
    }
    fn source_or_schema_edit(
        &mut self,
        params: &Value,
        apply: bool,
        schema: bool,
    ) -> Result<Value, ProtoError> {
        let request: SourceEditRequest = serde_json::from_value(
            params
                .get("request")
                .cloned()
                .ok_or_else(|| ProtoError::new(-32602, "需要request DTO"))?,
        )
        .map_err(|e| ProtoError::new(-32602, format!("无效源码草稿DTO：{e}")))?;
        let digest = if apply {
            Some(param_str(params, "plan_digest")?)
        } else {
            None
        };
        if params.get("project_id").is_some() == params.get("path").is_some() {
            return Err(ProtoError::new(-32602, "必须且只能提供project_id或path"));
        }
        if params.get("project_id").is_some() {
            let id = param_str(params, "project_id")?;
            let unit = self
                .projects
                .get_mut(id)
                .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
            return Ok(execute(&mut unit.project, &request, digest, schema));
        }
        let mut project = match Project::open(Path::new(param_str(params, "path")?)) {
            Ok(p) => p,
            Err(message) => {
                return Ok(json!({"ok":false,"error":{"code":"IO_ERROR","message":message}}))
            }
        };
        Ok(execute(&mut project, &request, digest, schema))
    }
}
fn execute(
    project: &mut Project,
    request: &SourceEditRequest,
    digest: Option<&str>,
    schema: bool,
) -> Value {
    let snapshot = project.clone();
    let result = if schema {
        if let Some(digest) = digest {
            project
                .apply_schema_edit(request, digest)
                .and_then(|preview| {
                    project.save()?;
                    Ok(json!(preview))
                })
        } else {
            project
                .preview_schema_edit(request)
                .map(|preview| json!(preview))
        }
    } else if let Some(digest) = digest {
        project
            .apply_source_edit(request, digest)
            .and_then(|preview| {
                project.save()?;
                Ok(json!(preview))
            })
    } else {
        project
            .preview_source_edit(request)
            .map(|preview| json!(preview))
    };
    match result {
        Ok(preview) => {
            json!({"ok":true,"operation":if digest.is_some(){"apply"}else{"preview"},"preview":preview,"baseline":project.content_baseline()})
        }
        Err(message) => {
            *project = snapshot;
            json!({"ok":false,"error":{"code":if schema {"SCHEMA_EDIT_REJECTED"} else {"SOURCE_EDIT_REJECTED"},"message":message},"baseline":project.content_baseline()})
        }
    }
}

impl Server {
    pub(super) fn schema_index(&mut self, params: &Value) -> Result<Value, ProtoError> {
        if params.get("project_id").is_some() == params.get("path").is_some() {
            return Err(ProtoError::new(-32602, "必须且只能提供project_id或path"));
        }
        if params.get("project_id").is_some() {
            let id = param_str(params, "project_id")?;
            let unit = self
                .projects
                .get(id)
                .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
            return Ok(index_response(&unit.project));
        }
        match Project::open(Path::new(param_str(params, "path")?)) {
            Ok(project) => Ok(index_response(&project)),
            Err(message) => Ok(json!({"ok":false,"error":{"code":"IO_ERROR","message":message}})),
        }
    }
}
fn index_response(project: &Project) -> Value {
    json!({"ok":true,"index":project.schema_index(),"baseline":project.content_baseline(),"workspace_diagnostics":project.authoring_diagnostics()})
}

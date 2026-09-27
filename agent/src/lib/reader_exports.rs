use super::*;
impl Server {
    pub(super) fn reader_export(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let selection: ReaderExportSelection = serde_json::from_value(
            params
                .get("selection")
                .cloned()
                .ok_or_else(|| ProtoError::new(-32602, "阅读包操作需要 `selection` DTO"))?,
        )
        .map_err(|error| ProtoError::new(-32602, format!("无效阅读包选择 DTO：{error}")))?;
        let plan_digest = if apply {
            Some(param_str(params, "plan_digest")?)
        } else {
            None
        };
        let destination = if apply {
            Some(PathBuf::from(param_str(params, "output")?))
        } else {
            None
        };
        let has_project_id = params.get("project_id").is_some();
        let has_path = params.get("path").is_some();
        if has_project_id == has_path {
            return Err(ProtoError::new(
                -32602,
                "阅读包操作必须且只能提供 `project_id` 或 `path`",
            ));
        }
        if has_project_id {
            let project_id = param_str(params, "project_id")?;
            let unit = self.projects.get(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(reader_export_project(
                &unit.project,
                &selection,
                plan_digest,
                destination.as_deref(),
            ));
        }
        let path = param_str(params, "path")?;
        let project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(reader_export_failure(
                    "IO_ERROR",
                    error,
                    None,
                    None,
                    &[],
                    if apply { "apply" } else { "preview" },
                ))
            }
        };
        Ok(reader_export_project(
            &project,
            &selection,
            plan_digest,
            destination.as_deref(),
        ))
    }
}
fn reader_export_failure(
    code: &str,
    message: String,
    baseline: Option<String>,
    plan: Option<Value>,
    workspace_diagnostics: &[Diagnostic],
    operation: &str,
) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "error": {"code": code, "message": message},
        "baseline": baseline,
        "plan": plan,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
    })
}

fn reader_export_project(
    project: &Project,
    selection: &ReaderExportSelection,
    plan_digest: Option<&str>,
    destination: Option<&Path>,
) -> Value {
    let operation = if plan_digest.is_some() {
        "apply"
    } else {
        "preview"
    };
    let baseline = project.content_baseline();
    let workspace_diagnostics = project.authoring_diagnostics();
    let plan = match project.preview_reader_export(selection) {
        Ok(plan) => plan,
        Err(error) => {
            return reader_export_failure(
                "PREVIEW_FAILED",
                error,
                Some(baseline),
                None,
                workspace_diagnostics,
                operation,
            )
        }
    };
    let Some(plan_digest) = plan_digest else {
        return json!({
            "ok": true,
            "operation": "preview",
            "plan": plan,
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
        });
    };
    if plan.plan_digest != plan_digest {
        return reader_export_failure(
            "STALE_PLAN",
            "阅读包预览已过期，请重新预览并核对选择".into(),
            Some(baseline),
            Some(json!(plan)),
            workspace_diagnostics,
            operation,
        );
    }
    let Some(destination) = destination else {
        unreachable!("apply params validate output before dispatch")
    };
    match project.export_reader_site(selection, plan_digest, destination) {
        Ok(()) => json!({
            "ok": true,
            "operation": "apply",
            "plan": plan,
            "baseline": baseline,
            "output": destination,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
        }),
        Err(error) => {
            let code = if error.contains("预览已过期") {
                "STALE_PLAN"
            } else {
                "EXPORT_FAILED"
            };
            reader_export_failure(
                code,
                error,
                Some(baseline),
                Some(json!(plan)),
                workspace_diagnostics,
                operation,
            )
        }
    }
}

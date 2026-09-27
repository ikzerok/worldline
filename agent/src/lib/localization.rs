use super::*;
impl Server {
    pub(super) fn localization_export(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let selection: LocalizationSelection = serde_json::from_value(
            params
                .get("selection")
                .cloned()
                .ok_or_else(|| ProtoError::new(-32602, "本地化操作需要 `selection` DTO"))?,
        )
        .map_err(|error| ProtoError::new(-32602, format!("无效本地化选择 DTO：{error}")))?;
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
                "本地化导出必须且只能提供 `project_id` 或 `path`",
            ));
        }
        if has_project_id {
            let project_id = param_str(params, "project_id")?;
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(localization_export_project(
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
                return Ok(localization_failure(
                    "IO_ERROR",
                    error,
                    None,
                    None,
                    &[],
                    if apply { "apply" } else { "preview" },
                ))
            }
        };
        Ok(localization_export_project(
            &project,
            &selection,
            plan_digest,
            destination.as_deref(),
        ))
    }

    pub(super) fn localization_import(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let selection: LocalizationSelection = serde_json::from_value(
            params
                .get("selection")
                .cloned()
                .ok_or_else(|| ProtoError::new(-32602, "本地化操作需要 `selection` DTO"))?,
        )
        .map_err(|error| ProtoError::new(-32602, format!("无效本地化选择 DTO：{error}")))?;
        let plan_digest = if apply {
            Some(param_str(params, "plan_digest")?)
        } else {
            None
        };
        let exchange_value = params
            .get("exchange")
            .cloned()
            .ok_or_else(|| ProtoError::new(-32602, "本地化导入需要 `exchange` DTO"))?;
        let exchange: LocalizationExchange = match serde_json::from_value(exchange_value) {
            Ok(exchange) => exchange,
            Err(error) => {
                return Ok(localization_failure(
                    "INVALID_PACKAGE",
                    format!("本地化交换包 DTO 无效：{error}"),
                    None,
                    None,
                    &[],
                    if apply { "apply" } else { "preview" },
                ))
            }
        };
        let has_project_id = params.get("project_id").is_some();
        let has_path = params.get("path").is_some();
        if has_project_id == has_path {
            return Err(ProtoError::new(
                -32602,
                "本地化导入必须且只能提供 `project_id` 或 `path`",
            ));
        }
        if has_project_id {
            let project_id = param_str(params, "project_id")?;
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(localization_import_project(
                &mut unit.project,
                &selection,
                &exchange,
                plan_digest,
            ));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(localization_failure(
                    "IO_ERROR",
                    error,
                    None,
                    None,
                    &[],
                    if apply { "apply" } else { "preview" },
                ))
            }
        };
        Ok(localization_import_project(
            &mut project,
            &selection,
            &exchange,
            plan_digest,
        ))
    }
}
fn localization_failure(
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

fn localization_export_project(
    project: &Project,
    selection: &LocalizationSelection,
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
    let plan = match project.preview_localization_export(selection) {
        Ok(plan) => plan,
        Err(error) => {
            return localization_failure(
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
        return localization_failure(
            "STALE_PLAN",
            "本地化导出预览已过期，请重新预览".into(),
            Some(baseline),
            Some(json!(plan)),
            workspace_diagnostics,
            operation,
        );
    }
    if !plan.can_export {
        let message = plan
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "本地化导出未通过校验".into());
        return localization_failure(
            "EXPORT_REJECTED",
            message,
            Some(baseline),
            Some(json!(plan)),
            workspace_diagnostics,
            operation,
        );
    }
    let Some(destination) = destination else {
        unreachable!("apply params validate output before dispatch")
    };
    match project.export_localization(selection, plan_digest, destination) {
        Ok(applied) => json!({
            "ok": true,
            "operation": "apply",
            "plan": applied,
            "baseline": baseline,
            "output": destination,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
        }),
        Err(error) => {
            let code = if error.contains("过期") {
                "STALE_PLAN"
            } else {
                "EXPORT_FAILED"
            };
            localization_failure(
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

fn localization_import_project(
    project: &mut Project,
    selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    plan_digest: Option<&str>,
) -> Value {
    let operation = if plan_digest.is_some() {
        "apply"
    } else {
        "preview"
    };
    let baseline = project.content_baseline();
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    let plan = match project.preview_localization_import(selection, exchange) {
        Ok(plan) => plan,
        Err(error) => {
            return localization_failure(
                "PREVIEW_FAILED",
                error,
                Some(baseline),
                None,
                &workspace_diagnostics,
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
        return localization_failure(
            "STALE_PLAN",
            "本地化导入预览已过期，请重新预览".into(),
            Some(baseline),
            Some(json!(plan)),
            &workspace_diagnostics,
            operation,
        );
    }
    if !plan.can_apply {
        let message = plan
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "本地化导入未通过校验".into());
        return localization_failure(
            "IMPORT_REJECTED",
            message,
            Some(baseline),
            Some(json!(plan)),
            &workspace_diagnostics,
            operation,
        );
    }
    match project.apply_localization_import(selection, exchange, plan_digest) {
        Ok(result) => {
            let workspace_diagnostics = project.authoring_diagnostics().to_vec();
            json!({
                "ok": true,
                "operation": "apply",
                "plan": result.plan,
                "changed_files": result.changed_files,
                "baseline": result.baseline,
                "new_baseline": result.new_baseline,
                "workspace_diagnostics": workspace_diagnostics,
                "read_only": !workspace_diagnostics.is_empty(),
            })
        }
        Err(error) => {
            let code = if error.contains("过期") || error.contains("基线") {
                "STALE_PLAN"
            } else {
                "IMPORT_REJECTED"
            };
            localization_failure(
                code,
                error,
                Some(baseline),
                Some(json!(plan)),
                &workspace_diagnostics,
                operation,
            )
        }
    }
}

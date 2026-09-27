use super::*;
impl Server {
    pub(super) fn markdown_import(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let has_project_id = params.get("project_id").is_some();
        let has_path = params.get("path").is_some();
        if has_project_id == has_path {
            return Err(ProtoError::new(
                -32602,
                "Markdown 导入必须且只能提供 `project_id` 或 `path`",
            ));
        }
        let source = PathBuf::from(param_str(params, "source")?);
        let baseline = param_str(params, "baseline")?.to_string();
        let id_overrides = match params.get("id_overrides") {
            None => std::collections::BTreeMap::new(),
            Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
                ProtoError::new(
                    -32602,
                    format!("`id_overrides` 必须是来源相对路径到 ID 的对象：{error}"),
                )
            })?,
        };
        let namespace = match params.get("namespace") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value.clone()),
            Some(_) => return Err(ProtoError::new(-32602, "`namespace` 必须是字符串")),
        };
        let (plan_digest, accept_losses, allow_language_upgrade) = if apply {
            (
                Some(param_str(params, "plan_digest")?.to_string()),
                param_bool(params, "accept_losses")?,
                param_bool(params, "allow_language_upgrade")?,
            )
        } else {
            if params.get("plan_digest").is_some()
                || params.get("accept_losses").is_some()
                || params.get("allow_language_upgrade").is_some()
            {
                return Err(ProtoError::new(
                    -32602,
                    "preview 不接受 `plan_digest`、`accept_losses` 或 `allow_language_upgrade`",
                ));
            }
            (None, false, false)
        };
        let request = worldline_core::markdown_import::MarkdownImportRequest {
            source_root: source,
            expected_baseline: baseline,
            id_overrides,
            namespace,
            accept_losses,
            allow_language_upgrade,
        };
        if has_project_id {
            let project_id = param_str(params, "project_id")?;
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(markdown_import_project(
                &mut unit.project,
                &request,
                apply,
                plan_digest.as_deref(),
            ));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(markdown_import_failure(
                    "IO_ERROR",
                    error,
                    None,
                    &[],
                    if apply { "apply" } else { "preview" },
                ))
            }
        };
        Ok(markdown_import_project(
            &mut project,
            &request,
            apply,
            plan_digest.as_deref(),
        ))
    }
}
fn markdown_import_error_code(message: &str) -> &'static str {
    if message.contains("预览已过期") {
        "STALE_PLAN"
    } else if message.contains("基线已过期") || message.contains("基线已变化") {
        "STALE_BASELINE"
    } else if message.contains("缺少损失确认") || message.contains("缺少语言升级确认")
    {
        "CONFIRMATION_REQUIRED"
    } else if message.contains("只读") || message.contains("必需能力") {
        "READ_ONLY"
    } else if message.contains("候选源码产生新编译错误") {
        "COMPILE_FAILED"
    } else if message.contains("冲突")
        || message.contains("已占用")
        || message.contains("已存在")
        || message.contains("外部修改")
    {
        "CONFLICT"
    } else {
        "MARKDOWN_IMPORT_REJECTED"
    }
}

fn markdown_import_failure(
    code: &str,
    message: impl Into<String>,
    baseline: Option<String>,
    workspace_diagnostics: &[Diagnostic],
    operation: &str,
) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "error": {"code": code, "message": message.into()},
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
    })
}

fn markdown_import_project(
    project: &mut Project,
    request: &worldline_core::markdown_import::MarkdownImportRequest,
    apply: bool,
    plan_digest: Option<&str>,
) -> Value {
    let operation = if apply { "apply" } else { "preview" };
    let baseline = project.content_baseline();
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    if apply {
        let digest = plan_digest.expect("apply requests require a plan digest");
        match project.apply_markdown_import(request, digest) {
            Ok(result) => json!({
                "ok": true,
                "operation": operation,
                "plan": result.plan,
                "changed_files": result.changed_files,
                "baseline": result.baseline,
                "new_baseline": result.new_baseline,
                "workspace_diagnostics": workspace_diagnostics,
                "read_only": false,
            }),
            Err(message) => markdown_import_failure(
                markdown_import_error_code(&message),
                message,
                Some(baseline),
                &workspace_diagnostics,
                operation,
            ),
        }
    } else {
        match project.preview_markdown_import(request) {
            Ok(plan) => json!({
                "ok": true,
                "operation": operation,
                "baseline": plan.baseline,
                "plan": plan,
                "workspace_diagnostics": workspace_diagnostics,
                "read_only": false,
            }),
            Err(message) => markdown_import_failure(
                markdown_import_error_code(&message),
                message,
                Some(baseline),
                &workspace_diagnostics,
                operation,
            ),
        }
    }
}

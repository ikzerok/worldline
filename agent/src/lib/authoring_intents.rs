use super::*;
impl Server {
    pub(super) fn authoring_intent(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let intent_value = params
            .get("intent")
            .ok_or_else(|| ProtoError::new(-32602, "组合意图需要 `intent` DTO"))?;
        let intent: AuthoringIntent = serde_json::from_value(intent_value.clone())
            .map_err(|error| ProtoError::new(-32602, format!("无效组合意图 DTO：{error}")))?;
        let has_project_id = params.get("project_id").is_some();
        let has_path = params.get("path").is_some();
        if has_project_id == has_path {
            return Err(ProtoError::new(
                -32602,
                "组合意图必须且只能提供 `project_id` 或 `path`",
            ));
        }
        if has_project_id {
            let project_id = param_str(params, "project_id")?;
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(authoring_intent_project(&mut unit.project, &intent, apply));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(authoring_intent_failure(
                    "IO_ERROR",
                    error,
                    None,
                    None,
                    &[],
                    None,
                ))
            }
        };
        Ok(authoring_intent_project(&mut project, &intent, apply))
    }
}
fn authoring_intent_failure(
    code: &str,
    message: String,
    result: Option<&CompileResult>,
    baseline: Option<String>,
    workspace_diagnostics: &[Diagnostic],
    conflicts: Option<&[PathBuf]>,
) -> Value {
    let mut response = json!({
        "ok": false,
        "error": {"code": code, "message": message},
        "language_version": result.map(|result| result.options.language_version.as_str()),
        "baseline": baseline,
        "diagnostics": result.map_or_else(Vec::new, |result| result.diagnostics.clone()),
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
    });
    if let Some(conflicts) = conflicts {
        response["conflicts"] = json!(conflicts);
    }
    response
}

fn authoring_intent_project(project: &mut Project, intent: &AuthoringIntent, apply: bool) -> Value {
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            let result = project.compile();
            return authoring_intent_failure(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result),
                Some(project.content_baseline()),
                project.authoring_diagnostics(),
                None,
            );
        }
    };
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    let before = project.compile();
    let baseline = project.content_baseline();
    if !conflicts.is_empty() {
        return authoring_intent_failure(
            "CONFLICT",
            format!(
                "工程存在外部修改冲突：{}",
                conflicts
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join("、")
            ),
            Some(&before),
            Some(baseline),
            &workspace_diagnostics,
            Some(&conflicts),
        );
    }
    if intent.expected_baseline != baseline {
        return authoring_intent_failure(
            "STALE_BASELINE",
            format!("工程基线已变化，请重新预览；当前基线为 {baseline}"),
            Some(&before),
            Some(baseline),
            &workspace_diagnostics,
            None,
        );
    }
    if before.has_errors() {
        return authoring_intent_failure(
            "COMPILE_FAILED",
            "当前工程存在错误诊断，组合意图未应用".into(),
            Some(&before),
            Some(baseline),
            &workspace_diagnostics,
            None,
        );
    }
    if !workspace_diagnostics.is_empty() {
        return authoring_intent_failure(
            "READ_ONLY",
            "工程清单或展示文档包含当前工具不支持的能力，只能只读查看".into(),
            Some(&before),
            Some(baseline),
            &workspace_diagnostics,
            None,
        );
    }
    let snapshot = project.clone();
    let applied = if apply {
        project.apply_authoring_intent(intent)
    } else {
        project.preview_authoring_intent(intent)
    };
    let intent_result = match applied {
        Ok(result) => result,
        Err(message) => {
            let code = authoring_intent_error_code(&message);
            return authoring_intent_failure(
                code,
                message,
                Some(&before),
                Some(baseline),
                &workspace_diagnostics,
                None,
            );
        }
    };
    if apply {
        if let Err(error) = project.save() {
            *project = snapshot;
            return authoring_intent_failure(
                "CONFLICT",
                error,
                Some(&before),
                Some(baseline),
                &workspace_diagnostics,
                None,
            );
        }
    }
    let result = if apply { project.compile() } else { before };
    let current_baseline = project.content_baseline();
    json!({
        "ok": true,
        "operation": if apply {"apply"} else {"preview"},
        "target": intent_result.target,
        "reference_impact": intent_result.reference_impact,
        "changed_files": intent_result.changed_files,
        "baseline": current_baseline,
        "new_baseline": intent_result.new_baseline,
        "language_version": result.options.language_version.as_str(),
        "diagnostics": result.diagnostics,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    })
}
fn authoring_intent_error_code(message: &str) -> &'static str {
    if message.contains("基线已过期") {
        "STALE_BASELINE"
    } else if message.contains("外部修改") || message.contains("保存事务冲突") {
        "CONFLICT"
    } else if message.contains("只读") || message.contains("必需能力") {
        "READ_ONLY"
    } else if message.contains("1.10") {
        "LANGUAGE_VERSION_REQUIRED"
    } else {
        "INTENT_REJECTED"
    }
}

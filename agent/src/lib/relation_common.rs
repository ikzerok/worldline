use super::projects::project_failure_with_workspace;
use super::*;
pub(super) fn baseline_param(params: &Value) -> Result<Option<&str>, ProtoError> {
    params
        .get("baseline")
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| ProtoError::new(-32602, "`baseline` 必须是字符串"))
        })
        .transpose()
}

/// 关系写入共享工作区边界:刷新外部文件、检查内容基线、故事诊断、工作区诊断
/// 和语言版本。所有关系写入入口都在这里之后才调用 core 的 Project 编辑 API。
pub(super) fn prepare_relation_project(
    project: &mut Project,
    expected: Option<&str>,
) -> Result<(CompileResult, String, Vec<Diagnostic>), Value> {
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            let result = project.compile();
            return Err(project_failure_with_workspace(
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
        return Err(project_failure_with_workspace(
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
        return Err(project_failure_with_workspace(
            "STALE_BASELINE",
            format!("工程基线已变化，拒绝覆盖；当前基线为 {baseline}"),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.has_errors() {
        return Err(project_failure_with_workspace(
            "COMPILE_FAILED",
            "当前工程存在错误诊断，关系编辑未提交".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if !workspace_diagnostics.is_empty() {
        return Err(project_failure_with_workspace(
            "READ_ONLY",
            "工程清单或展示文档包含当前工具不支持的格式，只能只读查看".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    if before.options.language_version != LanguageVersion::V1_10
        || project.language_version_kind() != LanguageVersion::V1_10
    {
        return Err(project_failure_with_workspace(
            "LANGUAGE_VERSION_REQUIRED",
            "关系编辑要求工程清单明确选择语言版本 1.10".into(),
            Some(&before.diagnostics),
            Some(baseline),
            Some(before.options.language_version.as_str()),
            &workspace_diagnostics,
        ));
    }
    Ok((before, baseline, workspace_diagnostics))
}
pub(super) fn operation_name<T>(operation: T) -> &'static str
where
    T: IntoOperationName,
{
    operation.into_operation_name()
}

pub(super) trait IntoOperationName {
    fn into_operation_name(self) -> &'static str;
}

impl IntoOperationName for RelationOperation {
    fn into_operation_name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

impl IntoOperationName for RelationTypeOperation {
    fn into_operation_name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

pub(super) fn relation_success(
    relation: Value,
    operation: &str,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": true,
        "operation": operation,
        "relation": relation,
        "catalog": &result.analysis.catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    })
}

pub(super) fn relation_type_success(
    relation_type: Value,
    operation: &str,
    result: &CompileResult,
    baseline: String,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    json!({
        "ok": true,
        "operation": operation,
        "relation_type": relation_type,
        "catalog": &result.analysis.catalog,
        "diagnostics": result.diagnostics,
        "language_version": result.options.language_version.as_str(),
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    })
}

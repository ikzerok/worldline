//! 模板机器接口的确定性预览摘要；不另建模板语义或保存通道。
use super::{
    ProjectTemplateDraft, ProjectTemplateDraftEdit, ProjectTemplateDraftProjection,
    ProjectTemplateDraftSource, ProjectTemplateFieldChange, ProjectTemplateInstanceImpact,
    ProjectTemplateMutation, ProjectTemplatePreview, ProjectTemplateSummary, TemplateCommand,
    TemplateImpactLimits,
};
use crate::presentation_commands::Revision;
use crate::project::Project;
use crate::{Diagnostic, Severity};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
mod budget;

pub const TEMPLATE_AUTHORING_CAPABILITY: &str = "authoring.template_designer.v1";
pub const MAX_TEMPLATE_REQUEST_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TEMPLATE_PLAN_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TEMPLATE_IMPACT_INSTANCES: usize = 10_000;
pub const MAX_TEMPLATE_IMPACT_FIELD_VALUES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TemplateDraftAction {
    Open {
        source: ProjectTemplateDraftSource,
    },
    Inspect {
        draft: ProjectTemplateDraft,
    },
    Edit {
        draft: ProjectTemplateDraft,
        edit: ProjectTemplateDraftEdit,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateDraftRequest {
    pub schema_version: u64,
    #[serde(default)]
    pub expected_baseline: Option<String>,
    pub action: TemplateDraftAction,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateDraftResult {
    pub schema_version: u64,
    pub baseline: String,
    pub projection: ProjectTemplateDraftProjection,
    pub applied: bool,
    pub saved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TemplateMutationIntent {
    Upsert { draft: ProjectTemplateDraft },
    RepairInvalid { draft: ProjectTemplateDraft },
    Delete { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateMutationRequest {
    pub schema_version: u64,
    #[serde(deserialize_with = "strict_revision")]
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub intent: TemplateMutationIntent,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateMutationPlan {
    pub schema_version: u64,
    pub operation: String,
    pub template_id: String,
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub plan_digest: String,
    pub field_changes: Vec<ProjectTemplateFieldChange>,
    pub instances: Vec<ProjectTemplateInstanceImpact>,
    pub diagnostics: Vec<Diagnostic>,
    pub changed_files: Vec<PathBuf>,
    pub complete: bool,
    pub incomplete_reason: Option<String>,
    pub current_template: Option<ProjectTemplateSummary>,
    pub proposed_template: Option<ProjectTemplateSummary>,
    pub can_apply: bool,
    pub replaces_invalid_source: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateMutationApplied {
    pub plan: TemplateMutationPlan,
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
    pub baseline: String,
    pub applied: bool,
    pub saved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateProtocolError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for TemplateProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.code, self.message)
    }
}
impl std::error::Error for TemplateProtocolError {}

fn error(code: &'static str, message: impl Into<String>) -> TemplateProtocolError {
    TemplateProtocolError {
        code,
        message: message.into(),
    }
}

pub fn parse_template_mutation_request(text: &str) -> Result<TemplateMutationRequest, String> {
    if text.len() > MAX_TEMPLATE_REQUEST_BYTES {
        return Err("模板请求超过 4 MiB 预算".into());
    }
    let value = crate::parse_unique_json(text.as_bytes())?;
    serde_json::from_value(value).map_err(|e| format!("模板请求格式无效：{e}"))
}

pub fn parse_template_draft_request(text: &str) -> Result<TemplateDraftRequest, String> {
    if text.len() > MAX_TEMPLATE_REQUEST_BYTES {
        return Err("模板请求超过 4 MiB 预算".into());
    }
    let value = crate::parse_unique_json(text.as_bytes())?;
    serde_json::from_value(value).map_err(|e| format!("模板草稿请求格式无效：{e}"))
}

impl Project {
    /// 机器入口显式建立只读内容投影；原模板草稿API仍可复用调用方已有的编译快照。
    pub fn template_draft_request(
        &self,
        request: &TemplateDraftRequest,
    ) -> Result<TemplateDraftResult, TemplateProtocolError> {
        if request.schema_version != 1 {
            return Err(error(
                "UNSUPPORTED_SCHEMA",
                "仅支持模板请求 schema_version 1",
            ));
        }
        budget::encoded_size(request, MAX_TEMPLATE_REQUEST_BYTES)
            .map_err(|_| error("REQUEST_LIMIT", "模板请求超过 4 MiB 预算"))?;
        let baseline = self.content_baseline();
        if request
            .expected_baseline
            .as_ref()
            .is_some_and(|expected| expected != &baseline)
        {
            return Err(error("STALE_BASELINE", "模板草稿的工程基线已过期"));
        }
        let content = self.compile_object_search_snapshot();
        let projection = match &request.action {
            TemplateDraftAction::Open { source } => self.template_draft(source.clone(), &content),
            TemplateDraftAction::Inspect { draft } => {
                Ok(self.project_template_draft_projection(draft, &content))
            }
            TemplateDraftAction::Edit { draft, edit } => {
                self.edit_template_draft(draft, edit, &content)
            }
        }
        .map_err(|e| error("DRAFT_REJECTED", e))?;
        let result = TemplateDraftResult {
            schema_version: 1,
            baseline,
            projection,
            applied: false,
            saved: false,
        };
        budget::encoded_size(&result, MAX_TEMPLATE_PLAN_BYTES)
            .map_err(|_| error("PLAN_LIMIT", "模板草稿结果超过 8 MiB 机器接口预算"))?;
        Ok(result)
    }

    pub fn preview_template_request(
        &self,
        revision: Revision,
        request: &TemplateMutationRequest,
    ) -> Result<TemplateMutationPlan, TemplateProtocolError> {
        self.prepare_template_request(revision, request)
            .map(|(_, plan)| plan)
    }

    pub fn apply_template_request(
        &mut self,
        revision: &mut Revision,
        request: &TemplateMutationRequest,
        expected_plan_digest: &str,
    ) -> Result<TemplateMutationApplied, TemplateProtocolError> {
        let (preview, plan) = self.prepare_template_request(*revision, request)?;
        if expected_plan_digest != plan.plan_digest {
            return Err(error("STALE_PLAN", "模板预览摘要不匹配，请重新预览"));
        }
        if !plan.can_apply {
            return Err(error("INVALID_TEMPLATE", "模板影响预览含错误，不能应用"));
        }
        let result = self
            .apply_template_mutation(revision, preview)
            .map_err(|e| error("APPLY_REJECTED", e))?;
        Ok(TemplateMutationApplied {
            plan,
            changed_files: result.changed_files,
            new_revision: result.new_revision,
            baseline: self.content_baseline(),
            applied: true,
            saved: false,
        })
    }

    fn prepare_template_request(
        &self,
        revision: Revision,
        request: &TemplateMutationRequest,
    ) -> Result<(ProjectTemplatePreview, TemplateMutationPlan), TemplateProtocolError> {
        if request.schema_version != 1 {
            return Err(error(
                "UNSUPPORTED_SCHEMA",
                "仅支持模板请求 schema_version 1",
            ));
        }
        let request_size = budget::encoded_size(request, MAX_TEMPLATE_REQUEST_BYTES)
            .map_err(|_| error("REQUEST_LIMIT", "模板请求超过 4 MiB 预算"))?;
        if revision != request.expected_revision
            || self.content_baseline() != request.expected_baseline
        {
            return Err(error("STALE_BASELINE", "模板请求的工程基线或修订已过期"));
        }
        let mutation = match &request.intent {
            TemplateMutationIntent::Upsert { draft } => self.template_mutation_from_draft(draft),
            TemplateMutationIntent::RepairInvalid { draft } => {
                self.template_repair_mutation_from_draft(draft)
            }
            TemplateMutationIntent::Delete { id } => {
                Ok(ProjectTemplateMutation::Delete { id: id.clone() })
            }
        }
        .map_err(|e| error("INVALID_REQUEST", e))?;
        let (operation, template_id) = match &mutation {
            ProjectTemplateMutation::Import { id, .. } => ("import", id.clone()),
            ProjectTemplateMutation::Replace { id, .. } => ("replace", id.clone()),
            ProjectTemplateMutation::RepairInvalid { id, .. } => ("repair_invalid", id.clone()),
            ProjectTemplateMutation::Delete { id } => ("delete", id.clone()),
        };
        let command = TemplateCommand {
            expected_revision: request.expected_revision,
            expected_baseline: request.expected_baseline.clone(),
            mutation,
            check_integrity: true,
        };
        let limits = TemplateImpactLimits {
            max_instances: MAX_TEMPLATE_IMPACT_INSTANCES,
            max_field_values: MAX_TEMPLATE_IMPACT_FIELD_VALUES,
            max_output_bytes: MAX_TEMPLATE_PLAN_BYTES,
        };
        let preview = self
            .preview_template_mutation_with_limits(revision, &command, &limits)
            .map_err(|e| {
                error(
                    if e.starts_with("TemplateImpactLimit") {
                        "PLAN_LIMIT"
                    } else {
                        "PREVIEW_REJECTED"
                    },
                    e,
                )
            })?;
        let mut plan = TemplateMutationPlan {
            schema_version: 1,
            operation: operation.into(),
            template_id,
            expected_revision: preview.expected_revision,
            expected_baseline: preview.expected_baseline.clone(),
            plan_digest: String::new(),
            field_changes: preview.field_changes.clone(),
            instances: preview.instances.clone(),
            diagnostics: preview.diagnostics.clone(),
            changed_files: preview.changed_files.clone(),
            complete: preview.complete,
            incomplete_reason: preview.incomplete_reason.clone(),
            current_template: preview.current_template.clone(),
            proposed_template: preview.proposed_template.clone(),
            can_apply: preview.complete
                && !preview
                    .diagnostics
                    .iter()
                    .any(|d| d.severity == Severity::Error),
            replaces_invalid_source: operation == "repair_invalid",
        };
        // 摘要占16个ASCII字符；预留后再填，不能用空摘要时的大小冒充最终上限。
        let plan_size = budget::encoded_size(&plan, MAX_TEMPLATE_PLAN_BYTES - 16)
            .map_err(|_| error("PLAN_LIMIT", "模板影响结果超过 8 MiB 机器接口预算"))?;
        // 长度分隔的校验摘要用于过期/篡改检测，不是授权令牌或安全签名。
        plan.plan_digest = budget::digest(request, request_size, &plan, plan_size)
            .map_err(|e| error("INVALID_PLAN", e))?;
        budget::encoded_size(&plan, MAX_TEMPLATE_PLAN_BYTES)
            .map_err(|_| error("PLAN_LIMIT", "最终模板计划超过 8 MiB 机器接口预算"))?;
        Ok((preview, plan))
    }
}

fn strict_revision<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Revision, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        workspace_generation: u64,
        content_generation: u64,
        presentation_generation: u64,
    }
    let revision = Wire::deserialize(deserializer)?;
    Ok(Revision {
        workspace_generation: revision.workspace_generation,
        content_generation: revision.content_generation,
        presentation_generation: revision.presentation_generation,
    })
}

#[cfg(test)]
mod tests;

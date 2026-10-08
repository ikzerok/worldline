//! 工程级作者模板索引、影响预览与基线保护写入。
//!
//! 模板描述只用于编辑器表单；本模块从不把字段默认值或类型迁移写入源码实例。

use crate::ast::PropertyValue;
use crate::catalog::TargetRef;
use crate::presentation_commands::Revision;
use crate::project::{AuthoringDocument, Project};
use crate::workspace_documents::{
    manifest_path, parse_registry, parse_unique_json, registered_path,
};
use crate::{CompileResult, Diagnostic, Span};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
mod document;
mod drafts;
pub use drafts::*;
pub mod protocol;
mod references;

type PreparedTemplateMutation = (
    Project,
    Option<ProjectTemplate>,
    Option<ProjectTemplate>,
    Vec<PathBuf>,
    Vec<Diagnostic>,
);

pub const PROJECT_TEMPLATE_REQUIRED_FEATURE: &str = "content.templates.v1";
pub const OBJECT_REFS_REQUIRED_FEATURE: &str = "content.object_refs.v1";
pub const CHARACTER_REFS_REQUIRED_FEATURE: &str = "content.character_refs.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectTemplateField {
    pub id: String,
    pub key: Option<String>,
    pub label: String,
    pub field_type: String,
    pub required: bool,
    pub choices: Vec<String>,
    pub target: Option<TargetRef>,
    pub target_entity_type: Option<String>,
    pub default: Option<Value>,
    pub fields: Vec<ProjectTemplateField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectTemplate {
    pub id: String,
    pub title: String,
    pub applies_to: TargetRef,
    pub applies_to_entity_type: Option<String>,
    pub fields: Vec<ProjectTemplateField>,
}

#[derive(Debug, Clone)]
pub struct ProjectTemplateDocument {
    /// 注册文档的来源路径，供缓存索引直接投影引用导航。
    pub file: String,
    pub template: Option<ProjectTemplate>,
    pub source_document: Option<Value>,
    pub source_bytes: Vec<u8>,
    pub diagnostics: Vec<Diagnostic>,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectTemplateIndex {
    pub builtins: Vec<crate::content_templates::ContentTemplate>,
    pub projects: BTreeMap<String, ProjectTemplateDocument>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectTemplateMutation {
    Import {
        id: String,
        document: Vec<u8>,
    },
    Replace {
        id: String,
        document: Vec<u8>,
    },
    /// 显式以完整新文修复仅 TPL001 的坏原文；普通 Replace 不提供此救援。
    RepairInvalid {
        id: String,
        document: Vec<u8>,
    },
    Delete {
        id: String,
    },
}

#[derive(Debug, Clone)]
pub struct TemplateCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub mutation: ProjectTemplateMutation,
    pub check_integrity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectTemplateValueState {
    Missing,
    Empty,
    Default,
    Set,
    TypeMismatch,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplateFieldImpact {
    pub field_id: String,
    pub template_state: String,
    pub key: String,
    pub state: ProjectTemplateValueState,
    pub value: Option<PropertyValue>,
    pub type_matches: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplateInstanceImpact {
    pub target: TargetRef,
    pub current_applicable: bool,
    pub proposed_applicable: bool,
    pub fields: Vec<ProjectTemplateFieldImpact>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectTemplateFieldChange {
    pub field_id: String,
    pub change: String,
    pub old_key: Option<String>,
    pub new_key: Option<String>,
    pub old_type: Option<String>,
    pub new_type: Option<String>,
    pub old_parent_id: Option<String>,
    pub new_parent_id: Option<String>,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub old_properties: Option<ProjectTemplateFieldProperties>,
    pub new_properties: Option<ProjectTemplateFieldProperties>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectTemplateSummary {
    pub id: String,
    pub title: String,
    pub applies_to: TargetRef,
    pub applies_to_entity_type: Option<String>,
}

impl From<&ProjectTemplate> for ProjectTemplateSummary {
    fn from(template: &ProjectTemplate) -> Self {
        Self {
            id: template.id.clone(),
            title: template.title.clone(),
            applies_to: template.applies_to.clone(),
            applies_to_entity_type: template.applies_to_entity_type.clone(),
        }
    }
}

/// 显式受限调用的影响预算；旧预览入口不隐式应用这些限制。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateImpactLimits {
    pub max_instances: usize,
    pub max_field_values: usize,
    pub max_output_bytes: usize,
}

#[derive(Clone)]
pub struct ProjectTemplatePreview {
    pub mutation: ProjectTemplateMutation,
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub complete: bool,
    pub incomplete_reason: Option<String>,
    pub current_template: Option<ProjectTemplateSummary>,
    pub proposed_template: Option<ProjectTemplateSummary>,
    pub field_changes: Vec<ProjectTemplateFieldChange>,
    pub instances: Vec<ProjectTemplateInstanceImpact>,
    pub diagnostics: Vec<Diagnostic>,
    pub changed_files: Vec<PathBuf>,
    candidate: Project,
    // 公开摘要不能被调用方篡改为可应用；仅核心生产者设置此凭据。
    verified_complete: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectTemplateResult {
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
}

impl std::fmt::Debug for ProjectTemplatePreview {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProjectTemplatePreview")
            .field("mutation", &self.mutation)
            .field("expected_revision", &self.expected_revision)
            .field("expected_baseline", &self.expected_baseline)
            .field("complete", &self.complete)
            .field("incomplete_reason", &self.incomplete_reason)
            .field("current_template", &self.current_template)
            .field("proposed_template", &self.proposed_template)
            .field("field_changes", &self.field_changes)
            .field("instances", &self.instances)
            .field("diagnostics", &self.diagnostics)
            .field("changed_files", &self.changed_files)
            .finish_non_exhaustive()
    }
}

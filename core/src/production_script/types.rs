use crate::localization::LocalizationPart;
use crate::manuscript::ManuscriptQueryRequest;
use crate::TargetRef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProductionScope {
    CurrentTarget {
        #[serde(deserialize_with = "super::input::target")]
        target: TargetRef,
    },
    Manuscript {
        query: Box<ManuscriptQueryRequest>,
        #[serde(default)]
        chapter_ids: Option<Vec<String>>,
        #[serde(default)]
        expected_query_key: Option<String>,
    },
    Project,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionLocalePolicy {
    #[default]
    Strict,
    SourceFallback,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionStatus {
    Source,
    Translated,
    Missing,
    Stale,
    Invalid,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionKind {
    Say,
    Text,
    Choice,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProductionLimits {
    pub chapters: usize,
    pub definitions: usize,
    pub call_sites: usize,
    pub rows: usize,
    pub source_files: usize,
    pub source_bytes: usize,
    pub result_bytes: usize,
    pub export_bytes: usize,
}
impl Default for ProductionLimits {
    fn default() -> Self {
        Self {
            chapters: 4096,
            definitions: 4096,
            call_sites: 20_000,
            rows: 50_000,
            source_files: 4096,
            source_bytes: 16 * 1024 * 1024,
            result_bytes: 16 * 1024 * 1024,
            export_bytes: 32 * 1024 * 1024,
        }
    }
}
fn yes() -> bool {
    true
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionScriptRequest {
    pub schema_version: u32,
    pub scope: ProductionScope,
    #[serde(default = "yes")]
    pub include_fragments: bool,
    #[serde(default, deserialize_with = "super::input::optional_target")]
    pub speaker: Option<TargetRef>,
    #[serde(default)]
    pub include_narration: bool,
    #[serde(default)]
    pub include_choices: bool,
    #[serde(default)]
    pub target_locale: Option<String>,
    #[serde(default)]
    pub locale_policy: ProductionLocalePolicy,
    #[serde(default)]
    pub statuses: Vec<ProductionStatus>,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub limits: ProductionLimits,
    #[serde(default)]
    pub expected_snapshot_key: Option<String>,
}
impl ProductionScriptRequest {
    pub fn new(scope: ProductionScope) -> Self {
        Self {
            schema_version: 1,
            scope,
            include_fragments: true,
            speaker: None,
            include_narration: false,
            include_choices: false,
            target_locale: None,
            locale_policy: ProductionLocalePolicy::Strict,
            statuses: Vec::new(),
            search: String::new(),
            limits: ProductionLimits::default(),
            expected_snapshot_key: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionSource {
    pub file: String,
    pub line: u32,
    pub column: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionSpeaker {
    pub target: TargetRef,
    pub display: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionControl {
    pub kind: String,
    pub source: ProductionSource,
    pub condition: Option<String>,
    pub enable: Option<String>,
    pub once: bool,
    pub target: Option<TargetRef>,
    pub evaluated: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionCallUse {
    pub caller: TargetRef,
    pub callee: TargetRef,
    pub source: ProductionSource,
    pub control_ancestry: Vec<ProductionControl>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionDefinition {
    pub target: TargetRef,
    pub source: ProductionSource,
    pub is_root: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionChapterOccurrence {
    pub manuscript_id: String,
    pub chapter_id: String,
    pub target: TargetRef,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionScopeSummary {
    pub selected_chapter_occurrences: usize,
    pub root_targets: usize,
    pub definition_count: usize,
    pub added_fragment_definitions: usize,
    pub call_sites: usize,
    pub source_files: usize,
    pub matching_rows: usize,
    pub source_only: bool,
    pub includes_fragment_closure: bool,
    pub source_fallback_rows: usize,
    pub complete: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionRow {
    pub row_key: String,
    pub kind: ProductionKind,
    pub declaration: TargetRef,
    pub speaker: Option<ProductionSpeaker>,
    pub source: ProductionSource,
    pub stable_line_id: Option<String>,
    pub source_revision: String,
    pub source_parts: Vec<LocalizationPart>,
    pub selected_parts: Vec<LocalizationPart>,
    pub status: ProductionStatus,
    pub target_locale: Option<String>,
    pub used_source_fallback: bool,
    pub control_ancestry: Vec<ProductionControl>,
    pub external_call_uses: Vec<ProductionCallUse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionScriptPage {
    pub schema_version: u32,
    pub snapshot_key: String,
    pub summary: ProductionScopeSummary,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub rows: Vec<ProductionRow>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionFormat {
    Json,
    Markdown,
    Csv,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionExportOptions {
    pub schema_version: u32,
    pub format: ProductionFormat,
    #[serde(default)]
    pub include_direction: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct ProductionDocument {
    pub schema_version: u32,
    pub scope_kind: String,
    pub speaker: Option<TargetRef>,
    pub target_locale: Option<String>,
    pub snapshot_key: String,
    pub summary: ProductionScopeSummary,
    pub locale_policy: ProductionLocalePolicy,
    pub metadata_language: String,
    pub direction_included: bool,
    pub definitions: Vec<ProductionDefinition>,
    pub chapter_occurrences: Vec<ProductionChapterOccurrence>,
    pub call_sites: Vec<ProductionCallUse>,
    pub rows: Vec<ProductionRow>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductionError {
    pub code: String,
    pub message: String,
}
impl ProductionError {
    pub(super) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub(super) fn budget() -> Self {
        Self::new("BUDGET_EXCEEDED", "制作台本超过完整结果预算；未截断交付")
    }
    pub(super) fn source() -> Self {
        Self::new("INVALID_SOURCE", "无法确认当前正式语句来源；未生成替代位置")
    }
}
impl std::fmt::Display for ProductionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.code, self.message)
    }
}
impl std::error::Error for ProductionError {}

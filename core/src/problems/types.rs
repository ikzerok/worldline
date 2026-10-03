use crate::{Severity, Span};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use crate::diagnostic::DiagnosticSourceRole as ProblemSourceRole;

pub const PROBLEM_SOURCE_CONTEXT_CAPABILITY: &str = "authoring.problem_source_context.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemDomain {
    Content,
    Workspace,
    Maps,
    GraphViews,
    Presets,
    Comments,
    Proposals,
    Templates,
    SavedQueries,
    Manuscripts,
    ReaderProfiles,
    Localizations,
}
impl ProblemDomain {
    pub const ALL: [Self; 12] = [
        Self::Content,
        Self::Workspace,
        Self::Maps,
        Self::GraphViews,
        Self::Presets,
        Self::Comments,
        Self::Proposals,
        Self::Templates,
        Self::SavedQueries,
        Self::Manuscripts,
        Self::ReaderProfiles,
        Self::Localizations,
    ];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemCoverageState {
    Checked,
    Partial,
    Unavailable,
    NotApplicable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemCoverage {
    pub domain: ProblemDomain,
    pub path: Option<String>,
    pub state: ProblemCoverageState,
    pub reasons: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemPrecision {
    Span,
    Document,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemRange {
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemContextVisibility {
    Full,
    Partial,
    NoText,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemSourceContext {
    pub version: u32,
    pub role: ProblemSourceRole,
    pub text: Option<String>,
    pub slice_byte_range: Option<ProblemRange>,
    pub slice_char_range: Option<ProblemRange>,
    pub hit_byte_range: Option<ProblemRange>,
    pub hit_char_range: Option<ProblemRange>,
    pub visibility: ProblemContextVisibility,
    pub prefix_clipped: bool,
    pub suffix_clipped: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemLocation {
    pub path: Option<String>,
    pub precision: ProblemPrecision,
    pub span: Option<Span>,
    pub byte_range: Option<ProblemRange>,
    pub char_range: Option<ProblemRange>,
    pub excerpt: Option<String>,
    pub excerpt_truncated: bool,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ProblemSourceContext>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemEntry {
    pub id: String,
    pub domain: ProblemDomain,
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub note: Option<String>,
    pub suggestion: Option<String>,
    pub primary: ProblemLocation,
    pub related_count: usize,
    pub text_truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProblemsOptions {
    pub max_entries: usize,
    pub max_related_locations: usize,
    pub max_report_bytes: usize,
    pub max_text_bytes: usize,
    pub max_excerpt_bytes: usize,
}
impl Default for ProblemsOptions {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_related_locations: 50_000,
            max_report_bytes: 32 * 1024 * 1024,
            max_text_bytes: 16_384,
            max_excerpt_bytes: 512,
        }
    }
}
impl ProblemsOptions {
    pub(crate) fn validate(&self) -> Result<(), ProblemsError> {
        let hard = Self::default();
        if self.max_entries > hard.max_entries
            || self.max_related_locations > hard.max_related_locations
            || self.max_report_bytes > hard.max_report_bytes
            || self.max_text_bytes > hard.max_text_bytes
            || self.max_excerpt_bytes > hard.max_excerpt_bytes
            || self.max_report_bytes == 0
            || self.max_text_bytes == 0
        {
            return Err(ProblemsError::new(
                "BUDGET_EXCEEDED",
                "问题报告预算无效或超过硬上限",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemsReport {
    pub schema_version: u32,
    pub report_version: String,
    pub content_baseline: String,
    pub source_observation: String,
    pub language_version: String,
    pub content_has_errors: bool,
    pub read_only: bool,
    pub complete: bool,
    pub truncated: bool,
    pub reasons: Vec<String>,
    pub coverage: Vec<ProblemCoverage>,
    pub entries: Vec<ProblemEntry>,
    pub related: BTreeMap<String, Vec<ProblemLocation>>,
    pub limits: ProblemsOptions,
    pub compile_count: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProblemQuery {
    pub severities: Vec<Severity>,
    pub domains: Vec<ProblemDomain>,
    pub path: Option<String>,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProblemCursor {
    pub report_version: String,
    pub query_key: String,
    pub offset: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemPage {
    pub report_version: String,
    pub content_baseline: String,
    pub total: usize,
    pub matched: usize,
    pub entries: Vec<ProblemEntry>,
    pub next_cursor: Option<ProblemCursor>,
    pub complete: bool,
    pub truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemRelatedPage {
    pub report_version: String,
    pub problem_id: String,
    pub total: usize,
    pub locations: Vec<ProblemLocation>,
    pub next_cursor: Option<ProblemCursor>,
    pub truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProblemsError {
    pub code: String,
    pub message: String,
}
impl ProblemsError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for ProblemsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ProblemsError {}

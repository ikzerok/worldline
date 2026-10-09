use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum LocalizationStatus {
    MissingId,
    DuplicateId,
    InvalidTranslation,
    StaleSource,
    MissingTranslation,
    Translated,
    OrphanTranslation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationCatalogQuery {
    pub schema_version: u32,
    pub target_locale: Option<String>,
    pub source_prefix: Option<String>,
    pub kind: Option<String>,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub statuses: Vec<LocalizationStatus>,
    #[serde(default)]
    pub string_ids: Vec<String>,
    #[serde(default)]
    pub offset: usize,
    pub limit: usize,
    pub expected_content_baseline: Option<String>,
    pub expected_source_baseline: Option<String>,
}

impl Default for LocalizationCatalogQuery {
    fn default() -> Self {
        Self {
            schema_version: 1,
            target_locale: None,
            source_prefix: None,
            kind: None,
            search: String::new(),
            statuses: Vec::new(),
            string_ids: Vec::new(),
            offset: 0,
            limit: 50,
            expected_content_baseline: None,
            expected_source_baseline: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationCatalogEntry {
    pub unit_key: String,
    pub id: Option<String>,
    pub source: Option<LocalizationSource>,
    pub source_revision: Option<String>,
    pub source_parts: Vec<LocalizationPart>,
    pub translation_parts: Option<Vec<LocalizationPart>>,
    pub status: LocalizationStatus,
    pub sidecar_path: Option<String>,
    pub translation_pointer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationCatalogPage {
    pub schema_version: u32,
    pub content_baseline: String,
    pub source_baseline: String,
    pub source_locale: Option<String>,
    pub target_locale: Option<String>,
    pub available_locales: Vec<String>,
    pub all_total: usize,
    pub total: usize,
    pub status_counts: BTreeMap<LocalizationStatus, usize>,
    pub entries: Vec<LocalizationCatalogEntry>,
    pub next_offset: Option<usize>,
    pub read_only: bool,
    pub diagnostics: Vec<LocalizationDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationEdit {
    pub id: String,
    pub source_revision: String,
    pub translation_parts: Vec<LocalizationPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationEditDraft {
    pub schema_version: u32,
    pub source_locale: String,
    pub target_locale: String,
    pub source_baseline: String,
    pub edits: Vec<LocalizationEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationIdAssignment {
    pub source: LocalizationSource,
    pub source_revision: String,
    pub expected_id: Option<String>,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationIdDraft {
    pub schema_version: u32,
    pub source_baseline: String,
    pub assignments: Vec<LocalizationIdAssignment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationSourceChange {
    pub file: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationIdPlan {
    pub schema_version: u32,
    pub plan_digest: String,
    pub content_baseline: String,
    pub source_baseline: String,
    pub changes: Vec<LocalizationSourceChange>,
    pub diagnostics: Vec<LocalizationDiagnostic>,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationIdResult {
    pub plan: LocalizationIdPlan,
    pub changed_files: Vec<PathBuf>,
    pub baseline: String,
    pub new_baseline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationImportDraft {
    pub selection: LocalizationSelection,
    pub exchange: LocalizationExchange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationError {
    pub code: String,
    pub message: String,
}

impl LocalizationError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl From<String> for LocalizationError {
    fn from(value: String) -> Self {
        let codes = [
            "BUDGET_EXCEEDED",
            "INVALID_ID",
            "INVALID_JSON",
            "INVALID_QUERY",
            "STALE_QUERY",
            "STALE_PLAN",
            "STALE_SOURCE",
            "EXTERNAL_CONFLICT",
            "COMPILE_ERROR",
            "UNSUPPORTED_VERSION",
            "FEATURE_REQUIRED",
            "READ_ONLY",
            "UNKNOWN_LOCALE",
            "SIDECAR_INVALID",
            "INVALID_PRESENTATION",
            "VALIDATION_FAILED",
            "INVALID_SOURCE",
            "NO_CHANGE",
            "UNKNOWN_ID",
            "DUPLICATE_ID",
            "INVALID_TOKEN",
            "MISSING_TRANSLATION",
            "MISSING_ENTRY",
            "BASELINE_MISMATCH",
            "SOURCE_MISMATCH",
            "SELECTION_MISMATCH",
        ];
        if let Some((code, message)) = value.split_once('：') {
            if codes.contains(&code) {
                return Self::new(code, message);
            }
        }
        Self::new("VALIDATION_FAILED", value)
    }
}

impl std::fmt::Display for LocalizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for LocalizationError {}

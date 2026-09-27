//! Explicitly selected localization exchange; see `spec/localization.md`.

mod export;
mod import;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const LOCALIZATION_REQUIRED_FEATURE: &str = "content.localization.v1";
pub const LOCALIZATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationSelection {
    pub schema_version: u32,
    pub source_locale: String,
    pub target_locale: String,
    pub string_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalizationPart {
    Text { text: String },
    Placeholder { token: String },
    Link { token: String, label: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationSource {
    pub file: String,
    pub line: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationExchangeEntry {
    pub id: String,
    pub source_revision: String,
    pub source: LocalizationSource,
    pub source_parts: Vec<LocalizationPart>,
    pub translation_parts: Option<Vec<LocalizationPart>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationExchange {
    pub schema_version: u32,
    pub source_locale: String,
    pub target_locale: String,
    pub source_baseline: String,
    pub string_ids: Vec<String>,
    pub entries: Vec<LocalizationExchangeEntry>,
}

impl LocalizationExchange {
    /// Parse an exchange file while rejecting duplicate JSON keys.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, String> {
        let value = crate::workspace_documents::parse_unique_json(bytes)
            .map_err(|error| format!("本地化交换包 JSON 无效：{error}"))?;
        let exchange: Self = serde_json::from_value(value)
            .map_err(|error| format!("本地化交换包字段无效：{error}"))?;
        Ok(exchange)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationDiagnostic {
    pub code: String,
    pub id: Option<String>,
    pub source: Option<LocalizationSource>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationExportPlan {
    pub schema_version: u32,
    pub plan_digest: String,
    pub content_baseline: String,
    pub exchange: LocalizationExchange,
    pub diagnostics: Vec<LocalizationDiagnostic>,
    pub can_export: bool,
}

#[derive(Clone)]
struct SourceUnit {
    source: LocalizationSource,
    parts: Vec<LocalizationPart>,
    source_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationImportPlan {
    pub schema_version: u32,
    pub plan_digest: String,
    pub content_baseline: String,
    pub source_baseline: String,
    pub target_locale: String,
    pub sidecar_path: String,
    pub affected_ids: Vec<String>,
    pub diagnostics: Vec<LocalizationDiagnostic>,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalizationImportResult {
    pub plan: LocalizationImportPlan,
    pub changed_files: Vec<PathBuf>,
    pub baseline: String,
    pub new_baseline: String,
}

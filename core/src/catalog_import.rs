//! 世界资料CSV批量导入；唯一解析、字段预览和一次内存事务。
mod csv;
mod mapping;
mod patch;
mod planning;
#[cfg(test)]
mod tests;

pub use csv::parse_catalog_csv;
use crate::{ast::PropertyValue, catalog::TargetRef};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_CSV_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ROWS: usize = 500;
pub const MAX_COLUMNS: usize = 64;
pub const MAX_CELL_BYTES: usize = 64 * 1024;
pub const MAX_DIAGNOSTICS: usize = 100;
pub const MAX_PREVIEW_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogImportRequest {
    pub schema_version: u32,
    pub expected_baseline: String,
    pub csv: String,
    pub destination: PathBuf,
    pub columns: Vec<CatalogColumnMapping>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogColumnMapping {
    pub column: usize,
    pub field: CatalogImportField,
    #[serde(default)]
    pub blank: CatalogBlankPolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogImportField {
    Kind,
    Id,
    Display,
    EntityType,
    Description,
    Property { key: String, value_type: CatalogImportType },
    Ignore,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CatalogImportType {
    Text,
    Number,
    Bool,
    Ref { target_kind: String },
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogBlankPolicy {
    #[default]
    Error,
    Keep,
    EmptyText,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCsvTable {
    pub headers: Vec<String>,
    pub rows: Vec<CatalogCsvRow>,
    pub normalization_count: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogCsvRow {
    pub cells: Vec<String>,
    pub line: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogImportDiagnostic {
    pub code: String,
    pub row: Option<usize>,
    pub column: Option<usize>,
    pub line: Option<u32>,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogImportFieldChange {
    pub field: String,
    pub value_type: String,
    pub before: Option<PropertyValue>,
    pub after: Option<PropertyValue>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogImportRow {
    pub row: usize,
    pub line: u32,
    pub target: Option<TargetRef>,
    pub operation: String,
    pub source: Option<PathBuf>,
    pub fields: Vec<CatalogImportFieldChange>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogImportPlan {
    pub schema_version: u32,
    pub baseline: String,
    pub input_digest: String,
    pub plan_digest: String,
    pub destination: PathBuf,
    pub ignored_columns: Vec<String>,
    pub normalization_count: usize,
    pub rows: Vec<CatalogImportRow>,
    pub diagnostics: Vec<CatalogImportDiagnostic>,
    pub error_count: usize,
    pub can_apply: bool,
    pub changed_files: Vec<PathBuf>,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogImportResult {
    pub plan: CatalogImportPlan,
    pub changed_files: Vec<PathBuf>,
    pub new_baseline: String,
}

impl CatalogImportDiagnostic {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), row: None, column: None, line: None, message: message.into() }
    }
    fn at(mut self, row: usize, column: usize, line: u32) -> Self {
        self.row = Some(row);
        self.column = Some(column);
        self.line = Some(line);
        self
    }
}
fn digest(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

/// 机器JSON入口拒绝重复键及未知字段，返回同一严格请求DTO。
pub fn parse_catalog_import_request(json: &str) -> Result<CatalogImportRequest, String> {
    let value = crate::workspace_documents::parse_unique_json(json.as_bytes())?;
    serde_json::from_value(value).map_err(|error| format!("资料导入请求格式错误：{error}"))
}

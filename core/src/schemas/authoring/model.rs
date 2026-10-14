use super::protocol::{deserialize_binding, deserialize_target, deserialize_value};
use crate::schemas::{SchemaBinding, SchemaDecl, SchemaIncompleteReason, SchemaType};
use crate::{
    ast::PropertyValue,
    catalog::{CatalogObject, TargetRef},
    Diagnostic,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_SCHEMA_AUTHORING_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_SCHEMA_AUTHORING_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SCHEMA_AUTHORING_FIELDS: usize = 256;
pub const MAX_SCHEMA_AUTHORING_CANDIDATES: usize = 10_000;
pub const MAX_SCHEMA_AUTHORING_SCHEMAS: usize = 256;
pub const MAX_SCHEMA_AUTHORING_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_SCHEMA_AUTHORING_ID_BYTES: usize = 512;
pub const MAX_SCHEMA_AUTHORING_SOURCE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_SCHEMA_AUTHORING_WORKSPACE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaAuthoringRequest {
    pub schema_version: u32,
    pub expected_baseline: String,
    #[serde(deserialize_with = "deserialize_target")]
    pub target: TargetRef,
    pub binding: SchemaBindingMutation,
    pub fields: Vec<SchemaFieldMutation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SchemaBindingMutation {
    Keep,
    Bind { schema_id: String },
    Change { schema_id: String },
    Unbind,
}

impl<'de> Deserialize<'de> for SchemaBindingMutation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_binding(deserializer)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum SchemaFieldMutation {
    Set {
        field_id: String,
        key: String,
        #[serde(deserialize_with = "deserialize_value")]
        value: PropertyValue,
    },
    Remove {
        field_id: String,
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaAuthoringSource {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaAuthoringField {
    pub id: String,
    pub key: String,
    pub value_type: SchemaType,
    pub required: bool,
    pub present: bool,
    pub value: Option<PropertyValue>,
    pub property_source: Option<SchemaAuthoringSource>,
    pub constraint_source: SchemaAuthoringSource,
    pub diagnostics: Vec<Diagnostic>,
    pub reference_candidates: Vec<CatalogObject>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaAuthoringProjection {
    pub schema_version: u32,
    pub baseline: String,
    pub target: TargetRef,
    pub object: CatalogObject,
    pub binding: Option<SchemaBinding>,
    pub schema: Option<SchemaDecl>,
    pub compatible_schemas: Vec<SchemaDecl>,
    pub fields: Vec<SchemaAuthoringField>,
    pub diagnostics: Vec<Diagnostic>,
    pub complete: bool,
    pub incomplete_reasons: Vec<SchemaIncompleteReason>,
    pub writable: bool,
    pub read_only_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaAuthoringPreview {
    pub schema_version: u32,
    pub expected_baseline: String,
    pub plan_digest: String,
    pub changed: bool,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
    pub runtime_changed: bool,
    pub before: SchemaAuthoringProjection,
    pub after: SchemaAuthoringProjection,
    pub changed_files: Vec<PathBuf>,
    pub diagnostics: Vec<Diagnostic>,
    pub has_errors: bool,
}

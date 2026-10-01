use crate::{ast::Loc, catalog::TargetRef, Diagnostic};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaType {
    Text,
    Number,
    Boolean,
    Enum {
        values: Vec<String>,
    },
    Ref {
        target_kind: String,
        entity_type: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaField {
    pub id: String,
    pub key: String,
    pub value_type: SchemaType,
    pub required: bool,
    pub loc: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaDecl {
    pub id: String,
    pub kind: String,
    pub entity_type: Option<String>,
    pub closed: bool,
    pub fields: Vec<SchemaField>,
    pub file: String,
    pub loc: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaBinding {
    pub target: TargetRef,
    pub schema_id: String,
    pub file: String,
    pub loc: Loc,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaInstance {
    pub target: TargetRef,
    pub schema_id: String,
    pub file: String,
    pub line: u32,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaIndex {
    pub schemas: Vec<SchemaDecl>,
    pub bindings: Vec<SchemaBinding>,
    pub instances: Vec<SchemaInstance>,
    pub diagnostics: Vec<Diagnostic>,
}

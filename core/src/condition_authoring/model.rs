use crate::{Diagnostic, TargetRef};
use serde::{Deserialize, Serialize};
use std::{ops::Range, path::PathBuf};

pub const CONDITION_AUTHORING_CAPABILITY: &str = "authoring.condition_authoring.v1";
pub const MAX_CONDITION_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_CONDITION_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_CONDITION_SOURCE_BYTES: usize = 16 * 1024;
pub const MAX_CONDITION_NODES: usize = 128;
pub const MAX_CONDITION_DEPTH: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionOwner {
    #[serde(deserialize_with = "super::protocol::target")]
    pub target: TargetRef,
    pub choice_line: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionSlot {
    ChoiceDisplay,
    ChoiceEnable,
    EventAfter,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionSelector {
    pub schema_version: u32,
    pub owner: ConditionOwner,
    pub slot: ConditionSlot,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionValueType {
    Num,
    Str,
    Bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ConditionLiteral {
    Num(f64),
    Str(String),
    Bool(bool),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionComparison {
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConditionNode {
    Bool {
        value: bool,
    },
    GlobalBool {
        variable: String,
    },
    Has {
        state_id: String,
        tag_id: String,
    },
    Compare {
        variable: String,
        op: ConditionComparison,
        value: ConditionLiteral,
        literal_left: bool,
    },
    Not {
        child: Box<ConditionNode>,
    },
    And {
        left: Box<ConditionNode>,
        right: Box<ConditionNode>,
    },
    Or {
        left: Box<ConditionNode>,
        right: Box<ConditionNode>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionSupport {
    Empty,
    Supported,
    Advanced,
    Invalid,
    BudgetExceeded,
    VersionUnsupported,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConditionSource {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub byte_range: Range<usize>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ConditionProjection {
    pub schema_version: u32,
    pub selector: ConditionSelector,
    pub baseline: String,
    pub generation: u64,
    pub input_generation: u64,
    pub source: String,
    pub location: Option<ConditionSource>,
    pub support: ConditionSupport,
    pub tree: Option<ConditionNode>,
    pub reasons: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub writable: bool,
    pub read_only_reasons: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionDraftInput {
    pub schema_version: u32,
    pub owner: ConditionOwner,
    pub slot: ConditionSlot,
    pub generation: u64,
    pub source: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct ConditionDraftPreview {
    pub schema_version: u32,
    pub plan_digest: String,
    pub changed: bool,
    pub before: ConditionProjection,
    pub after_source: String,
    pub replacement: Option<ConditionNode>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionAuthoringRequest {
    pub schema_version: u32,
    pub expected_baseline: String,
    pub expected_generation: u64,
    pub selector: ConditionSelector,
    pub replacement: ConditionNode,
}
#[derive(Debug, Clone, Serialize)]
pub struct ConditionSourcePatch {
    pub file: PathBuf,
    pub byte_range: Range<usize>,
    pub before: String,
    pub after: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct ConditionAuthoringPreview {
    pub schema_version: u32,
    pub expected_baseline: String,
    pub plan_digest: String,
    pub changed: bool,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
    pub runtime_changed: bool,
    pub before: ConditionProjection,
    pub after: ConditionProjection,
    pub patch: Option<ConditionSourcePatch>,
    pub changed_files: Vec<PathBuf>,
    pub diagnostics: Vec<Diagnostic>,
    pub has_errors: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionCandidateKind {
    Variable,
    State,
    Tag,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionCandidateQuery {
    pub schema_version: u32,
    pub kind: ConditionCandidateKind,
    pub text: String,
    pub offset: usize,
    pub limit: usize,
}
#[derive(Debug, Clone, Serialize)]
pub struct ConditionCandidate {
    pub target: TargetRef,
    pub display: String,
    pub value_type: Option<ConditionValueType>,
    pub owner: Option<TargetRef>,
    pub file: String,
    pub line: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct ConditionCandidates {
    pub schema_version: u32,
    pub baseline: String,
    pub generation: u64,
    pub total: usize,
    pub offset: usize,
    pub has_more: bool,
    pub items: Vec<ConditionCandidate>,
}

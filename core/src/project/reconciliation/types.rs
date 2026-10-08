use super::Project;
use crate::problems::ProblemsReport;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const RECONCILIATION_CAPABILITY: &str = "authoring.workspace_reconciliation.v1";
pub const MAX_FILES: usize = 4096;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReconciliationChoice {
    Baseline,
    Local,
    Disk,
    Manual { text: String },
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationDecision {
    pub path: PathBuf,
    pub choice: ReconciliationChoice,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationRequest {
    pub choices: Vec<ReconciliationDecision>,
    #[serde(default)]
    pub allow_incomplete_source: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconciliationFile {
    pub path: PathBuf,
    pub authoring: bool,
    pub baseline: Option<Vec<u8>>,
    pub local: Option<Vec<u8>>,
    pub disk: Option<Vec<u8>>,
    pub protected_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconciliationSession {
    pub schema_version: u32,
    pub workspace_root: PathBuf,
    pub entry: PathBuf,
    pub content_baseline: String,
    pub session_digest: String,
    pub files: Vec<ReconciliationFile>,
    pub blockers: Vec<String>,
    #[serde(skip)]
    pub(super) guard: Guard,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconciliationCandidate {
    pub path: PathBuf,
    pub authoring: bool,
    pub result: Option<Vec<u8>>,
    pub resolved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconciliationPlan {
    pub schema_version: u32,
    pub session_digest: String,
    pub plan_digest: String,
    pub expected_baseline: String,
    pub candidate_baseline: String,
    pub request: ReconciliationRequest,
    pub files: Vec<ReconciliationCandidate>,
    pub unresolved: usize,
    pub blockers: Vec<String>,
    pub problems: ProblemsReport,
    pub source_has_errors: bool,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: u64,
    pub can_apply: bool,
    #[serde(skip)]
    pub(super) session: ReconciliationSession,
}

/// 仅成功的内存采纳生成该结果。undo 不含旧磁盘基线，不自动执行或保存。
pub struct PreparedReconciliation {
    pub(super) plan: ReconciliationPlan,
    pub(super) candidate: Project,
}

pub struct ReconciliationApplied {
    pub plan: ReconciliationPlan,
    pub undo: Project,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconciliationStage {
    Capture,
    Candidate,
    Validate,
    Revalidate,
    BeforeCommit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct BufferEvidence {
    pub path: PathBuf,
    pub authoring: bool,
    pub retained: Vec<u8>,
    pub deleted: bool,
    pub saved: Option<Vec<u8>>,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Guard {
    pub root: PathBuf,
    pub entry: PathBuf,
    pub generation: u64,
    pub buffers: Vec<BufferEvidence>,
    pub disk: BTreeMap<PathBuf, Vec<u8>>,
    pub diagnostics: Vec<u8>,
    pub language_version: String,
    pub recovery_conflicts: Vec<PathBuf>,
}

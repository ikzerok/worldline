//! 同一当前稿的真实双路线对照；契约见 spec/route-comparison.md。
use crate::{AccessCoverage, ChoiceIdentity, ReplayBudget, StateActionEvidence, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use worldline_core::evidence_source::EvidenceSource;

mod checkpoint_limit;
mod limits;
mod projection;
mod session;
mod side;
pub(crate) use limits::{check_story, encoded_size, OutputUsage};
pub use session::{compare_routes, RouteComparisonSession};

pub const ROUTE_COMPARISON_SCHEMA_VERSION: u32 = 1;
pub const ROUTE_COMPARISON_CAPABILITY: &str = "authoring.route_comparison.v1";
pub const MAX_ROUTE_TRACE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ROUTE_TRACE_STEPS: usize = 4096;
pub const MAX_ROUTE_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_ROUTE_REPORT_RECORDS: usize = 16384;
pub const MAX_ROUTE_EVIDENCE_RECORDS: usize = 256;
pub const MAX_ROUTE_EVIDENCE_BYTES: usize = 64 * 1024;
pub const MAX_ROUTE_STEPS: u64 = 100_000;
pub const MAX_ROUTE_TIME_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct RouteComparisonOptions {
    pub budget: ReplayBudget,
    pub max_trace_bytes: usize,
    pub max_trace_steps: usize,
    pub max_output_bytes: usize,
    pub max_evidence_records: usize,
    pub max_evidence_bytes: usize,
}
impl Default for RouteComparisonOptions {
    fn default() -> Self {
        Self {
            budget: ReplayBudget::new(MAX_ROUTE_STEPS, MAX_ROUTE_TIME_MS),
            max_trace_bytes: MAX_ROUTE_TRACE_BYTES,
            max_trace_steps: MAX_ROUTE_TRACE_STEPS,
            max_output_bytes: MAX_ROUTE_OUTPUT_BYTES,
            max_evidence_records: MAX_ROUTE_EVIDENCE_RECORDS,
            max_evidence_bytes: MAX_ROUTE_EVIDENCE_BYTES,
        }
    }
}
impl RouteComparisonOptions {
    pub fn validate(&self) -> Result<(), RouteComparisonError> {
        let maximum = Self::default();
        if self.budget.max_steps > maximum.budget.max_steps
            || self.budget.time_budget_ms > maximum.budget.time_budget_ms
            || self.max_trace_bytes > maximum.max_trace_bytes
            || self.max_trace_steps > maximum.max_trace_steps
            || self.max_output_bytes > maximum.max_output_bytes
            || self.max_evidence_records > maximum.max_evidence_records
            || self.max_evidence_bytes > maximum.max_evidence_bytes
        {
            return Err(RouteComparisonError::new(
                "invalid_options",
                "比较额度不得超过预设上限",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteComparisonError {
    pub code: String,
    pub message: String,
}
impl RouteComparisonError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for RouteComparisonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RouteComparisonError {}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteStatus {
    Replayed,
    Diverged,
    StepBudgetExceeded,
    TimeBudgetExceeded,
    Cancelled,
    IncompleteTrace,
    StoryFailed,
    OutputBudgetExceeded,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteOriginSummary {
    pub kind: String,
    pub seed: u64,
    pub checkpoint_fingerprint: Option<u64>,
    pub checkpoint_digest: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RouteCoverage {
    pub inherited: AccessCoverage,
    pub executed: AccessCoverage,
    pub total: AccessCoverage,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteSideResult {
    pub origin: RouteOriginSummary,
    pub original_fingerprint: u64,
    pub status: RouteStatus,
    pub ended: bool,
    pub complete: bool,
    pub executed_steps: u64,
    pub completed_choices: usize,
    pub current_node: Option<String>,
    pub detail: Option<String>,
    pub divergence_step: Option<usize>,
    pub states: Option<BTreeMap<String, Vec<String>>>,
    pub vars: Option<BTreeMap<String, Value>>,
    pub coverage: RouteCoverage,
    pub state_actions: StateActionEvidence,
    pub omitted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteChoiceInput {
    pub choice: ChoiceIdentity,
    pub source: Option<EvidenceSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteChoiceDifference {
    pub index: usize,
    pub left: RouteChoiceInput,
    pub right: RouteChoiceInput,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteAlignment {
    pub comparable: bool,
    pub reason: Option<String>,
    pub common_prefix: usize,
    pub first_difference: Option<RouteChoiceDifference>,
    pub left_verified_choices: usize,
    pub right_verified_choices: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteValueDifference {
    pub id: String,
    pub left: Option<serde_json::Value>,
    pub right: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteComparisonResult {
    pub schema_version: u32,
    pub runtime_version: String,
    pub source_fingerprint: u64,
    pub source_snapshot: String,
    pub left: RouteSideResult,
    pub right: RouteSideResult,
    pub alignment: RouteAlignment,
    pub state_differences: Vec<RouteValueDifference>,
    pub variable_differences: Vec<RouteValueDifference>,
    pub differences_complete: bool,
    pub omitted: bool,
}

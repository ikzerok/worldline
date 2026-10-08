use crate::Value;
use serde::{Deserialize, Serialize};
use worldline_core::state_inspection_source::DeclarationSource;

pub const STATE_INSPECTION_CAPABILITY: &str = "runtime.state_inspection.v1";
pub const MAX_INSPECTION_ROWS: usize = 4096;
pub const MAX_INSPECTION_HISTORY_BYTES: usize = 1024 * 1024;
pub const MAX_INSPECTION_VALUE_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionGroup {
    Global,
    Local,
    State,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionBaseline {
    First,
    #[default]
    Previous,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InspectionKey {
    pub group: InspectionGroup,
    pub name: String,
    pub call_id: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectionStamp {
    #[serde(with = "super::stamp_wire")]
    pub run_id: u64,
    #[serde(with = "super::stamp_wire")]
    pub compiled_snapshot: u64,
    #[serde(with = "super::stamp_wire")]
    pub fingerprint: u64,
    #[serde(with = "super::stamp_wire")]
    pub trace_generation: u64,
    #[serde(with = "super::stamp_wire")]
    pub revision: u64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionStatus {
    #[default]
    Ready,
    Choice,
    Ended,
    Advancing,
    StepBudgetExceeded,
    TimeBudgetExceeded,
    Cancelled,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionCellStatus {
    Present,
    Uninitialized,
    NotInScope,
    Unrecorded,
    Omitted,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectionCell {
    pub status: InspectionCellStatus,
    pub value: Option<Value>,
    pub display: String,
    pub truncated: bool,
}
impl InspectionCell {
    pub(super) fn missing(status: InspectionCellStatus) -> Self {
        let display = match status {
            InspectionCellStatus::Present => "",
            InspectionCellStatus::Uninitialized => "未初始化",
            InspectionCellStatus::NotInScope => "该调用当时不存在",
            InspectionCellStatus::Unrecorded => "无已记录观测",
            InspectionCellStatus::Omitted => "观测值已省略",
        }
        .into();
        Self {
            status,
            value: None,
            display,
            truncated: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionChange {
    Unchanged,
    Changed,
    NotComparable,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateInspectionItem {
    pub key: InspectionKey,
    pub fragment: Option<String>,
    pub depth: Option<usize>,
    pub first: InspectionCell,
    pub previous: InspectionCell,
    pub current: InspectionCell,
    pub first_change: InspectionChange,
    pub previous_change: InspectionChange,
    pub source: Option<DeclarationSource>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StateInspectionQuery {
    pub text: String,
    pub group: Option<InspectionGroup>,
    pub changed_only: bool,
    pub compare_to: InspectionBaseline,
    pub offset: usize,
    pub limit: usize,
    pub expected_stamp: Option<InspectionStamp>,
}
impl Default for StateInspectionQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            group: None,
            changed_only: false,
            compare_to: InspectionBaseline::Previous,
            offset: 0,
            limit: 50,
            expected_stamp: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateInspectionPage {
    pub stamp: InspectionStamp,
    pub status: InspectionStatus,
    pub first_observation: Option<u64>,
    pub previous_observation: Option<u64>,
    pub current_observation: Option<u64>,
    pub items: Vec<StateInspectionItem>,
    pub total_items: usize,
    pub total_matches: usize,
    pub incomparable_items: usize,
    pub offset: usize,
    pub limit: usize,
    pub next_offset: Option<usize>,
    pub history_omitted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateInspectionError {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for StateInspectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for StateInspectionError {}

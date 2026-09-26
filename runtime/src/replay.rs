//! Versioned public DTOs for deterministic replay and runtime inspection.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub const REPLAY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceIdentity {
    pub id: String,
    pub node: String,
    pub line: u32,
    pub offset: usize,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConditionExplanation {
    pub expression: String,
    pub result: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChoiceExplanation {
    pub choice: ChoiceIdentity,
    pub available: bool,
    pub condition: Option<ConditionExplanation>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceCoverage {
    pub id: String,
    pub node: String,
    pub label: String,
    pub line: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AccessCoverage {
    pub visited_nodes: BTreeMap<String, u32>,
    pub selected_choices: Vec<ChoiceCoverage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayCheckpoint {
    pub schema_version: u32,
    pub runtime_version: String,
    pub fingerprint: u64,
    pub seed: u64,
    /// Existing runtime save JSON; the checkpoint wrapper binds it to this schema/version.
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayOrigin {
    Entry { seed: u64 },
    Checkpoint { checkpoint: ReplayCheckpoint },
}

impl ReplayOrigin {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Entry { .. } => "entry",
            Self::Checkpoint { .. } => "checkpoint",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayObservation {
    pub outputs: Vec<serde_json::Value>,
    pub choices: Vec<ChoiceIdentity>,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayStep {
    pub choice: ChoiceIdentity,
    /// `None` means the trace ended immediately after this input was chosen.
    pub observation: Option<ReplayObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayTrace {
    pub schema_version: u32,
    pub runtime_version: String,
    pub fingerprint: u64,
    pub origin: ReplayOrigin,
    pub initial_observation: Option<ReplayObservation>,
    pub steps: Vec<ReplayStep>,
    /// True only when the recorded Story actually reached its terminal state.
    pub complete: bool,
}

impl ReplayTrace {
    pub(crate) fn entry(fingerprint: u64, seed: u64) -> Self {
        Self {
            schema_version: REPLAY_SCHEMA_VERSION,
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            fingerprint,
            origin: ReplayOrigin::Entry { seed },
            initial_observation: None,
            steps: Vec::new(),
            complete: false,
        }
    }

    pub(crate) fn checkpoint(checkpoint: ReplayCheckpoint) -> Self {
        Self {
            schema_version: REPLAY_SCHEMA_VERSION,
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            fingerprint: checkpoint.fingerprint,
            origin: ReplayOrigin::Checkpoint { checkpoint },
            initial_observation: None,
            steps: Vec::new(),
            complete: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayBudget {
    pub max_steps: u64,
    pub time_budget_ms: u64,
}

impl ReplayBudget {
    pub const fn new(max_steps: u64, time_budget_ms: u64) -> Self {
        Self {
            max_steps,
            time_budget_ms,
        }
    }
}

impl Default for ReplayBudget {
    fn default() -> Self {
        Self::new(100_000, 30_000)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReplayCancellation(Arc<AtomicBool>);

impl ReplayCancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReplayStatus {
    Replayed {
        ended: bool,
        complete: bool,
    },
    Diverged {
        step_index: usize,
        reason: String,
        expected_choice: Option<ChoiceIdentity>,
        actual_choices: Vec<ChoiceIdentity>,
    },
    StepBudgetExceeded,
    TimeBudgetExceeded,
    Cancelled,
    IncompleteTrace,
    StoryFailed {
        message: String,
        node: Option<String>,
        line: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayResult {
    pub status: ReplayStatus,
    pub executed_steps: u64,
    pub completed_choices: usize,
    pub source_fingerprint: u64,
    pub original_fingerprint: u64,
    pub current_node: Option<String>,
    pub current_state: serde_json::Value,
    pub state_diff: BTreeMap<String, serde_json::Value>,
    pub coverage: AccessCoverage,
}

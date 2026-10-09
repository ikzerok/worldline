//! 独立作者试玩交接；只投影重新执行并匹配成功的观察。
use crate::{ReplayBudget, RouteOriginSummary, RouteStatus};
use serde::{Deserialize, Serialize};
use worldline_core::CompileOptions;

mod capture;
mod markdown;
mod observer;
mod session;
mod sources;
pub(crate) use capture::OutputSources;
pub(crate) use observer::ReportObserver;
pub use session::{generate_playthrough_report, PlaythroughReportSession};

pub const PLAYTHROUGH_REPORT_SCHEMA_VERSION: u32 = 1;
pub const PLAYTHROUGH_REPORT_CAPABILITY: &str = "authoring.playthrough_report.v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PlaythroughReportOptions {
    pub budget: ReplayBudget,
    pub max_trace_bytes: usize,
    pub max_trace_steps: usize,
    pub max_output_bytes: usize,
}
impl Default for PlaythroughReportOptions {
    fn default() -> Self {
        Self {
            budget: ReplayBudget::new(100_000, 30_000),
            max_trace_bytes: 4 * 1024 * 1024,
            max_trace_steps: 4096,
            max_output_bytes: 1024 * 1024,
        }
    }
}
impl PlaythroughReportOptions {
    pub fn validate(&self) -> Result<(), PlaythroughReportError> {
        let maximum = Self::default();
        if self.budget.max_steps > maximum.budget.max_steps
            || self.budget.time_budget_ms > maximum.budget.time_budget_ms
            || self.max_trace_bytes > maximum.max_trace_bytes
            || self.max_trace_steps > maximum.max_trace_steps
            || self.max_output_bytes > maximum.max_output_bytes
        {
            return Err(PlaythroughReportError::new(
                "invalid_options",
                "审阅记录额度不得超过预设上限",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaythroughReportError {
    pub code: String,
    pub message: String,
}
impl PlaythroughReportError {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for PlaythroughReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for PlaythroughReportError {}
impl From<crate::RouteComparisonError> for PlaythroughReportError {
    fn from(error: crate::RouteComparisonError) -> Self {
        Self::new(&error.code, "审阅记录或验证状态超过允许额度")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaythroughSource {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub precision: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaythroughSourceFile {
    pub file: String,
    pub bytes: usize,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaythroughChoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub localization_status: Option<worldline_core::localization::LocalizationStatus>,
    pub id: String,
    pub node: String,
    pub label: String,
    pub source: Option<PlaythroughSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaythroughText {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub localization_status: Option<worldline_core::localization::LocalizationStatus>,
    pub content: String,
    pub new_line: bool,
    pub speaker: Option<worldline_core::TargetRef>,
    pub speaker_label: Option<String>,
    pub source: Option<PlaythroughSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaythroughObservation {
    pub index: usize,
    pub choice: Option<PlaythroughChoice>,
    pub texts: Vec<PlaythroughText>,
    pub ended: bool,
    pub state_actions: u64,
    pub variable_writes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaythroughReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<crate::RuntimeLocalizationIdentity>,
    pub schema_version: u32,
    pub runtime_version: String,
    pub compile_options: CompileOptions,
    pub limits: PlaythroughReportOptions,
    pub generated_at_unix_ms: Option<u64>,
    pub source_fingerprint: u64,
    pub original_fingerprint: u64,
    pub source_snapshot: String,
    pub source_base: String,
    pub source_manifest: Vec<PlaythroughSourceFile>,
    pub origin: RouteOriginSummary,
    pub entry: String,
    pub inherited_visited_nodes: usize,
    pub inherited_selected_choices: u64,
    pub status: RouteStatus,
    pub ended: bool,
    pub complete: bool,
    pub executed_steps: u64,
    pub verified_choices: usize,
    pub divergence_step: Option<usize>,
    pub observations: Vec<PlaythroughObservation>,
    pub pending_choice: Option<PlaythroughChoice>,
    pub markdown: String,
}

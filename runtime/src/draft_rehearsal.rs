//! 当前正文的独立真实会话；不暴露正式保存/检查点/持久路径接口。
mod limits;
mod run;
pub(crate) use limits::DraftRehearsalLimits;
pub use limits::{
    MAX_DRAFT_REHEARSAL_OUTPUT_BYTES, MAX_DRAFT_REHEARSAL_REQUEST_BYTES,
    MAX_DRAFT_REHEARSAL_RESULT_BYTES,
};
#[cfg(test)]
mod tests;
pub use run::{run_draft_rehearsal, DraftRehearsalRunRequest, DraftRehearsalRunResult};

use crate::{
    BoundedContinuation, ChoiceExplanation, ChoicePresentation, InspectionStamp, OwnedStory,
    ReplayBudget, ReplayCancellation, RunError, StateInspectionError, StateInspectionPage,
    StateInspectionQuery, StateRecord,
};
use worldline_core::draft_rehearsal::DraftRehearsalSnapshot;

pub struct DraftRehearsal {
    snapshot: DraftRehearsalSnapshot,
    story: OwnedStory,
    seed: u64,
    limits: DraftRehearsalLimits,
}

impl DraftRehearsal {
    pub fn new(snapshot: DraftRehearsalSnapshot, seed: u64) -> Result<Self, RunError> {
        let compiled = snapshot.compiled();
        let story =
            OwnedStory::new_with_seed(compiled.program.clone(), compiled.analysis.clone(), seed)?;
        limits::view_guard(story.as_story())?;
        Ok(Self {
            snapshot,
            story,
            seed,
            limits: DraftRehearsalLimits::new(),
        })
    }
    pub fn snapshot(&self) -> &DraftRehearsalSnapshot {
        &self.snapshot
    }
    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn continue_bounded(
        &mut self,
        budget: ReplayBudget,
        cancellation: &ReplayCancellation,
    ) -> Result<BoundedContinuation, RunError> {
        self.story
            .continue_draft_bounded(budget, cancellation, &mut self.limits)
    }
    pub fn choose_id(&mut self, id: &str) -> Result<(), RunError> {
        if self.limits.blocked {
            return Err(RunError::new("试演已停止，请重新试演"));
        }
        self.story.choose_id(id)
    }
    pub fn choose_presentation(&mut self, index: usize) -> Result<(), RunError> {
        if self.limits.blocked {
            return Err(RunError::new("试演已停止，请重新试演"));
        }
        self.story.choose_presentation(index)
    }
    pub fn choice_presentations(&self) -> &[ChoicePresentation] {
        self.story.choice_presentations()
    }
    pub fn choice_evidence(&self) -> Option<&[ChoiceExplanation]> {
        self.story.choice_evidence()
    }
    pub fn view_guard(&self) -> Result<(), RunError> {
        limits::view_guard(self.story.as_story())
    }
    pub fn output_complete(&self) -> bool {
        self.limits.output_complete
    }
    pub fn last_executed_steps(&self) -> u64 {
        self.limits.last_steps
    }
    pub fn state_view(&self) -> Result<serde_json::Value, RunError> {
        self.view_guard()?;
        Ok(self.story.state_view())
    }
    pub fn state_history(&self) -> &[StateRecord] {
        self.story.state_history()
    }
    pub fn inspect_state(
        &self,
        query: &StateInspectionQuery,
    ) -> Result<StateInspectionPage, StateInspectionError> {
        self.story.inspect_state(query)
    }
    pub fn inspection_stamp(&self) -> InspectionStamp {
        self.story.inspection_stamp()
    }
    pub fn is_ended(&self) -> bool {
        self.story.is_ended()
    }
    pub fn is_paused(&self) -> bool {
        self.story.is_paused()
    }
    pub fn current_node(&self) -> Option<String> {
        self.story.current_node()
    }
    pub fn turns(&self) -> u32 {
        self.story.turns()
    }
}

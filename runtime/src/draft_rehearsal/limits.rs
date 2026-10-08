//! 复用真实解释器的逐语句输出/借用状态门，不先积累无界JSON副本。
use crate::{
    execution::{MonotonicInstant, ReplayExecutionBudget, ReplayStop},
    route_comparison::{check_story, encoded_size, OutputUsage},
    BoundedContinuation, ContinuationOutcome, InspectionStatus, ReplayBudget, ReplayCancellation,
    ReplayStatus, RunError, Story,
};

pub const MAX_DRAFT_REHEARSAL_REQUEST_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_DRAFT_REHEARSAL_RESULT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_DRAFT_REHEARSAL_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(crate) struct DraftRehearsalLimits {
    usage: OutputUsage,
    pub last_steps: u64,
    pub output_complete: bool,
    pub blocked: bool,
}

impl DraftRehearsalLimits {
    pub(super) fn new() -> Self {
        Self {
            output_complete: true,
            ..Self::default()
        }
    }
}

pub(super) fn view_guard(story: &Story<'_>) -> Result<(), RunError> {
    check_story(story, MAX_DRAFT_REHEARSAL_OUTPUT_BYTES)
        .map_err(|_| RunError::new("试演真实状态超过1MiB或16384记录预算"))?;
    encoded_size(
        story.choice_presentations(),
        MAX_DRAFT_REHEARSAL_OUTPUT_BYTES,
    )
    .map_err(|_| RunError::new("试演选择展示超过1MiB预算"))?;
    encoded_size(&story.choice_evidence(), MAX_DRAFT_REHEARSAL_OUTPUT_BYTES)
        .map_err(|_| RunError::new("试演条件证据超过1MiB预算"))?;
    Ok(())
}

impl Story<'_> {
    pub(crate) fn continue_draft_bounded(
        &mut self,
        limits: ReplayBudget,
        cancellation: &ReplayCancellation,
        guard: &mut DraftRehearsalLimits,
    ) -> Result<BoundedContinuation, RunError> {
        guard.last_steps = 0;
        if guard.blocked {
            return Err(RunError::new("试演已达到输出/状态预算，请重新试演"));
        }
        if let Err(error) = view_guard(self) {
            guard.blocked = true;
            guard.output_complete = false;
            return Err(error);
        }
        if !self.is_paused() && !self.is_ended() {
            self.inspection.advancing();
        }
        let started = MonotonicInstant::now();
        let mut budget = ReplayExecutionBudget {
            limits,
            cancellation,
            started,
            steps: 0,
            slice: None,
            slice_started: started,
            slice_steps: 0,
            comparison_limit: Some(MAX_DRAFT_REHEARSAL_OUTPUT_BYTES),
            output_usage: Some(&mut guard.usage),
        };
        let result = self.continue_story_inner(&mut budget);
        guard.last_steps = budget.steps;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                guard.blocked = true;
                guard.output_complete = false;
                self.inspection.status = InspectionStatus::Failed;
                return Err(error);
            }
        };
        let outcome = match result.stop {
            Some(ReplayStop::OutputBudgetExceeded | ReplayStop::ComparisonRejected(_)) => {
                guard.blocked = true;
                guard.output_complete = false;
                self.inspection.status = InspectionStatus::Failed;
                return Err(RunError::new(
                    "试演达到1MiB/32768项输出或真实状态预算；本次未返回的输出不完整，旧证据保留",
                ));
            }
            Some(ReplayStop::Status(ReplayStatus::Cancelled)) => ContinuationOutcome::Cancelled,
            Some(ReplayStop::Status(ReplayStatus::TimeBudgetExceeded)) => {
                ContinuationOutcome::TimeBudgetExceeded
            }
            Some(_) => ContinuationOutcome::StepBudgetExceeded,
            None if self.is_paused() => ContinuationOutcome::Choice,
            None => ContinuationOutcome::Ended,
        };
        if let Err(error) = view_guard(self) {
            guard.blocked = true;
            guard.output_complete = false;
            return Err(error);
        }
        self.inspection.status = match outcome {
            ContinuationOutcome::Choice => InspectionStatus::Choice,
            ContinuationOutcome::Ended => InspectionStatus::Ended,
            ContinuationOutcome::StepBudgetExceeded => InspectionStatus::StepBudgetExceeded,
            ContinuationOutcome::TimeBudgetExceeded => InspectionStatus::TimeBudgetExceeded,
            ContinuationOutcome::Cancelled => InspectionStatus::Cancelled,
        };
        self.continuation_outputs
            .extend(result.outputs.iter().cloned());
        if !outcome.is_suspended() {
            let outputs = std::mem::take(&mut self.continuation_outputs);
            self.record_continuation(&outputs);
        }
        Ok(BoundedContinuation {
            outputs: result.outputs,
            outcome,
            executed_steps: guard.last_steps,
        })
    }
}

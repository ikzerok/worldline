//! 展示快照必须由调用方重新准备；持久身份本身不授权运行。
use super::PresentationContext;
use crate::{
    ReplayBudget, ReplayCancellation, ReplayCheckpoint, ReplayResult, ReplaySession, ReplayTrace,
    RunError, Story,
};
use worldline_core::{localization::LocalizationPresentationSnapshot, Analysis, Program};

impl<'p> Story<'p> {
    pub fn load_with_presentation(
        program: &'p Program,
        analysis: &'p Analysis,
        json: &str,
        snapshot: &LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        Self::load_with_context(
            program,
            analysis,
            json,
            Some(PresentationContext::new(snapshot)),
        )
    }

    pub fn from_checkpoint_with_presentation(
        program: &'p Program,
        analysis: &'p Analysis,
        checkpoint: &ReplayCheckpoint,
        snapshot: &LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        Self::from_checkpoint_with_context(
            program,
            analysis,
            checkpoint,
            Some(PresentationContext::new(snapshot)),
        )
    }
}

impl ReplayTrace {
    pub fn replay_with_presentation(
        program: &Program,
        analysis: &Analysis,
        trace: &ReplayTrace,
        limits: ReplayBudget,
        cancellation: &ReplayCancellation,
        snapshot: &LocalizationPresentationSnapshot,
    ) -> Result<ReplayResult, RunError> {
        Self::replay_with_context(
            program,
            analysis,
            trace,
            limits,
            cancellation,
            Some(PresentationContext::new(snapshot)),
        )
    }
}

impl ReplaySession {
    pub fn new_with_presentation(
        trace: ReplayTrace,
        limits: ReplayBudget,
        cancellation: ReplayCancellation,
        snapshot: &LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        Self::new_with_context(
            trace,
            limits,
            cancellation,
            Some(PresentationContext::new(snapshot)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::{MonotonicInstant, ReplayExecutionBudget, ReplayStop};
    use crate::ReplayStatus;
    use std::time::Duration;

    #[test]
    fn restoration_time_does_not_starve_slices_or_reset_aggregate_limits() {
        let token = ReplayCancellation::new();
        let old = MonotonicInstant::now() - Duration::from_secs(2);
        let mut budget = ReplayExecutionBudget {
            limits: ReplayBudget::new(5, u64::MAX),
            cancellation: &token,
            started: old,
            steps: 3,
            slice: Some(ReplayBudget::new(1, 1000)),
            slice_started: old,
            slice_steps: 0,
            comparison_limit: None,
            output_usage: None,
        };
        assert!(budget.slice_exhausted());
        budget.resume_slice_after_restore();
        assert!(!budget.slice_exhausted());
        assert_eq!(budget.started, old);
        assert_eq!(budget.steps, 3);
        assert!(budget.consume_step().is_none());
        assert!(budget.slice_exhausted());
        budget.limits.time_budget_ms = 1;
        budget.resume_slice_after_restore();
        assert!(matches!(
            budget.consume_step(),
            Some(ReplayStop::Status(ReplayStatus::TimeBudgetExceeded))
        ));
        assert_eq!(budget.steps, 4);
    }
}

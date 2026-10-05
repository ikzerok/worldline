use super::*;
use crate::{
    action_capture::ActionCapture,
    execution::ReplayExecutionBudget,
    replay_runner::{replay_story, run_replay_slice, ReplayCursor, ReplayProgress},
    AccessCoverage, ReplayCheckpoint, ReplayOrigin, ReplayTrace, Story,
};
use worldline_core::CompileResult;

pub(super) struct Side {
    pub trace: ReplayTrace,
    pub cursor: Option<ReplayCursor>,
    checkpoint: Option<ReplayCheckpoint>,
    evidence: ActionCapture,
    inherited: AccessCoverage,
    pub result: Option<RouteSideResult>,
    steps: u64,
}
impl Side {
    pub fn new(trace: ReplayTrace) -> Self {
        Self {
            trace,
            cursor: None,
            checkpoint: None,
            evidence: Default::default(),
            inherited: Default::default(),
            result: None,
            steps: 0,
        }
    }
    pub fn advance(
        &mut self,
        snapshot: &CompileResult,
        options: RouteComparisonOptions,
        budget: &mut ReplayExecutionBudget<'_>,
    ) -> Result<(), RouteComparisonError> {
        if self.result.is_some() {
            return Ok(());
        }
        if self.cursor.is_none() && budget.cancellation.is_cancelled() {
            self.result = Some(projection::empty_result(
                &self.trace,
                RouteStatus::Cancelled,
                None,
            ));
            return Ok(());
        }
        let restored = match &self.checkpoint {
            Some(checkpoint) => {
                Story::from_checkpoint(&snapshot.program, &snapshot.analysis, checkpoint)
            }
            None => replay_story(&snapshot.program, &snapshot.analysis, &self.trace),
        };
        let mut story = match restored {
            Ok(story) => story,
            Err(error) => {
                if self.checkpoint.is_some()
                    || matches!(self.trace.origin, ReplayOrigin::Checkpoint { .. })
                {
                    return Err(RouteComparisonError::new("invalid_trace", error.message));
                }
                self.result = Some(projection::empty_result(
                    &self.trace,
                    RouteStatus::StoryFailed,
                    Some(error.message),
                ));
                return Ok(());
            }
        };
        if self.cursor.is_some() {
            story.action_capture = std::mem::take(&mut self.evidence);
        } else {
            story
                .action_capture
                .enable_variable_writes(options.max_evidence_records, options.max_evidence_bytes);
        }
        check_story(&story, options.max_output_bytes)?;
        if self.cursor.is_none() {
            if matches!(self.trace.origin, ReplayOrigin::Checkpoint { .. }) {
                self.inherited = story.access_coverage();
            }
            self.cursor = Some(ReplayCursor::new(&story));
        }
        let cursor = self.cursor.as_mut().expect("比较游标已建立");
        // 恢复本身不可抢占；重新开始执行片的时钟，避免大检查点在每次恢复后零推进让出。
        budget.slice_started = crate::execution::MonotonicInstant::now();
        let start_steps = budget.steps;
        let progress = run_replay_slice(&self.trace, cursor, &mut story, budget);
        self.steps += budget.steps - start_steps;
        check_story(&story, options.max_output_bytes)?;
        match progress {
            ReplayProgress::Yielded => {
                super::checkpoint_limit::check_checkpoint(&story, options.max_output_bytes)?;
                self.checkpoint =
                    Some(story.checkpoint().map_err(|error| {
                        RouteComparisonError::new("output_limit", error.message)
                    })?);
                encoded_size(self.checkpoint.as_ref().unwrap(), options.max_output_bytes)?;
                self.evidence = std::mem::take(&mut story.action_capture);
            }
            ReplayProgress::Finished(end) => {
                self.result = Some(projection::result(
                    &self.trace,
                    &story,
                    &self.inherited,
                    projection::status(end.status),
                    self.steps,
                    cursor.completed_choices,
                ));
            }
            ReplayProgress::OutputBudgetExceeded => {
                self.result = Some(projection::result(
                    &self.trace,
                    &story,
                    &self.inherited,
                    (
                        RouteStatus::OutputBudgetExceeded,
                        Some("本次比较的实际输出超过额度".into()),
                        None,
                        false,
                    ),
                    self.steps,
                    cursor.completed_choices,
                ));
            }
            ReplayProgress::Rejected(error) => return Err(error),
        }
        Ok(())
    }
}

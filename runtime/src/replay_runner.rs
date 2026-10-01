use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant as MonotonicInstant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant as MonotonicInstant;

use worldline_core::ast::Program;
use worldline_core::Analysis;

use super::execution::{ReplayExecutionBudget, ReplayStop};
use super::{
    Output, ReplayBudget, ReplayCancellation, ReplayCheckpoint, ReplayObservation, ReplayOrigin,
    ReplayResult, ReplayStatus, ReplayTrace, RunError, Story, REPLAY_SCHEMA_VERSION,
};

#[cfg(test)]
#[path = "replay_runner/tests.rs"]
mod tests;

struct ReplayCursor {
    initial_state: serde_json::Value,
    initial_pending: bool,
    pending_outputs: Vec<Output>,
    step_index: usize,
    completed_choices: usize,
    choice_pending: bool,
}

impl ReplayCursor {
    fn new(story: &Story<'_>) -> Self {
        Self {
            initial_state: story.state_view(),
            initial_pending: true,
            pending_outputs: Vec::new(),
            step_index: 0,
            completed_choices: 0,
            choice_pending: false,
        }
    }
}

#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        clippy::large_enum_variant,
        reason = "Boxing the final result allocates on every replay"
    )
)]
#[cfg_attr(
    target_arch = "wasm32",
    allow(
        clippy::large_enum_variant,
        reason = "wasm32 shrinks the payload below the lint threshold"
    )
)]
enum ReplayProgress {
    Yielded,
    Finished(ReplayResult),
}

/// Cooperative replay state for callers that return control between bounded slices.
///
/// Reuse the same program and analysis for every `advance` call. `Ok(None)` means the
/// replay yielded; `Ok(Some(result))` is its final structured status, including cancellation.
pub struct ReplaySession {
    trace: ReplayTrace,
    limits: ReplayBudget,
    cancellation: ReplayCancellation,
    started: MonotonicInstant,
    executed_steps: u64,
    checkpoint: Option<ReplayCheckpoint>,
    cursor: Option<ReplayCursor>,
    finished: bool,
}

impl ReplaySession {
    /// Validate and start a cooperative replay without executing story statements.
    pub fn new(
        trace: ReplayTrace,
        limits: ReplayBudget,
        cancellation: ReplayCancellation,
    ) -> Result<Self, RunError> {
        validate_replay_trace(&trace)?;
        Ok(Self {
            trace,
            limits,
            cancellation,
            started: MonotonicInstant::now(),
            executed_steps: 0,
            checkpoint: None,
            cursor: None,
            finished: false,
        })
    }

    /// Execute up to `slice` interpreter steps or elapsed time, whichever comes first.
    /// The aggregate limits supplied to `new` apply across every call.
    pub fn advance(
        &mut self,
        program: &Program,
        analysis: &Analysis,
        slice: ReplayBudget,
    ) -> Result<Option<ReplayResult>, RunError> {
        if self.finished {
            return Err(RunError::new("重放会话已完成"));
        }
        let mut budget = ReplayExecutionBudget {
            limits: self.limits,
            cancellation: &self.cancellation,
            started: self.started,
            steps: self.executed_steps,
            slice: Some(slice),
            slice_started: MonotonicInstant::now(),
            slice_steps: 0,
        };
        let mut story = match &self.checkpoint {
            Some(checkpoint) => Story::from_checkpoint(program, analysis, checkpoint)?,
            None => replay_story(program, analysis, &self.trace)?,
        };
        let cursor = self.cursor.get_or_insert_with(|| ReplayCursor::new(&story));
        let progress = run_replay_slice(&self.trace, cursor, &mut story, &mut budget);
        let executed_steps = budget.steps;
        self.executed_steps = executed_steps;
        match progress {
            ReplayProgress::Yielded => {
                let checkpoint = match story.checkpoint() {
                    Ok(checkpoint) => checkpoint,
                    Err(error) => {
                        self.finished = true;
                        return Err(error);
                    }
                };
                self.checkpoint = Some(checkpoint);
                Ok(None)
            }
            ReplayProgress::Finished(result) => {
                self.finished = true;
                Ok(Some(result))
            }
        }
    }
}

impl ReplayTrace {
    /// Replay a recorded input sequence against a compiled story. Entry traces may be
    /// checked against changed source; checkpoint traces require an exact fingerprint.
    pub fn replay(
        program: &Program,
        analysis: &Analysis,
        trace: &ReplayTrace,
        limits: ReplayBudget,
        cancellation: &ReplayCancellation,
    ) -> Result<ReplayResult, RunError> {
        validate_replay_trace(trace)?;
        let mut story = replay_story(program, analysis, trace)?;
        let started = MonotonicInstant::now();
        let mut budget = ReplayExecutionBudget {
            limits,
            cancellation,
            started,
            steps: 0,
            slice: None,
            slice_started: started,
            slice_steps: 0,
        };
        let mut cursor = ReplayCursor::new(&story);
        match run_replay_slice(trace, &mut cursor, &mut story, &mut budget) {
            ReplayProgress::Finished(result) => Ok(result),
            ReplayProgress::Yielded => Err(RunError::new("同步重放意外让出执行")),
        }
    }
}

fn validate_replay_trace(trace: &ReplayTrace) -> Result<(), RunError> {
    if trace.schema_version != REPLAY_SCHEMA_VERSION {
        return Err(RunError::new("重放 trace schema_version 不兼容"));
    }
    if trace.runtime_version != env!("CARGO_PKG_VERSION") {
        return Err(RunError::new("重放 trace runtime_version 不兼容"));
    }
    if let ReplayOrigin::Checkpoint { checkpoint } = &trace.origin {
        if checkpoint.fingerprint != trace.fingerprint {
            return Err(RunError::new("trace 与检查点 fingerprint 不一致"));
        }
    }
    Ok(())
}

fn replay_story<'p>(
    program: &'p Program,
    analysis: &'p Analysis,
    trace: &ReplayTrace,
) -> Result<Story<'p>, RunError> {
    match &trace.origin {
        ReplayOrigin::Entry { seed } => Story::new_with_seed(program, analysis, *seed),
        ReplayOrigin::Checkpoint { checkpoint } => {
            Story::from_checkpoint(program, analysis, checkpoint)
        }
    }
}

fn run_replay_slice(
    trace: &ReplayTrace,
    cursor: &mut ReplayCursor,
    story: &mut Story<'_>,
    budget: &mut ReplayExecutionBudget<'_>,
) -> ReplayProgress {
    if budget.cancellation.is_cancelled() {
        return ReplayProgress::Finished(make_replay_result(
            ReplayStatus::Cancelled,
            budget.steps,
            cursor.completed_choices,
            trace.fingerprint,
            story,
            &cursor.initial_state,
        ));
    }

    if cursor.initial_pending {
        let initial_actual = if story.is_paused() {
            story.observation(&[])
        } else {
            let mut outcome = match story.continue_story_inner(&mut *budget) {
                Ok(outcome) => outcome,
                Err(error) => {
                    return ReplayProgress::Finished(make_replay_result(
                        ReplayStatus::StoryFailed {
                            message: error.message,
                            node: error.node,
                            line: error.line,
                        },
                        budget.steps,
                        cursor.completed_choices,
                        trace.fingerprint,
                        story,
                        &cursor.initial_state,
                    ));
                }
            };
            cursor.pending_outputs.append(&mut outcome.outputs);
            if let Some(stop) = outcome.stop {
                match stop {
                    ReplayStop::Yield => return ReplayProgress::Yielded,
                    ReplayStop::Status(status) => {
                        let outputs = std::mem::take(&mut cursor.pending_outputs);
                        story.record_continuation(&outputs);
                        return ReplayProgress::Finished(make_replay_result(
                            status,
                            budget.steps,
                            cursor.completed_choices,
                            trace.fingerprint,
                            story,
                            &cursor.initial_state,
                        ));
                    }
                }
            }
            let outputs = std::mem::take(&mut cursor.pending_outputs);
            story.record_continuation(&outputs);
            story.observation(&outputs)
        };
        cursor.initial_state = initial_actual.state.clone();
        if let Some(expected) = &trace.initial_observation {
            if !observations_match(expected, &initial_actual) {
                return ReplayProgress::Finished(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index: 0,
                        reason: "初始输出、状态或选择组不匹配".into(),
                        expected_choice: None,
                        actual_choices: initial_actual.choices,
                    },
                    budget.steps,
                    cursor.completed_choices,
                    trace.fingerprint,
                    story,
                    &expected.state,
                ));
            }
        }
        cursor.initial_pending = false;
    }

    while cursor.step_index < trace.steps.len() {
        if budget.cancellation.is_cancelled() {
            return ReplayProgress::Finished(make_replay_result(
                ReplayStatus::Cancelled,
                budget.steps,
                cursor.completed_choices,
                trace.fingerprint,
                story,
                &cursor.initial_state,
            ));
        }
        if budget.slice_exhausted() {
            return ReplayProgress::Yielded;
        }
        let step = &trace.steps[cursor.step_index];
        if !cursor.choice_pending {
            let Some(choice_index) = story
                .choices()
                .iter()
                .position(|choice| choice.id == step.choice.id)
            else {
                return ReplayProgress::Finished(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index: cursor.step_index,
                        reason: "记录的选择在当前暂停组中不存在".into(),
                        expected_choice: Some(step.choice.clone()),
                        actual_choices: story.observation(&[]).choices,
                    },
                    budget.steps,
                    cursor.completed_choices,
                    trace.fingerprint,
                    story,
                    &cursor.initial_state,
                ));
            };
            if let Err(error) = story.choose(choice_index) {
                return ReplayProgress::Finished(make_replay_result(
                    ReplayStatus::StoryFailed {
                        message: error.message,
                        node: error.node,
                        line: error.line,
                    },
                    budget.steps,
                    cursor.completed_choices,
                    trace.fingerprint,
                    story,
                    &cursor.initial_state,
                ));
            }
            cursor.completed_choices += 1;
            cursor.choice_pending = true;
        }
        let Some(expected_observation) = &step.observation else {
            return ReplayProgress::Finished(make_replay_result(
                ReplayStatus::IncompleteTrace,
                budget.steps,
                cursor.completed_choices,
                trace.fingerprint,
                story,
                &cursor.initial_state,
            ));
        };
        let mut outcome = match story.continue_story_inner(&mut *budget) {
            Ok(outcome) => outcome,
            Err(error) => {
                return ReplayProgress::Finished(make_replay_result(
                    ReplayStatus::StoryFailed {
                        message: error.message,
                        node: error.node,
                        line: error.line,
                    },
                    budget.steps,
                    cursor.completed_choices,
                    trace.fingerprint,
                    story,
                    &cursor.initial_state,
                ));
            }
        };
        cursor.pending_outputs.append(&mut outcome.outputs);
        if let Some(stop) = outcome.stop {
            match stop {
                ReplayStop::Yield => return ReplayProgress::Yielded,
                ReplayStop::Status(status) => {
                    let outputs = std::mem::take(&mut cursor.pending_outputs);
                    story.record_continuation(&outputs);
                    return ReplayProgress::Finished(make_replay_result(
                        status,
                        budget.steps,
                        cursor.completed_choices,
                        trace.fingerprint,
                        story,
                        &cursor.initial_state,
                    ));
                }
            }
        }
        let outputs = std::mem::take(&mut cursor.pending_outputs);
        story.record_continuation(&outputs);
        let actual = story.observation(&outputs);
        if !observations_match(expected_observation, &actual) {
            return ReplayProgress::Finished(make_replay_result(
                ReplayStatus::Diverged {
                    step_index: cursor.step_index + 1,
                    reason: "选择后的输出、状态或选择组不匹配".into(),
                    expected_choice: Some(step.choice.clone()),
                    actual_choices: actual.choices,
                },
                budget.steps,
                cursor.completed_choices,
                trace.fingerprint,
                story,
                &expected_observation.state,
            ));
        }
        cursor.step_index += 1;
        cursor.choice_pending = false;
    }

    ReplayProgress::Finished(make_replay_result(
        ReplayStatus::Replayed {
            ended: story.is_ended(),
            complete: trace.complete && story.is_ended(),
        },
        budget.steps,
        cursor.completed_choices,
        trace.fingerprint,
        story,
        &cursor.initial_state,
    ))
}

fn observations_match(expected: &ReplayObservation, actual: &ReplayObservation) -> bool {
    expected.outputs == actual.outputs
        && presentation_semantics(&expected.choice_presentation)
            == presentation_semantics(&actual.choice_presentation)
        && expected
            .choices
            .iter()
            .map(|choice| (&choice.id, &choice.label))
            .eq(actual
                .choices
                .iter()
                .map(|choice| (&choice.id, &choice.label)))
        && semantic_state(&expected.state) == semantic_state(&actual.state)
}

fn presentation_semantics(presentation: &[serde_json::Value]) -> Vec<serde_json::Value> {
    presentation
        .iter()
        .map(|item| {
            let mut item = item.clone();
            if let Some(object) = item.as_object_mut() {
                object.remove("line");
            }
            item
        })
        .collect()
}

fn semantic_state(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(calls) = value
        .get_mut("calls")
        .and_then(serde_json::Value::as_array_mut)
    {
        for call in calls {
            if let Some(call) = call.as_object_mut() {
                // Only these frame-level fields are source metadata. Keep all
                // semantic/unknown fields, including locals named file or line.
                call.remove("file");
                call.remove("line");
            }
        }
    }
    if let Some(choices) = value
        .get_mut("coverage")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|coverage| coverage.get_mut("selected_choices"))
        .and_then(serde_json::Value::as_array_mut)
    {
        for choice in choices {
            if let Some(choice) = choice.as_object_mut() {
                choice.remove("line");
            }
        }
    }
    value
}

fn make_replay_result(
    status: ReplayStatus,
    executed_steps: u64,
    completed_choices: usize,
    original_fingerprint: u64,
    story: &Story<'_>,
    initial_state: &serde_json::Value,
) -> ReplayResult {
    let current_state = story.state_view();
    let mut state_diff = BTreeMap::new();
    if let (Some(before), Some(after)) = (initial_state.as_object(), current_state.as_object()) {
        for key in before.keys().chain(after.keys()) {
            let before_value = before.get(key);
            let after_value = after.get(key);
            if before_value != after_value {
                state_diff.insert(
                    key.clone(),
                    after_value.cloned().unwrap_or(serde_json::Value::Null),
                );
            }
        }
    }
    ReplayResult {
        status,
        executed_steps,
        completed_choices,
        source_fingerprint: story.fingerprint,
        original_fingerprint,
        current_node: story.current_node(),
        current_state,
        state_diff,
        coverage: story.access_coverage(),
    }
}

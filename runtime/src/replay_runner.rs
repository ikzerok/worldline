use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant as MonotonicInstant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant as MonotonicInstant;

use worldline_core::ast::Program;
use worldline_core::Analysis;

use super::execution::ReplayExecutionBudget;
use super::{
    ReplayBudget, ReplayCancellation, ReplayObservation, ReplayOrigin, ReplayResult, ReplayStatus,
    ReplayTrace, RunError, Story, REPLAY_SCHEMA_VERSION,
};

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
        if trace.schema_version != REPLAY_SCHEMA_VERSION {
            return Err(RunError::new("重放 trace schema_version 不兼容"));
        }
        if trace.runtime_version != env!("CARGO_PKG_VERSION") {
            return Err(RunError::new("重放 trace runtime_version 不兼容"));
        }
        let mut story = match &trace.origin {
            ReplayOrigin::Entry { seed } => Story::new_with_seed(program, analysis, *seed)?,
            ReplayOrigin::Checkpoint { checkpoint } => {
                if checkpoint.fingerprint != trace.fingerprint {
                    return Err(RunError::new("trace 与检查点 fingerprint 不一致"));
                }
                Story::from_checkpoint(program, analysis, checkpoint)?
            }
        };
        let mut budget = ReplayExecutionBudget {
            limits,
            cancellation,
            started: MonotonicInstant::now(),
            steps: 0,
        };
        let mut initial_state = story.state_view();

        if cancellation.is_cancelled() {
            return Ok(make_replay_result(
                ReplayStatus::Cancelled,
                0,
                0,
                trace.fingerprint,
                &story,
                &initial_state,
            ));
        }

        let initial_actual = if story.is_paused() {
            story.observation(&[])
        } else {
            let outcome = match story.continue_story_inner(Some(&mut budget)) {
                Ok(outcome) => outcome,
                Err(error) => {
                    return Ok(make_replay_result(
                        ReplayStatus::StoryFailed {
                            message: error.message,
                            node: error.node,
                            line: error.line,
                        },
                        budget.steps,
                        0,
                        trace.fingerprint,
                        &story,
                        &initial_state,
                    ));
                }
            };
            let outputs = outcome.outputs;
            story.record_continuation(&outputs);
            if let Some(status) = outcome.stop {
                return Ok(make_replay_result(
                    status,
                    budget.steps,
                    0,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
            story.observation(&outputs)
        };
        initial_state = initial_actual.state.clone();
        if let Some(expected) = &trace.initial_observation {
            if !observations_match(expected, &initial_actual) {
                return Ok(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index: 0,
                        reason: "初始输出、状态或选择组不匹配".into(),
                        expected_choice: None,
                        actual_choices: initial_actual.choices,
                    },
                    budget.steps,
                    0,
                    trace.fingerprint,
                    &story,
                    &expected.state,
                ));
            }
        }

        let mut completed_choices = 0;
        for (step_index, step) in trace.steps.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Ok(make_replay_result(
                    ReplayStatus::Cancelled,
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
            let Some(choice_index) = story
                .choices()
                .iter()
                .position(|choice| choice.id == step.choice.id)
            else {
                return Ok(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index,
                        reason: "记录的选择在当前暂停组中不存在".into(),
                        expected_choice: Some(step.choice.clone()),
                        actual_choices: story.observation(&[]).choices,
                    },
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            };
            if let Err(error) = story.choose(choice_index) {
                return Ok(make_replay_result(
                    ReplayStatus::StoryFailed {
                        message: error.message,
                        node: error.node,
                        line: error.line,
                    },
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
            completed_choices += 1;
            let Some(expected_observation) = &step.observation else {
                return Ok(make_replay_result(
                    ReplayStatus::IncompleteTrace,
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            };
            let outcome = match story.continue_story_inner(Some(&mut budget)) {
                Ok(outcome) => outcome,
                Err(error) => {
                    return Ok(make_replay_result(
                        ReplayStatus::StoryFailed {
                            message: error.message,
                            node: error.node,
                            line: error.line,
                        },
                        budget.steps,
                        completed_choices,
                        trace.fingerprint,
                        &story,
                        &initial_state,
                    ));
                }
            };
            let outputs = outcome.outputs;
            story.record_continuation(&outputs);
            if let Some(status) = outcome.stop {
                return Ok(make_replay_result(
                    status,
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
            let actual = story.observation(&outputs);
            if !observations_match(expected_observation, &actual) {
                return Ok(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index: step_index + 1,
                        reason: "选择后的输出、状态或选择组不匹配".into(),
                        expected_choice: Some(step.choice.clone()),
                        actual_choices: actual.choices,
                    },
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &expected_observation.state,
                ));
            }
        }

        let status = ReplayStatus::Replayed {
            ended: story.is_ended(),
            complete: trace.complete && story.is_ended(),
        };
        Ok(make_replay_result(
            status,
            budget.steps,
            completed_choices,
            trace.fingerprint,
            &story,
            &initial_state,
        ))
    }
}

fn observations_match(expected: &ReplayObservation, actual: &ReplayObservation) -> bool {
    expected.outputs == actual.outputs
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

fn semantic_state(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
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

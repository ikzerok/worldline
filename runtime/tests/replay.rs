//! Replay protocol behavior through the public runtime API.

use worldline_core::{compile_source, CompileResult};
use worldline_runtime::{
    ChoiceExplanation, ReplayBudget, ReplayCancellation, ReplayStatus, ReplayTrace, Story,
};

fn compile(source: &str) -> CompileResult {
    let result = compile_source("replay.wl", source);
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    result
}

fn play_to_end(result: &CompileResult, seed: u64) -> serde_json::Value {
    let mut story = Story::new_with_seed(&result.program, &result.analysis, seed).unwrap();
    let mut events = Vec::new();
    loop {
        let outputs = story.continue_story().unwrap();
        events.push(serde_json::to_value(outputs).unwrap());
        if story.is_ended() {
            break;
        }
        story.choose(0).unwrap();
    }
    serde_json::json!({"events":events,"state":story.state_view()})
}

fn captured_trace(result: &CompileResult, seed: u64) -> ReplayTrace {
    let mut story = Story::new_with_seed(&result.program, &result.analysis, seed).unwrap();
    loop {
        let outputs = story.continue_story().unwrap();
        if story.is_ended() {
            break;
        }
        if story.choices().is_empty() {
            continue;
        }
        if !outputs.is_empty() {
            // Capture outputs and the current decision before making a choice.
        }
        story.choose(0).unwrap();
    }
    story.replay_trace()
}

#[test]
fn explicit_seed_repeats_random_output_and_trace_replays_it() {
    let result = compile("event start\n  值:{rnd(1, 100)}\n  choice \"结束\"\n    -> END\n");
    let first = play_to_end(&result, 9041);
    let second = play_to_end(&result, 9041);
    assert_eq!(first, second);

    let trace = captured_trace(&result, 9041);
    let replay = ReplayTrace::replay(
        &result.program,
        &result.analysis,
        &trace,
        ReplayBudget::new(10_000, 5_000),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(matches!(
        replay.status,
        ReplayStatus::Replayed { ended: true, .. }
    ));
    assert_eq!(replay.current_state, first["state"]);
}

#[test]
fn checkpoint_roundtrip_binds_runtime_and_program_but_does_not_write_project() {
    let result = compile("event start\n  before\n  choice \"continue\"\n    -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 7).unwrap();
    story.continue_story().unwrap();
    let before = story.save().unwrap();
    let checkpoint = story.checkpoint().unwrap();
    assert_eq!(story.save().unwrap(), before);
    let restored = Story::from_checkpoint(&result.program, &result.analysis, &checkpoint).unwrap();
    assert_eq!(restored.state_view(), story.state_view());
    assert_eq!(checkpoint.fingerprint, result.analysis.fingerprint);

    let changed = compile("event start\n  changed\n  choice \"continue\"\n    -> END\n");
    let error = match Story::from_checkpoint(&changed.program, &changed.analysis, &checkpoint) {
        Err(error) => error,
        Ok(_) => panic!("changed source unexpectedly accepted the checkpoint"),
    };
    assert!(error.message.contains("fingerprint"));
}

#[test]
fn condition_explanation_is_read_only_and_reports_false_and_once_reasons() {
    let result = compile(
        "tag calm\nworld setting\nstate mood on world setting with []\nevent start\n  choice once \"loop\"\n    -> start\n  choice \"needs calm\" if has(mood, calm)\n    -> END\n  choice \"finish\"\n    -> END\n",
    );
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 13).unwrap();
    // Select the once option, then inspect the following choice group.
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let before = story.save().unwrap();
    let explanations: Vec<ChoiceExplanation> = story.explain_choices().unwrap();
    assert_eq!(story.save().unwrap(), before);
    assert_eq!(explanations.len(), 3);
    assert!(!explanations[0].available);
    assert!(explanations[0]
        .unavailable_reason
        .as_deref()
        .unwrap()
        .contains("once"));
    assert!(!explanations[1].available);
    assert_eq!(
        explanations[1].condition.as_ref().unwrap().result,
        Some(false)
    );
    assert!(explanations[1]
        .unavailable_reason
        .as_deref()
        .unwrap()
        .contains("false"));
}

#[test]
fn changed_choice_stops_replay_without_falling_back_to_the_old_index() {
    let original = compile(
        "event start\n  choice \"north\" if true\n    -> END\n  choice \"south\"\n    -> END\n",
    );
    let trace = captured_trace(&original, 123);
    let changed = compile(
        "event start\n  choice \"east\" if true\n    -> END\n  choice \"south\"\n    -> END\n",
    );
    let replay = ReplayTrace::replay(
        &changed.program,
        &changed.analysis,
        &trace,
        ReplayBudget::new(100, 1_000),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(matches!(replay.status, ReplayStatus::Diverged { .. }));
    assert_eq!(replay.completed_choices, 0);
    assert_eq!(replay.current_node.as_deref(), Some("start"));
}

#[test]
fn self_loop_respects_step_budget_and_pre_cancelled_replay_stops_immediately() {
    let result = compile("event start\n  -> start\n");
    let trace = Story::new_with_seed(&result.program, &result.analysis, 1)
        .unwrap()
        .replay_trace();
    let replay = ReplayTrace::replay(
        &result.program,
        &result.analysis,
        &trace,
        ReplayBudget::new(40, 5_000),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(replay.status, ReplayStatus::StepBudgetExceeded);
    assert_eq!(replay.executed_steps, 40);

    let cancellation = ReplayCancellation::new();
    cancellation.cancel();
    let replay = ReplayTrace::replay(
        &result.program,
        &result.analysis,
        &trace,
        ReplayBudget::new(40, 5_000),
        &cancellation,
    )
    .unwrap();
    assert_eq!(replay.status, ReplayStatus::Cancelled);
    assert_eq!(replay.executed_steps, 0);
}

#[test]
fn mid_story_trace_starts_from_a_checkpoint_and_reports_only_observed_coverage() {
    let result = compile(
        "event start\n  scene first\n    choice \"continue\"\n      -> next\nevent next\n  choice \"end\"\n    -> END\n",
    );
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 5).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    story.start_trace_from_here().unwrap();
    assert_eq!(story.replay_trace().origin.kind(), "checkpoint");
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let coverage = story.access_coverage();
    assert!(coverage.visited_nodes.contains_key("start"));
    assert!(coverage.visited_nodes.contains_key("next"));
    assert_eq!(coverage.selected_choices.len(), 2);
    assert!(coverage
        .selected_choices
        .iter()
        .all(|choice| choice.count == 1));
}

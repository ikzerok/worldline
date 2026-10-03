//! 独立边界：实际动作生命周期、循环前缀与资源停止。
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    compare_routes, ReplayCancellation, ReplayTrace, RouteComparisonOptions, RouteStatus, Story,
};
fn compile(source: &str) -> CompileResult {
    let value = compile_source_with_options("edges.wl", source, CompileOptions::v1_13());
    assert!(!value.has_errors(), "{:?}", value.diagnostics);
    value
}
fn record(snapshot: &CompileResult, choices: &[usize]) -> ReplayTrace {
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    story.continue_story().unwrap();
    for choice in choices {
        story.choose(*choice).unwrap();
        story.continue_story().unwrap();
    }
    story.replay_trace()
}
#[test]
fn fragments_same_value_and_effect_enter_done_exit_preserve_actual_order() {
    let snapshot=compile("tag one\ntag two\nworld setting\nstate fate on world setting with []\nfragment touch()\n  become state(fate) add from tags(tag(one))\n  return\nevent start\n  effect on enter\n    become fate add one\n  effect on done\n    become fate add two\n  effect on exit\n    become fate remove one\n  call touch()\n  call touch()\n");
    let trace = record(&snapshot, &[]);
    let compared = compare_routes(
        &snapshot,
        &trace,
        &trace,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    let actions = &compared.left.state_actions.records;
    assert_eq!(actions.len(), 5);
    assert_eq!(compared.left.states.as_ref().unwrap()["fate"], vec!["two"]);
    for action in actions {
        assert_eq!(action.event.as_deref(), Some("start"));
        worldline_core::evidence_source::resolve_evidence_source(
            &snapshot,
            action.source.as_ref().unwrap(),
        )
        .unwrap();
    }
    assert_eq!(actions[1].node.as_deref(), Some("fragment:touch"));
    assert_eq!(actions[1].before, actions[1].after);
    assert_eq!(actions[1].source, actions[2].source);
    assert_ne!(actions[1].sequence, actions[2].sequence);
    let timings = actions
        .iter()
        .map(|action| match &action.source.as_ref().unwrap().owner {
            worldline_core::evidence_source::EvidenceSourceOwner::StateAction {
                timing, ..
            } => timing.as_str(),
            _ => panic!("wrong source owner"),
        })
        .collect::<Vec<_>>();
    assert_eq!(timings, vec!["enter", "during", "during", "done", "exit"]);
}
#[test]
fn looping_inputs_align_occurrences_and_partial_does_not_mean_complete() {
    let snapshot=compile("let n = 0\nevent start\n  choice \"再来\"\n    set n = n + 1\n    -> start\n  choice \"结束\"\n    -> END\n");
    let a = record(&snapshot, &[0, 0, 1]);
    let b = record(&snapshot, &[0, 1]);
    let compared = compare_routes(
        &snapshot,
        &a,
        &b,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(compared.alignment.common_prefix, 1);
    assert_eq!(compared.alignment.first_difference.unwrap().index, 1);
    let partial = record(&snapshot, &[0]);
    let compared = compare_routes(
        &snapshot,
        &partial,
        &b,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(compared.left.status, RouteStatus::Replayed);
    assert!(!compared.left.ended && !compared.left.complete);
    assert_eq!(compared.alignment.common_prefix, 1);
    assert!(compared.alignment.first_difference.is_none());
    let mut incomplete = b.clone();
    incomplete.steps[0].observation = None;
    let compared = compare_routes(
        &snapshot,
        &incomplete,
        &b,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(compared.left.status, RouteStatus::IncompleteTrace);
    assert!(!compared.left.complete);
}
#[test]
fn oversized_evidence_field_is_omitted_without_removing_the_real_action() {
    let note = "x".repeat(4096);
    let snapshot=compile(&format!("tag one\nworld setting\nstate fate on world setting with []\nevent start\n  become fate with one as \"{note}\"\n  -> END\n"));
    let trace = record(&snapshot, &[]);
    let compared = compare_routes(
        &snapshot,
        &trace,
        &trace,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(compared.left.status, RouteStatus::Replayed);
    assert!(compared.left.state_actions.omitted);
    assert_eq!(compared.left.state_actions.total_actions, 1);
    assert!(compared.left.state_actions.records.is_empty());
    assert_eq!(compared.left.states.as_ref().unwrap()["fate"], vec!["one"]);
}
#[test]
fn actual_output_budget_is_shared_and_never_compared_after_truncation() {
    let large = "x".repeat(600_000);
    let snapshot = compile(&format!("fragment output()\n  local value: str = \"{large}\"\n  {{value}}\n  return\nevent start\n  call output()\n  -> END\n"));
    assert!(!snapshot.has_errors());
    let trace = record(&snapshot, &[]);
    let compared = compare_routes(
        &snapshot,
        &trace,
        &trace,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(
        [compared.left.status, compared.right.status].contains(&RouteStatus::OutputBudgetExceeded)
    );
    for side in [&compared.left, &compared.right] {
        if side.status == RouteStatus::OutputBudgetExceeded {
            assert!(!side.complete);
        }
    }
}
#[test]
fn one_side_story_failure_retains_other_side_real_result() {
    let old=compile("let n = 0\nevent start\n  choice \"安全\"\n    set n = 1\n    -> END\n  choice \"风险\"\n    set n = 2\n    -> END\n");
    let (a, b) = (record(&old, &[0]), record(&old, &[1]));
    let changed=compile("let n = 0\nevent start\n  choice \"安全\"\n    set n = 1\n    -> END\n  choice \"风险\"\n    set n = 2 / 0\n    -> END\n");
    let compared = compare_routes(
        &changed,
        &a,
        &b,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(compared.left.status, RouteStatus::Replayed);
    assert_eq!(compared.right.status, RouteStatus::StoryFailed);
    assert!(compared
        .right
        .detail
        .as_ref()
        .is_some_and(|text| !text.is_empty()));
    assert!(compared.right.vars.is_some());
}
#[test]
fn checkpoint_runtime_019_is_rejected_but_plain_save_has_no_new_runtime_guard() {
    let snapshot = compile("event start\n  choice \"结束\"\n    -> END\n");
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    story.continue_story().unwrap();
    let plain_save = story.save().unwrap();
    let restored = Story::load(&snapshot.program, &snapshot.analysis, &plain_save).unwrap();
    assert_eq!(restored.state_view(), story.state_view());
    story.start_trace_from_here().unwrap();
    let mut trace = story.replay_trace();
    if let worldline_runtime::ReplayOrigin::Checkpoint { checkpoint } = &mut trace.origin {
        checkpoint.runtime_version = "0.19.0".into();
    }
    let error = compare_routes(
        &snapshot,
        &trace,
        &trace,
        RouteComparisonOptions::default(),
        &ReplayCancellation::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "invalid_trace");
}
#[test]
fn large_repeated_output_stops_at_a_statement_boundary_not_at_slice_end() {
    let payload = "x".repeat(600_000);
    let prefix = format!("fragment flood()\n  local payload: str = \"{payload}\"\n");
    let suffix = "  return\nevent start\n  call flood()\n  -> END\n";
    let old = compile(&format!("{prefix}  small\n{suffix}"));
    let trace = record(&old, &[]);
    let current = compile(&format!("{prefix}{}{suffix}", "  {payload}\n".repeat(128)));
    let compared = compare_routes(
        &current,
        &trace,
        &trace,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(compared.left.status, RouteStatus::OutputBudgetExceeded);
    assert_eq!(compared.right.status, RouteStatus::OutputBudgetExceeded);
    assert!(
        compared.left.executed_steps <= 4,
        "output cap must stop after the second large statement: {}",
        compared.left.executed_steps
    );
    assert!(
        compared.left.executed_steps + compared.right.executed_steps <= 6,
        "shared output budget stops on the second emitted large value"
    );
}
#[test]
fn unrecorded_initial_observation_is_not_a_verified_comparable_prefix() {
    let snapshot = compile("event start\n  choice \"结束\"\n    -> END\n");
    let story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    let unrecorded = story.replay_trace();
    assert!(unrecorded.initial_observation.is_none());
    let completed = record(&snapshot, &[0]);
    let result = compare_routes(
        &snapshot,
        &unrecorded,
        &completed,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(!result.alignment.comparable);
    assert!(result.alignment.first_difference.is_none());
    assert_eq!(result.alignment.left_verified_choices, 0);
}
#[test]
fn checkpoint_object_key_order_is_not_a_different_origin() {
    let snapshot =
        compile("let alpha = 1\nlet beta = 2\nevent start\n  choice \"结束\"\n    -> END\n");
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    story.continue_story().unwrap();
    story.start_trace_from_here().unwrap();
    let a = story.replay_trace();
    let mut b = a.clone();
    if let worldline_runtime::ReplayOrigin::Checkpoint { checkpoint } = &mut b.origin {
        let value: serde_json::Value = serde_json::from_str(&checkpoint.state).unwrap();
        checkpoint.state = serde_json::to_string_pretty(&value).unwrap();
    }
    let result = compare_routes(
        &snapshot,
        &a,
        &b,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(result.alignment.comparable);
}
#[test]
fn growing_values_are_bounded_between_statements_before_next_doubling() {
    let value = "x".repeat(8192);
    let prefix = format!("let value = \"{value}\"\nevent start\n");
    let old = compile(&format!("{prefix}  -> END\n"));
    let trace = record(&old, &[]);
    let changed = compile(&format!(
        "{prefix}{}  -> END\n",
        "  set value = value + value\n".repeat(128)
    ));
    let error = compare_routes(
        &changed,
        &trace,
        &trace,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "output_limit");
}
#[test]
fn restart_resets_transient_action_evidence_without_changing_save_shape() {
    let snapshot=compile("tag one\nworld setting\nstate fate on world setting with []\nevent start\n  effect on enter\n    become fate with one\n  become fate add one\n  -> END\n");
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.state_action_evidence().total_actions, 2);
    story.restart().unwrap();
    assert_eq!(story.state_action_evidence().total_actions, 1);
    assert_eq!(story.state_action_evidence().records[0].sequence, 1);
    assert!(!story.save().unwrap().contains("state_actions"));
}

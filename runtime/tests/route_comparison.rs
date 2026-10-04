//! 0.20真实对照不改持久语义，且所有宿主共用同一投影。
use worldline_core::{compile_source, compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    compare_routes, ReplayBudget, ReplayCancellation, ReplayTrace, RouteComparisonOptions,
    RouteComparisonSession, RouteStatus, Story,
};

const SOURCE: &str = "tag kept\ntag sent\nworld setting\nstate fate on world setting with []\nlet credits = 0\nevent start\n  effect on enter\n    become fate with []\n  scene decision\n    choice \"保留\"\n      become fate with kept\n      become fate add kept\n      set credits = 1\n      -> END\n    choice \"送出\"\n      become fate with sent\n      set credits = 10\n      -> END\n";
fn compile(source: &str) -> CompileResult {
    let result = compile_source("comparison.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn trace(result: &CompileResult, choice: usize, seed: u64) -> ReplayTrace {
    let mut story = Story::new_with_seed(&result.program, &result.analysis, seed).unwrap();
    story.continue_story().unwrap();
    if !story.is_ended() {
        story.choose(choice).unwrap();
        story.continue_story().unwrap();
    }
    story.replay_trace()
}
fn compare(
    result: &CompileResult,
    left: &ReplayTrace,
    right: &ReplayTrace,
) -> worldline_runtime::RouteComparisonResult {
    compare_routes(
        result,
        left,
        right,
        RouteComparisonOptions::default(),
        &ReplayCancellation::new(),
    )
    .unwrap()
}
#[test]
fn same_nodes_different_inputs_values_and_real_sources_do_not_change_live_story() {
    let result = compile(SOURCE);
    let (a, b) = (trace(&result, 0, 42), trace(&result, 1, 42));
    let mut live = Story::new_with_seed(&result.program, &result.analysis, 9).unwrap();
    live.continue_story().unwrap();
    let before = (live.save().unwrap(), live.replay_trace(), live.state_view());
    let compared = compare(&result, &a, &b);
    assert_eq!(compared.left.status, RouteStatus::Replayed);
    assert!(compared.left.complete && compared.right.complete && compared.left.ended);
    assert_eq!(
        compared.left.coverage.total.visited_nodes,
        compared.right.coverage.total.visited_nodes
    );
    assert_eq!(compared.state_differences.len(), 1);
    assert_eq!(compared.variable_differences.len(), 1);
    assert_eq!(compared.alignment.common_prefix, 0);
    let difference = compared.alignment.first_difference.unwrap();
    assert_eq!(difference.left.choice.id, a.steps[0].choice.id);
    for input in [&difference.left, &difference.right] {
        worldline_core::evidence_source::resolve_evidence_source(
            &result,
            input.source.as_ref().unwrap(),
        )
        .unwrap();
    }
    let actions = &compared.left.state_actions;
    assert_eq!(actions.total_actions, 3);
    assert_eq!(
        actions
            .records
            .iter()
            .map(|a| a.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(actions.records[2].before, actions.records[2].after);
    for action in &actions.records {
        assert_eq!(action.target.as_ref().unwrap().id, "setting");
        worldline_core::evidence_source::resolve_evidence_source(
            &result,
            action.source.as_ref().unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        before,
        (live.save().unwrap(), live.replay_trace(), live.state_view())
    );
    for json in [
        live.save().unwrap(),
        serde_json::to_string(&live.replay_trace()).unwrap(),
        live.state_view().to_string(),
    ] {
        assert!(!json.contains("state_actions"));
        assert!(!json.contains("state_action\""));
    }
}
#[test]
fn changed_current_source_keeps_independent_failure_and_actual_stop_values() {
    let old = compile(SOURCE);
    let (a, b) = (trace(&old, 0, 42), trace(&old, 1, 42));
    let current = compile(&SOURCE.replace("credits = 10", "credits = 11"));
    let result = compare(&current, &a, &b);
    assert_eq!(result.left.status, RouteStatus::Replayed);
    assert_eq!(result.right.status, RouteStatus::Diverged);
    assert_eq!(
        result.right.vars.unwrap()["credits"],
        worldline_runtime::Value::Num(11.0)
    );
    assert!(!result.right.complete);
    assert!(result.alignment.first_difference.is_some());
}
#[test]
fn seed_and_checkpoint_origins_are_not_aligned_by_indices() {
    let result = compile(SOURCE);
    let a = trace(&result, 0, 42);
    let b = trace(&result, 1, 9);
    assert!(!compare(&result, &a, &b).alignment.comparable);
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 42).unwrap();
    story.continue_story().unwrap();
    story.start_trace_from_here().unwrap();
    let checkpoint = story.replay_trace();
    let compared = compare(&result, &checkpoint, &checkpoint);
    assert_eq!(compared.left.status, RouteStatus::Replayed);
    assert!(!compared.left.complete && !compared.left.ended);
    assert!(compared.left.coverage.executed.visited_nodes.is_empty());
    assert!(!compared.left.coverage.inherited.visited_nodes.is_empty());
    assert_eq!(compared.left.state_actions.total_actions, 0);
    assert!(!compare(&result, &checkpoint, &a).alignment.comparable);
}
#[test]
fn evidence_omission_does_not_change_results_save_trace_or_rng() {
    let result = compile(SOURCE);
    let a = trace(&result, 0, 42);
    let full = compare(&result, &a, &a);
    let options = RouteComparisonOptions {
        max_evidence_records: 1,
        max_evidence_bytes: 1024,
        ..Default::default()
    };
    let limited = compare_routes(&result, &a, &a, options, &ReplayCancellation::new()).unwrap();
    assert_eq!(limited.left.status, full.left.status);
    assert_eq!(limited.left.states, full.left.states);
    assert_eq!(limited.left.vars, full.left.vars);
    assert_eq!(limited.left.coverage, full.left.coverage);
    assert!(limited.left.state_actions.omitted);
    assert_eq!(limited.left.state_actions.total_actions, 3);
    assert_eq!(limited.left.state_actions.records.len(), 1);
}
#[test]
fn cooperative_matches_sync_and_cancels_both_sides_with_global_budget() {
    let result = compile(SOURCE);
    let (a, b) = (trace(&result, 0, 42), trace(&result, 1, 42));
    let expected = compare(&result, &a, &b);
    let mut session = RouteComparisonSession::new(
        &result,
        a.clone(),
        b.clone(),
        Default::default(),
        ReplayCancellation::new(),
    )
    .unwrap();
    let actual = loop {
        if let Some(value) = session.advance(&result, ReplayBudget::new(1, 100)).unwrap() {
            break value;
        }
    };
    assert_eq!(actual, expected);
    let options = RouteComparisonOptions {
        budget: ReplayBudget::new(2, 30000),
        ..Default::default()
    };
    let limited = compare_routes(&result, &a, &b, options, &ReplayCancellation::new()).unwrap();
    assert_eq!(limited.left.status, RouteStatus::StepBudgetExceeded);
    assert_eq!(limited.right.status, RouteStatus::StepBudgetExceeded);
    assert!(limited.left.executed_steps + limited.right.executed_steps <= 2);
    let cancel = ReplayCancellation::new();
    let mut session = RouteComparisonSession::new(
        &result,
        a.clone(),
        b.clone(),
        Default::default(),
        cancel.clone(),
    )
    .unwrap();
    assert!(session
        .advance(&result, ReplayBudget::new(1, 100))
        .unwrap()
        .is_none());
    cancel.cancel();
    let stopped = session
        .advance(&result, ReplayBudget::new(100, 100))
        .unwrap()
        .unwrap();
    assert_eq!(stopped.left.status, RouteStatus::Cancelled);
    assert_eq!(stopped.right.status, RouteStatus::Cancelled);
    let precancel = compare_routes(&result, &a, &b, Default::default(), &cancel).unwrap();
    assert!(precancel.left.states.is_none() && precancel.right.states.is_none());
}
#[test]
fn zero_budgets_are_not_unlimited_and_initial_enter_is_truthfully_recorded() {
    let result = compile(SOURCE);
    let a = trace(&result, 0, 42);
    for (budget, status) in [
        (ReplayBudget::new(0, 30000), RouteStatus::StepBudgetExceeded),
        (
            ReplayBudget::new(100000, 0),
            RouteStatus::TimeBudgetExceeded,
        ),
    ] {
        let compared = compare_routes(
            &result,
            &a,
            &a,
            RouteComparisonOptions {
                budget,
                ..Default::default()
            },
            &ReplayCancellation::new(),
        )
        .unwrap();
        assert_eq!(compared.left.status, status);
        assert_eq!(compared.right.status, status);
        assert_eq!(
            compared.left.executed_steps + compared.right.executed_steps,
            0
        );
        assert_eq!(compared.left.state_actions.total_actions, 1);
    }
}
#[test]
fn input_versions_output_limits_and_changed_snapshot_are_explicit_errors() {
    let result = compile(SOURCE);
    let a = trace(&result, 0, 42);
    let mut old = a.clone();
    old.runtime_version = "0.19.0".into();
    assert_eq!(
        compare_routes(
            &result,
            &a,
            &old,
            Default::default(),
            &ReplayCancellation::new()
        )
        .unwrap_err()
        .code,
        "invalid_trace"
    );
    let tiny = RouteComparisonOptions {
        max_output_bytes: 32,
        ..Default::default()
    };
    assert_eq!(
        compare_routes(&result, &a, &a, tiny, &ReplayCancellation::new())
            .unwrap_err()
            .code,
        "output_limit"
    );
    let tiny = RouteComparisonOptions {
        max_trace_steps: 0,
        ..Default::default()
    };
    assert_eq!(
        compare_routes(&result, &a, &a, tiny, &ReplayCancellation::new())
            .unwrap_err()
            .code,
        "input_limit"
    );
    let mut session = RouteComparisonSession::new(
        &result,
        a.clone(),
        a.clone(),
        Default::default(),
        ReplayCancellation::new(),
    )
    .unwrap();
    let moved = compile(&format!("// shift\n{SOURCE}"));
    assert_eq!(moved.analysis.fingerprint, result.analysis.fingerprint);
    assert_eq!(
        session
            .advance(&moved, ReplayBudget::new(2, 10))
            .unwrap_err()
            .code,
        "snapshot_changed"
    );
    let mut session = RouteComparisonSession::new(
        &result,
        a.clone(),
        a,
        Default::default(),
        ReplayCancellation::new(),
    )
    .unwrap();
    let option_changed =
        compile_source_with_options("comparison.wl", SOURCE, CompileOptions::v1_12());
    assert_eq!(
        session
            .advance(&option_changed, ReplayBudget::new(2, 10))
            .unwrap_err()
            .code,
        "snapshot_changed"
    );
}
#[test]
fn moved_comments_use_current_action_and_choice_sources() {
    let original = compile(SOURCE);
    let (a, b) = (trace(&original, 0, 42), trace(&original, 1, 42));
    let moved = compile(&format!("// shift\n\n{SOURCE}"));
    let result = compare(&moved, &a, &b);
    assert_eq!(result.left.status, RouteStatus::Replayed);
    assert_eq!(
        result.alignment.first_difference.unwrap().left.choice.line,
        a.steps[0].choice.line + 2
    );
    assert_eq!(
        result.left.state_actions.records[1]
            .source
            .as_ref()
            .unwrap()
            .line,
        13
    );
}

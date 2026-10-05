//! 分片、失败、来源与共享配额只解释已执行动作，不把缺失证据冒充没有写入。
use std::collections::BTreeMap;
use worldline_core::evidence_source::{
    resolve_evidence_source, resolve_evidence_sources, EvidenceSourceOwner, VariableWriteOperation,
};
use worldline_core::{
    compile_source_with_options, compile_sources_with_options, CompileOptions, CompileResult,
};
use worldline_runtime::{
    compare_routes, ReplayBudget, ReplayCancellation, ReplayTrace, RouteComparisonOptions,
    RouteComparisonResult, RouteComparisonSession, RouteStatus, Story, Value,
};

fn compile(source: &str) -> CompileResult {
    let result = compile_source_with_options("limits.wl", source, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
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
fn compare(
    snapshot: &CompileResult,
    trace: &ReplayTrace,
    options: RouteComparisonOptions,
) -> RouteComparisonResult {
    compare_routes(snapshot, trace, trace, options, &ReplayCancellation::new()).unwrap()
}
const MIXED: &str = "tag a\nworld w\nstate s on world w with []\nlet n = 0\nevent start\n  become s add a\n  set n = 1\n  become s remove a\n  set n = 2\n  become s add a\n  set n = 3\n  -> END\n";

#[test]
fn single_statement_slices_match_sync_without_recounting_restored_globals() {
    let snapshot = compile(MIXED);
    let trace = record(&snapshot, &[]);
    for options in [
        RouteComparisonOptions::default(),
        RouteComparisonOptions {
            max_evidence_records: 3,
            ..Default::default()
        },
    ] {
        let expected = compare(&snapshot, &trace, options);
        let mut session = RouteComparisonSession::new(
            &snapshot,
            trace.clone(),
            trace.clone(),
            options,
            ReplayCancellation::new(),
        )
        .unwrap();
        let mut slices = 0;
        let actual = loop {
            slices += 1;
            assert!(slices < 100, "分片必须有界推进");
            if let Some(result) = session
                .advance(&snapshot, ReplayBudget::new(1, 1000))
                .unwrap()
            {
                break result;
            }
        };
        assert!(slices > 2);
        assert_eq!(actual, expected);
        assert_eq!(actual.left.variable_writes.total_writes, 3);
        assert_eq!(actual.left.state_actions.total_actions, 3);
    }
}

#[test]
fn shared_record_budget_retains_interleaved_prefix_and_separate_sequences() {
    let snapshot = compile(MIXED);
    let trace = record(&snapshot, &[]);
    let result = compare(
        &snapshot,
        &trace,
        RouteComparisonOptions {
            max_evidence_records: 3,
            ..Default::default()
        },
    );
    assert!(result.omitted);
    for side in [&result.left, &result.right] {
        assert!(side.complete && side.omitted);
        assert!(side.variable_writes.captured && side.variable_writes.omitted);
        assert!(side.state_actions.omitted);
        assert_eq!(side.state_actions.total_actions, 3);
        assert_eq!(side.variable_writes.total_writes, 3);
        assert_eq!(side.state_actions.records.len(), 2);
        assert_eq!(side.variable_writes.records.len(), 1);
        assert_eq!(side.state_actions.records[0].sequence, 1);
        assert_eq!(side.state_actions.records[1].sequence, 2);
        assert_eq!(side.variable_writes.records[0].sequence, 1);
        assert_eq!(side.variable_writes.records[0].after, Value::Num(1.0));
        assert_eq!(side.vars.as_ref().unwrap()["n"], Value::Num(3.0));
    }
}

#[test]
fn shared_byte_budget_charges_earlier_state_and_variable_records_together() {
    let snapshot = compile(MIXED);
    let trace = record(&snapshot, &[]);
    let full = compare(&snapshot, &trace, Default::default());
    let exact = serde_json::to_vec(&full.left.state_actions.records[0])
        .unwrap()
        .len()
        + serde_json::to_vec(&full.left.variable_writes.records[0])
            .unwrap()
            .len();
    let result = compare(
        &snapshot,
        &trace,
        RouteComparisonOptions {
            max_evidence_bytes: exact,
            ..Default::default()
        },
    );
    for side in [&result.left, &result.right] {
        assert_eq!(side.state_actions.records.len(), 1);
        assert_eq!(side.variable_writes.records.len(), 1);
        assert!(side.state_actions.omitted && side.variable_writes.omitted && side.omitted);
        assert_eq!(side.variable_writes.total_writes, 3);
        assert_eq!(side.state_actions.total_actions, 3);
        assert_eq!(side.vars, full.left.vars);
    }
}

#[test]
fn zero_evidence_limits_still_count_both_categories_without_changing_outcome() {
    let snapshot = compile(MIXED);
    let trace = record(&snapshot, &[]);
    for (records, bytes) in [(0, 65536), (256, 0), (0, 0)] {
        let result = compare(
            &snapshot,
            &trace,
            RouteComparisonOptions {
                max_evidence_records: records,
                max_evidence_bytes: bytes,
                ..Default::default()
            },
        );
        for side in [&result.left, &result.right] {
            assert_eq!(side.status, RouteStatus::Replayed);
            assert!(side.complete && side.variable_writes.captured && side.omitted);
            assert_eq!(side.variable_writes.total_writes, 3);
            assert_eq!(side.state_actions.total_actions, 3);
            assert!(
                side.variable_writes.records.is_empty() && side.state_actions.records.is_empty()
            );
            assert!(side.variable_writes.omitted && side.state_actions.omitted);
        }
    }
}

#[test]
fn combined_default_ceiling_is_256_records_and_64_kib_not_per_category() {
    let payload = "x".repeat(1000);
    let mut source =
        String::from("tag a\nworld w\nstate s on world w with []\nlet n = \"\"\nevent start\n");
    for _ in 0..300 {
        source += &format!("  become s add a\n  set n = \"{payload}\"\n");
    }
    source += "  -> END\n";
    let snapshot = compile(&source);
    let trace = record(&snapshot, &[]);
    let result = compare(&snapshot, &trace, Default::default());
    for side in [&result.left, &result.right] {
        assert!(side.complete && side.omitted);
        assert_eq!(side.state_actions.total_actions, 300);
        assert_eq!(side.variable_writes.total_writes, 300);
        let retained = side.state_actions.records.len() + side.variable_writes.records.len();
        assert!(retained < 256, "1000字节值应先触及共享字节预算");
        let bytes = side
            .state_actions
            .records
            .iter()
            .map(|r| serde_json::to_vec(r).unwrap().len())
            .sum::<usize>()
            + side
                .variable_writes
                .records
                .iter()
                .map(|r| serde_json::to_vec(r).unwrap().len())
                .sum::<usize>();
        assert!(bytes <= 64 * 1024);
        assert!(side.state_actions.omitted && side.variable_writes.omitted);
        assert_eq!(
            side.vars.as_ref().unwrap()["n"],
            Value::Str(payload.clone())
        );
    }
    for options in [
        RouteComparisonOptions {
            max_evidence_records: 257,
            ..Default::default()
        },
        RouteComparisonOptions {
            max_evidence_bytes: 65537,
            ..Default::default()
        },
    ] {
        assert_eq!(
            compare_routes(
                &snapshot,
                &trace,
                &trace,
                options,
                &ReplayCancellation::new()
            )
            .unwrap_err()
            .code,
            "invalid_options"
        );
    }
}

#[test]
fn oversized_before_and_after_values_are_omitted_but_later_small_writes_survive() {
    let giant = "界".repeat(100_000);
    let snapshot = compile(&format!("let text = \"\"\nlet n = 0\nevent start\n  set text = \"{giant}\"\n  set text = \"\"\n  set n = 1\n  -> END\n"));
    let trace = record(&snapshot, &[]);
    let result = compare(&snapshot, &trace, Default::default());
    for side in [&result.left, &result.right] {
        assert_eq!(side.status, RouteStatus::Replayed);
        assert!(side.variable_writes.captured && side.variable_writes.omitted);
        assert_eq!(side.variable_writes.total_writes, 3);
        assert_eq!(side.variable_writes.records.len(), 1);
        assert_eq!(side.variable_writes.records[0].sequence, 3);
        assert_eq!(side.variable_writes.records[0].variable, "n");
        assert_eq!(
            side.vars.as_ref().unwrap()["text"],
            Value::Str(String::new())
        );
    }
    assert!(serde_json::to_vec(&result).unwrap().len() < 1024 * 1024);
}

#[test]
fn cancelled_before_creation_is_uncaptured_but_started_sides_retain_real_prefix() {
    let snapshot =
        compile("let n = 0\nevent start\n  set n = 1\n  set n = 2\n  set n = 3\n  -> END\n");
    let trace = record(&snapshot, &[]);
    let cancel = ReplayCancellation::new();
    let mut session = RouteComparisonSession::new(
        &snapshot,
        trace.clone(),
        trace.clone(),
        Default::default(),
        cancel.clone(),
    )
    .unwrap();
    for _ in 0..2 {
        assert!(session
            .advance(&snapshot, ReplayBudget::new(1, 1000))
            .unwrap()
            .is_none());
    }
    cancel.cancel();
    let result = session
        .advance(&snapshot, ReplayBudget::new(100, 1000))
        .unwrap()
        .unwrap();
    for side in [&result.left, &result.right] {
        assert_eq!(side.status, RouteStatus::Cancelled);
        assert!(!side.complete);
        assert!(side.variable_writes.captured);
        assert_eq!(side.variable_writes.total_writes, 1);
        assert_eq!(side.variable_writes.records[0].after, Value::Num(1.0));
    }
    let stopped = compare_routes(&snapshot, &trace, &trace, Default::default(), &cancel).unwrap();
    for side in [&stopped.left, &stopped.right] {
        assert_eq!(side.status, RouteStatus::Cancelled);
        assert!(!side.variable_writes.captured);
        assert_eq!(side.variable_writes.total_writes, 0);
        assert!(side.variable_writes.records.is_empty());
    }
}

#[test]
fn step_and_time_budget_stops_never_invent_writes() {
    let snapshot =
        compile("let n = 0\nevent start\n  set n = 1\n  set n = 2\n  set n = 3\n  -> END\n");
    let trace = record(&snapshot, &[]);
    let result = compare(
        &snapshot,
        &trace,
        RouteComparisonOptions {
            budget: ReplayBudget::new(2, 30000),
            ..Default::default()
        },
    );
    assert_eq!(
        result.left.variable_writes.total_writes + result.right.variable_writes.total_writes,
        2
    );
    for side in [&result.left, &result.right] {
        assert_eq!(side.status, RouteStatus::StepBudgetExceeded);
        assert!(!side.complete);
        assert!(side.variable_writes.captured);
        assert_eq!(
            side.variable_writes.total_writes as usize,
            side.variable_writes.records.len()
        );
        assert!(side
            .variable_writes
            .records
            .iter()
            .all(|r| r.after != Value::Num(3.0)));
    }
    for (budget, status) in [
        (ReplayBudget::new(0, 30000), RouteStatus::StepBudgetExceeded),
        (ReplayBudget::new(1000, 0), RouteStatus::TimeBudgetExceeded),
    ] {
        let result = compare(
            &snapshot,
            &trace,
            RouteComparisonOptions {
                budget,
                ..Default::default()
            },
        );
        for side in [&result.left, &result.right] {
            assert_eq!(side.status, status);
            assert!(side.variable_writes.captured);
            assert_eq!(side.variable_writes.total_writes, 0);
            assert!(side.variable_writes.records.is_empty());
        }
    }
}

#[test]
fn failed_set_or_initializer_retains_only_earlier_successes() {
    for (old, changed) in [
        ("let n = 0\nevent start\n  set n = 1\n  set n = 2\n  -> END\n",
         "let n = 0\nevent start\n  set n = 1\n  set n = 2 / 0\n  -> END\n"),
        ("let n = 0\nevent start\n  set n = 1\n  let later = 2\n  -> END\n",
         "let n = 0\nevent start\n  set n = 1\n  let later = 2 / 0\n  -> END\n"),
        ("let n = 0\nevent start\n  set n = 1\n  if false\n    let missing = 2\n  set n = 2\n  -> END\n",
         "let n = 0\nevent start\n  set n = 1\n  if false\n    let missing = 2\n  set missing = 2\n  -> END\n"),
    ] {
        let original = compile(old);
        let trace = record(&original, &[]);
        let snapshot = compile(changed);
        let result = compare(&snapshot, &trace, Default::default());
        for side in [&result.left, &result.right] {
            assert_eq!(side.status, RouteStatus::StoryFailed);
            assert!(!side.complete);
            assert!(side.variable_writes.captured);
            assert_eq!(side.variable_writes.total_writes, 1);
            assert_eq!(side.variable_writes.records.len(), 1);
            assert_eq!(side.variable_writes.records[0].after, Value::Num(1.0));
            assert_eq!(side.vars.as_ref().unwrap()["n"], Value::Num(1.0));
        }
    }
}

#[test]
fn failed_startup_has_no_capture_and_divergence_keeps_actual_current_writes() {
    let original =
        compile("let divisor = 1\nlet n = 1 / divisor\nevent start\n  set n = 2\n  -> END\n");
    let trace = record(&original, &[]);
    let failed =
        compile("let divisor = 0\nlet n = 1 / divisor\nevent start\n  set n = 2\n  -> END\n");
    let result = compare(&failed, &trace, Default::default());
    assert_eq!(result.left.status, RouteStatus::StoryFailed);
    assert!(!result.left.variable_writes.captured);
    assert_eq!(result.left.variable_writes.total_writes, 0);
    let changed =
        compile("let divisor = 1\nlet n = 1 / divisor\nevent start\n  set n = 3\n  -> END\n");
    let result = compare(&changed, &trace, Default::default());
    assert_eq!(result.left.status, RouteStatus::Diverged);
    assert!(!result.left.complete);
    assert_eq!(result.left.variable_writes.total_writes, 1);
    assert_eq!(
        result.left.variable_writes.records[0].before,
        Some(Value::Num(1.0))
    );
    assert_eq!(
        result.left.variable_writes.records[0].after,
        Value::Num(3.0)
    );
}

#[test]
fn missing_choice_observation_does_not_execute_its_future_write() {
    let snapshot = compile(
        "let n = 0\nevent start\n  set n = 1\n  choice \"继续\"\n    set n = 2\n    -> END\n",
    );
    let mut trace = record(&snapshot, &[0]);
    trace.steps[0].observation = None;
    let result = compare(&snapshot, &trace, Default::default());
    assert_eq!(result.left.status, RouteStatus::IncompleteTrace);
    assert!(!result.left.complete);
    assert_eq!(result.left.variable_writes.total_writes, 1);
    assert_eq!(
        result.left.variable_writes.records[0].after,
        Value::Num(1.0)
    );
}

#[test]
fn output_budget_stop_keeps_prefix_before_the_actual_output_boundary() {
    let payload = "x".repeat(600_000);
    let source = format!("let n = 0\nfragment flood()\n  local payload: str = \"{payload}\"\n  set n = 1\n  {{payload}}\n  {{payload}}\n  set n = 2\n  return\nevent start\n  call flood()\n  -> END\n");
    let snapshot = compile(&source);
    let trace = record(&snapshot, &[]);
    let result = compare(&snapshot, &trace, Default::default());
    assert_eq!(result.left.status, RouteStatus::OutputBudgetExceeded);
    assert!(!result.left.complete);
    assert_eq!(result.left.variable_writes.total_writes, 1);
    assert_eq!(
        result.left.variable_writes.records[0].after,
        Value::Num(1.0)
    );
    assert_eq!(result.left.vars.as_ref().unwrap()["n"], Value::Num(1.0));
    assert!(result
        .right
        .variable_writes
        .records
        .iter()
        .all(|r| r.after == Value::Num(1.0)));
}

#[test]
fn included_fragments_same_lines_nested_scene_and_forged_source_identity() {
    let root = std::env::temp_dir().join(format!("write-evidence-{}", std::process::id()));
    let sources = BTreeMap::from([
        (root.join("world.wl"), "let n = 0\ninclude \"a.wl\"\ninclude \"b.wl\"\nevent start\n  scene outer\n    scene inner\n      if true\n        set n = 1\n      call first()\n      call second()\n      -> END\n".into()),
        (root.join("a.wl"), "fragment first()\n  set n = 2\n  return\n".into()),
        (root.join("b.wl"), "fragment second()\n  set n = 3\n  return\n".into()),
    ]);
    let snapshot =
        compile_sources_with_options(&root.join("world.wl"), &sources, CompileOptions::v1_13());
    assert!(!snapshot.has_errors(), "{:?}", snapshot.diagnostics);
    let trace = record(&snapshot, &[]);
    let result = compare(&snapshot, &trace, Default::default());
    let records = &result.left.variable_writes.records;
    assert_eq!(records.len(), 3);
    let sources = records
        .iter()
        .map(|r| r.source.as_ref().unwrap())
        .collect::<Vec<_>>();
    let resolved = resolve_evidence_sources(&snapshot, &sources).unwrap();
    for (index, target) in resolved.into_iter().enumerate() {
        let target = target.unwrap();
        assert_eq!(
            &snapshot.sources[&target.path][target.range],
            format!("set n = {}", index + 1)
        );
    }
    assert_eq!(records[0].node.as_deref(), Some("start.outer.inner"));
    assert_eq!(sources[0].line, 8);
    assert_eq!(sources[1].line, 2);
    assert_eq!(sources[2].line, 2);
    assert_ne!(sources[1].file, sources[2].file);
    for mutation in 0..6 {
        let mut forged = (*sources[1]).clone();
        match mutation {
            0 => forged.file = sources[2].file.clone(),
            1 => forged.line = 3,
            2 => forged.file = "../outside.wl".into(),
            3 => {
                forged.owner = EvidenceSourceOwner::VariableWrite {
                    node: "start.outer.inner".into(),
                    variable: "n".into(),
                    operation: VariableWriteOperation::Set,
                }
            }
            4 => {
                forged.owner = EvidenceSourceOwner::VariableWrite {
                    node: "fragment:first".into(),
                    variable: "different".into(),
                    operation: VariableWriteOperation::Set,
                }
            }
            _ => {
                forged.owner = EvidenceSourceOwner::VariableWrite {
                    node: "fragment:first".into(),
                    variable: "n".into(),
                    operation: VariableWriteOperation::Let,
                }
            }
        }
        assert!(
            resolve_evidence_source(&snapshot, &forged).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn same_fingerprint_comment_move_uses_current_source_and_rejects_old_line() {
    let source = "let n = 0\nevent start\n  set n = 1\n  -> END\n";
    let original = compile(source);
    let trace = record(&original, &[]);
    let old = compare(&original, &trace, Default::default());
    let moved = compile(&format!("// 位移🙂\n{source}"));
    assert_eq!(original.analysis.fingerprint, moved.analysis.fingerprint);
    let current = compare(&moved, &trace, Default::default());
    assert_eq!(current.left.status, RouteStatus::Replayed);
    let old_source = old.left.variable_writes.records[0].source.as_ref().unwrap();
    let new_source = current.left.variable_writes.records[0]
        .source
        .as_ref()
        .unwrap();
    assert_eq!(new_source.line, old_source.line + 1);
    assert!(resolve_evidence_source(&moved, old_source).is_err());
    assert!(resolve_evidence_source(&moved, new_source).is_ok());
}

#[test]
fn oversized_source_path_omits_record_without_losing_real_write_count() {
    let path = format!("{}.wl", "x".repeat(3000));
    let snapshot = compile_source_with_options(
        &path,
        "event start\n  let n = 1\n  -> END\n",
        CompileOptions::v1_13(),
    );
    assert!(!snapshot.has_errors(), "{:?}", snapshot.diagnostics);
    let trace = record(&snapshot, &[]);
    let result = compare(&snapshot, &trace, Default::default());
    for side in [&result.left, &result.right] {
        assert_eq!(side.status, RouteStatus::Replayed);
        assert!(side.complete && side.variable_writes.captured && side.variable_writes.omitted);
        assert_eq!(side.variable_writes.total_writes, 1);
        assert!(side.variable_writes.records.is_empty());
        assert_eq!(side.vars.as_ref().unwrap()["n"], Value::Num(1.0));
    }
}

#[test]
fn failing_right_route_does_not_drop_left_result_or_right_successful_prefix() {
    let source = "let n = 0\nevent start\n  set n = 1\n  choice \"安全\"\n    set n = 10\n    -> END\n  choice \"风险\"\n    set n = 20\n    set n = 30\n    -> END\n";
    let original = compile(source);
    let (a, b) = (record(&original, &[0]), record(&original, &[1]));
    let changed = compile(&source.replace("set n = 30", "set n = 30 / 0"));
    let result = compare_routes(
        &changed,
        &a,
        &b,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(result.left.status, RouteStatus::Replayed);
    assert_eq!(result.right.status, RouteStatus::StoryFailed);
    assert!(result.left.complete && !result.right.complete);
    assert_eq!(result.left.variable_writes.total_writes, 2);
    assert_eq!(result.right.variable_writes.total_writes, 2);
    assert_eq!(
        result.left.variable_writes.records[1].after,
        Value::Num(10.0)
    );
    assert_eq!(
        result.right.variable_writes.records[1].after,
        Value::Num(20.0)
    );
    assert_eq!(
        result.left.variable_writes.records[0],
        result.right.variable_writes.records[0]
    );
    assert_eq!(result.variable_differences.len(), 1);
    assert_eq!(result.variable_differences[0].id, "n");
}

//! 真实全局写入证据：旁录成功动作，不回填初始化历史或改变运行语义。
use worldline_core::evidence_source::{
    resolve_evidence_source, EvidenceSourceOwner, VariableWriteOperation,
};
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    compare_routes, ReplayCancellation, ReplayTrace, RouteComparisonOptions, RouteComparisonResult,
    RouteStatus, Story, Value, VariableWriteEvidence, VariableWriteRecord,
};

fn compile(source: &str) -> CompileResult {
    let result = compile_source_with_options("writes.wl", source, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn record(snapshot: &CompileResult, seed: u64, choices: &[usize]) -> ReplayTrace {
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, seed).unwrap();
    story.continue_story().unwrap();
    for choice in choices {
        story.choose(*choice).unwrap();
        story.continue_story().unwrap();
    }
    story.replay_trace()
}
fn compare(snapshot: &CompileResult, a: &ReplayTrace, b: &ReplayTrace) -> RouteComparisonResult {
    compare_routes(
        snapshot,
        a,
        b,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap()
}
fn assert_source(snapshot: &CompileResult, record: &VariableWriteRecord) {
    let source = record.source.as_ref().expect("真实语句应有来源");
    assert_eq!(
        source.owner,
        EvidenceSourceOwner::VariableWrite {
            node: record.node.clone().unwrap(),
            variable: record.variable.clone(),
            operation: record.operation,
        }
    );
    let target = resolve_evidence_source(snapshot, source).unwrap();
    let keyword = match record.operation {
        VariableWriteOperation::Let => "let ",
        VariableWriteOperation::Const => "const ",
        VariableWriteOperation::Set => "set ",
    };
    assert!(snapshot.sources[&target.path][target.range].starts_with(keyword));
}

#[test]
fn uninitialized_zero_false_empty_and_same_value_writes_remain_distinct() {
    let snapshot = compile(
        r#"let top = 4
event start
  let zero = 0
  let flag = false
  let empty = ""
  const fixed = 8
  set zero = 0
  set flag = false
  set empty = ""
  set top = 4
  if false
    let absent = 99
    set top = 999
  choice "执行"
    set top = 5
    -> END
  choice "未选"
    set top = 123
    -> END
"#,
    );
    let trace = record(&snapshot, 42, &[0]);
    let compared = compare(&snapshot, &trace, &trace);
    let evidence: &VariableWriteEvidence = &compared.left.variable_writes;
    assert!(evidence.captured && !evidence.omitted);
    assert_eq!(evidence.total_writes, 9);
    assert!(compared.left.complete && compared.right.complete);
    assert_eq!(
        compared.left.variable_writes,
        compared.right.variable_writes
    );
    let records = &evidence.records;
    assert_eq!(
        records
            .iter()
            .map(|r| r.variable.as_str())
            .collect::<Vec<_>>(),
        vec!["zero", "flag", "empty", "fixed", "zero", "flag", "empty", "top", "top"]
    );
    assert!(records[..4].iter().all(|r| r.before.is_none()));
    for (index, value) in [
        Value::Num(0.0),
        Value::Bool(false),
        Value::Str(String::new()),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(records[index].after, value);
        assert_eq!(records[index + 4].before.as_ref(), Some(&value));
        assert_eq!(records[index + 4].after, value);
    }
    assert_eq!(records[3].operation, VariableWriteOperation::Const);
    assert_eq!(records[7].before, Some(Value::Num(4.0)));
    assert_eq!(records[8].after, Value::Num(5.0));
    assert_eq!(records[8].turn, 1);
    for (index, entry) in records.iter().enumerate() {
        assert_eq!(entry.sequence, index as u64 + 1);
        assert_eq!(entry.event.as_deref(), Some("start"));
        assert_eq!(entry.node.as_deref(), Some("start"));
        assert_source(&snapshot, entry);
    }
    assert!(!compared.left.vars.as_ref().unwrap().contains_key("absent"));
    assert_eq!(
        serde_json::to_value(&records[0]).unwrap()["before"],
        serde_json::Value::Null
    );
    assert_eq!(
        serde_json::to_value(&records[4]).unwrap()["operation"],
        "set"
    );
}

#[test]
fn repeated_let_records_each_write_and_const_reentry_records_nothing() {
    let snapshot = compile(
        "event start\n  let rolling = 1\n  const token = rnd(1, 1000)\n  choice \"再次\"\n    set rolling = 2\n    -> start\n  choice \"结束\"\n    -> END\n",
    );
    let trace = record(&snapshot, 31, &[0, 0, 1]);
    let result = compare(&snapshot, &trace, &trace);
    assert_eq!(result.left.status, RouteStatus::Replayed);
    let records = &result.left.variable_writes.records;
    assert_eq!(result.left.variable_writes.total_writes, 6);
    assert_eq!(records.len(), 6);
    assert_eq!(
        records.iter().map(|r| r.operation).collect::<Vec<_>>(),
        vec![
            VariableWriteOperation::Let,
            VariableWriteOperation::Const,
            VariableWriteOperation::Set,
            VariableWriteOperation::Let,
            VariableWriteOperation::Set,
            VariableWriteOperation::Let
        ]
    );
    assert_eq!(records[0].before, None);
    for index in [3, 5] {
        assert_eq!(records[index].before, Some(Value::Num(2.0)));
        assert_eq!(records[index].after, Value::Num(1.0));
        assert_eq!(records[index].source, records[0].source);
    }
    assert_eq!(records[1].before, None);
    assert_eq!(records[3].turn, 1);
    assert_eq!(records[5].turn, 2);
}

#[test]
fn fragment_parameters_and_locals_shadow_globals_without_becoming_global_writes() {
    let snapshot = compile(
        r#"let value = 90
let label = "全局"
let sink = 0
fragment apply(value: num)
  local label: str = "局部"
  local local_only: num = value + 1
  let fresh = value
  const first = local_only
  set sink = value
  {label}/{local_only}
  return
event start
  call apply(2)
  call apply(3)
  -> END
"#,
    );
    let trace = record(&snapshot, 42, &[]);
    let result = compare(&snapshot, &trace, &trace);
    assert_eq!(result.left.status, RouteStatus::Replayed);
    let records = &result.left.variable_writes.records;
    assert_eq!(records.len(), 5);
    assert_eq!(
        records
            .iter()
            .map(|r| r.variable.as_str())
            .collect::<Vec<_>>(),
        vec!["fresh", "first", "sink", "fresh", "sink"]
    );
    assert_eq!(records[0].after, Value::Num(2.0));
    assert_eq!(records[1].after, Value::Num(3.0));
    assert_eq!(records[3].before, Some(Value::Num(2.0)));
    assert_eq!(records[3].after, Value::Num(3.0));
    assert_eq!(records[4].before, Some(Value::Num(2.0)));
    for entry in records {
        assert_eq!(entry.node.as_deref(), Some("fragment:apply"));
        assert_eq!(entry.event.as_deref(), Some("start"));
        assert_source(&snapshot, entry);
    }
    let vars = result.left.vars.unwrap();
    assert_eq!(vars["value"], Value::Num(90.0));
    assert_eq!(vars["label"], Value::Str("全局".into()));
    assert!(!vars.contains_key("local_only"));
}

#[test]
fn checkpoint_origin_records_only_new_writes_with_restored_before_value() {
    let snapshot = compile(
        "let n = 5\nevent start\n  set n = 6\n  choice \"继续\"\n    set n = 7\n    -> END\n",
    );
    let entry = record(&snapshot, 42, &[0]);
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    story.continue_story().unwrap();
    story.start_trace_from_here().unwrap();
    let untouched = story.replay_trace();
    let result = compare(&snapshot, &untouched, &untouched);
    assert!(result.left.variable_writes.captured);
    assert_eq!(result.left.variable_writes.total_writes, 0);
    assert!(result.left.variable_writes.records.is_empty());
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let checkpoint = story.replay_trace();
    let result = compare(&snapshot, &entry, &checkpoint);
    assert!(!result.alignment.comparable);
    assert_eq!(result.left.variable_writes.total_writes, 2);
    assert_eq!(result.right.variable_writes.total_writes, 1);
    let write = &result.right.variable_writes.records[0];
    assert_eq!(write.sequence, 1);
    assert_eq!(write.before, Some(Value::Num(6.0)));
    assert_eq!(write.after, Value::Num(7.0));
    assert_eq!(write.turn, 1);
    assert_eq!(result.left.vars, result.right.vars);
}

#[test]
fn different_seeds_keep_independent_real_values_and_no_false_alignment() {
    let snapshot = compile("let n = 0\nevent start\n  set n = rnd(1, 1000000)\n  -> END\n");
    let (a, b) = (record(&snapshot, 31, &[]), record(&snapshot, 99, &[]));
    let result = compare(&snapshot, &a, &b);
    assert!(!result.alignment.comparable);
    assert_ne!(result.left.vars, result.right.vars);
    for (seed, side) in [(31, &result.left), (99, &result.right)] {
        let mut direct = Story::new_with_seed(&snapshot.program, &snapshot.analysis, seed).unwrap();
        direct.continue_story().unwrap();
        assert_eq!(side.status, RouteStatus::Replayed);
        assert_eq!(side.variable_writes.total_writes, 1);
        assert_eq!(&side.variable_writes.records[0].after, &direct.vars()["n"]);
    }
}

#[test]
fn evidence_read_and_omission_leave_live_save_trace_checkpoint_and_rng_unchanged() {
    let snapshot = compile(
        "let n = rnd(1, 1000)\nevent start\n  set n = rnd(1, 1000)\n  choice \"继续\"\n    set n = rnd(1, 1000)\n    -> END\n",
    );
    let trace = record(&snapshot, 31, &[0]);
    let mut live = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 31).unwrap();
    live.continue_story().unwrap();
    assert_eq!(
        live.variable_write_evidence(),
        &VariableWriteEvidence::default()
    );
    let before = (
        live.save().unwrap(),
        live.replay_trace(),
        live.checkpoint().unwrap(),
    );
    let full = compare(&snapshot, &trace, &trace);
    let limited = compare_routes(
        &snapshot,
        &trace,
        &trace,
        RouteComparisonOptions {
            max_evidence_records: 0,
            max_evidence_bytes: 0,
            ..Default::default()
        },
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert_eq!(full.left.status, RouteStatus::Replayed);
    assert_eq!(limited.left.status, full.left.status);
    assert_eq!(limited.left.vars, full.left.vars);
    assert_eq!(limited.left.coverage, full.left.coverage);
    for _ in 0..4 {
        serde_json::to_string(&full.left.variable_writes).unwrap();
        for record in &full.left.variable_writes.records {
            assert_source(&snapshot, record);
        }
    }
    assert_eq!(
        before,
        (
            live.save().unwrap(),
            live.replay_trace(),
            live.checkpoint().unwrap()
        )
    );
    for json in [
        &before.0,
        &serde_json::to_string(&before.1).unwrap(),
        &serde_json::to_string(&before.2).unwrap(),
    ] {
        assert!(!json.contains("variable_writes"));
        assert!(!json.contains("total_writes"));
        assert!(!json.contains("\"kind\":\"variable_write\""));
    }
    let mut control = Story::load(&snapshot.program, &snapshot.analysis, &before.0).unwrap();
    live.choose(0).unwrap();
    control.choose(0).unwrap();
    live.continue_story().unwrap();
    control.continue_story().unwrap();
    assert_eq!(live.vars(), control.vars());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&live.save().unwrap()).unwrap(),
        serde_json::from_str::<serde_json::Value>(&control.save().unwrap()).unwrap()
    );
}

#[test]
fn old_result_missing_field_defaults_to_not_captured_and_schema_stays_one() {
    let snapshot = compile("event start\n  let n = 1\n  -> END\n");
    let trace = record(&snapshot, 42, &[]);
    let result = compare(&snapshot, &trace, &trace);
    assert_eq!(result.schema_version, 1);
    let mut json = serde_json::to_value(&result).unwrap();
    for side in ["left", "right"] {
        json[side]
            .as_object_mut()
            .unwrap()
            .remove("variable_writes");
    }
    let old: RouteComparisonResult = serde_json::from_value(json).unwrap();
    for side in [&old.left, &old.right] {
        assert_eq!(side.variable_writes, VariableWriteEvidence::default());
        assert!(!side.variable_writes.captured);
        assert!(!side.variable_writes.omitted);
        assert_eq!(side.variable_writes.total_writes, 0);
        assert!(side.variable_writes.records.is_empty());
    }
    assert!(result.left.variable_writes.captured);
    for (operation, name) in [
        (VariableWriteOperation::Let, "let"),
        (VariableWriteOperation::Const, "const"),
        (VariableWriteOperation::Set, "set"),
    ] {
        assert_eq!(serde_json::to_value(operation).unwrap(), name);
    }
}

#[test]
fn typed_global_values_use_existing_value_serialization_and_real_before_values() {
    let snapshot = compile(
        "world w\ntag a\ntag b\nstate s on world w with a\nevent start\n  let selected = tag(a)\n  let held = tags(tag(b), tag(a), tag(a))\n  let target = state(s)\n  set selected = tag(b)\n  set held = tags()\n  set target = state(s)\n  -> END\n",
    );
    let trace = record(&snapshot, 42, &[]);
    let result = compare(&snapshot, &trace, &trace);
    let records = &result.left.variable_writes.records;
    assert_eq!(records.len(), 6);
    for (index, expected) in [
        Value::Tag("a".into()),
        Value::TagSet(vec!["a".into(), "b".into()]),
        Value::StateRef("s".into()),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(records[index].before, None);
        assert_eq!(records[index].after, expected);
        assert_eq!(records[index + 3].before, Some(expected));
    }
    assert_eq!(records[3].after, Value::Tag("b".into()));
    assert_eq!(records[4].after, Value::TagSet(vec![]));
    assert_eq!(records[5].after, Value::StateRef("s".into()));
    let roundtrip: VariableWriteEvidence =
        serde_json::from_value(serde_json::to_value(&result.left.variable_writes).unwrap())
            .unwrap();
    assert_eq!(roundtrip, result.left.variable_writes);
}

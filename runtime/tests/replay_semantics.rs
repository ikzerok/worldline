//! 定位归一化不能掩盖真正的调用、状态、输出、选择与随机变化。
use serde_json::{json, Value};
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    ReplayBudget, ReplayCancellation, ReplayResult, ReplayStatus, ReplayTrace, Story,
};

const SOURCE: &str = r#"world coast
tag pass
state inventory on world coast with []
let trust = 1
fragment gate(file: str)
  local line: num = 7
  choice "继续"
    return
fragment wrapper(file: str)
  call gate(file)
  return
fragment other(file: str)
  local line: num = 7
  choice "继续"
    return
event start
  call gate("地图")
  结束。
  -> END
"#;

fn compile(source: &str) -> CompileResult {
    let result = compile_source_with_options("story.wl", source, CompileOptions::v1_11());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

fn capture(result: &CompileResult) -> ReplayTrace {
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    assert!(story.is_ended());
    story.replay_trace()
}

fn replay(result: &CompileResult, trace: &ReplayTrace) -> ReplayResult {
    ReplayTrace::replay(
        &result.program,
        &result.analysis,
        trace,
        ReplayBudget::new(1_000, 5_000),
        &ReplayCancellation::new(),
    )
    .unwrap()
}

fn assert_diverged(result: &ReplayResult, step: usize) {
    assert!(
        matches!(result.status, ReplayStatus::Diverged { step_index, .. } if step_index == step),
        "{:?}",
        result.status
    );
    assert_eq!(result.completed_choices, step);
}

#[test]
fn real_parameter_local_global_state_fragment_and_stack_changes_still_diverge() {
    let original = compile(SOURCE);
    let trace = capture(&original);
    for (before, after, path, value) in [
        (
            "gate(\"地图\")",
            "gate(\"海图\")",
            "/calls/0/locals/file/Str",
            json!("海图"),
        ),
        ("= 7", "= 8", "/calls/0/locals/line/Num", json!(8.0)),
        ("trust = 1", "trust = 2", "/vars/trust/Num", json!(2.0)),
        ("with []", "with pass", "/states/inventory", json!(["pass"])),
        (
            "gate(\"地图\")",
            "other(\"地图\")",
            "/calls/0/fragment",
            json!("other"),
        ),
        (
            "gate(\"地图\")",
            "wrapper(\"地图\")",
            "/calls/1/caller",
            json!("fragment:wrapper"),
        ),
        (
            "  call gate(\"地图\")",
            "  set trust = trust\n  call gate(\"地图\")",
            "/calls/0/call_statement",
            json!(1),
        ),
        (
            "  local line: num = 7\n  choice",
            "  local line: num = 7\n  if false\n    不输出。\n  choice",
            "/calls/0/statement",
            json!(2),
        ),
    ] {
        let changed = compile(&SOURCE.replace(before, after));
        let result = replay(&changed, &trace);
        assert_diverged(&result, 0);
        assert_eq!(result.current_state.pointer(path), Some(&value), "{path}");
    }
}

#[test]
fn each_call_frame_field_remains_observable_independently_of_other_state() {
    let original = compile(&SOURCE.replace("gate(\"地图\")", "wrapper(\"地图\")"));
    let trace = capture(&original);
    for (path, value) in [
        ("/calls/0/fragment", json!("another")),
        ("/calls/0/caller", json!("another_event")),
        ("/calls/0/statement", json!(99)),
        ("/calls/0/call_statement", json!(99)),
        ("/calls/1/caller", json!("fragment:another")),
        ("/calls/1/locals/file/Str", json!("海图")),
        ("/calls/1/locals/line/Num", json!(99.0)),
    ] {
        let mut changed_trace = trace.clone();
        let state = &mut changed_trace.initial_observation.as_mut().unwrap().state;
        *state.pointer_mut(path).unwrap() = value;
        assert_diverged(&replay(&original, &changed_trace), 0);
    }
    let mut changed_trace = trace.clone();
    changed_trace.initial_observation.as_mut().unwrap().state["calls"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_diverged(&replay(&original, &changed_trace), 0);
    changed_trace.initial_observation.as_mut().unwrap().state["calls"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_diverged(&replay(&original, &changed_trace), 0);
}

#[test]
fn output_and_available_choice_changes_stop_at_the_first_changed_observation() {
    let original = compile(SOURCE);
    let trace = capture(&original);
    for (before, after, step) in [
        ("继续", "离开", 0),
        ("结束。", "改写结尾。", 1),
        ("  choice \"继续\"", "  开场。\n  choice \"继续\"", 0),
        (
            "  choice \"继续\"",
            "  choice \"另一条路\"\n    return\n  choice \"继续\"",
            0,
        ),
    ] {
        assert_diverged(
            &replay(&compile(&SOURCE.replace(before, after)), &trace),
            step,
        );
    }
}

#[test]
fn random_consumption_keeps_its_first_actual_output_difference_after_the_choice() {
    let source = SOURCE
        .replace("= 7", "= 1")
        .replace("结束。", "抽签:{rnd(1, 1000000)}。");
    let original = compile(&source);
    let changed = compile(&source.replace("= 1\n  choice", "= rnd(1, 1)\n  choice"));
    let trace = capture(&original);
    let actual = capture(&changed);
    // 单点抽样得到相同 local，但仍消费 RNG；暂停观察相同，下一次输出才有差异。
    assert_eq!(trace.initial_observation, actual.initial_observation);
    assert_ne!(trace.steps[0].observation, actual.steps[0].observation);
    assert_diverged(&replay(&changed, &trace), 1);
    let mut original_story =
        Story::new_with_seed(&original.program, &original.analysis, 31).unwrap();
    let mut changed_story = Story::new_with_seed(&changed.program, &changed.analysis, 31).unwrap();
    original_story.continue_story().unwrap();
    changed_story.continue_story().unwrap();
    let saved_rng = |story: &Story<'_>| {
        serde_json::from_str::<Value>(&story.save().unwrap()).unwrap()["rng"].clone()
    };
    assert_ne!(saved_rng(&original_story), saved_rng(&changed_story));
}

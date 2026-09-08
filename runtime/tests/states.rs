//! 状态执行、事件生命周期和存档兼容性回归。

use worldline_core::{compile_source, CompileResult};
use worldline_runtime::{AnchorKind, Output, StateRecord, Story, Value};

fn compile(body: &str) -> CompileResult {
    let source = format!(
        "tag calm\ntag alert\ncharacter lin\nstate mood on character lin with calm\n\
         state health on character lin with []\nstate proxy on tag calm with alert\n{body}"
    );
    let result = compile_source("states.wl", &source);
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    result
}

fn notes<'a>(story: &'a Story<'_>) -> Vec<&'a str> {
    story
        .state_history()
        .iter()
        .map(|record| record.note.as_deref().unwrap_or(""))
        .collect()
}

#[test]
fn initial_states_are_available_to_variables_admission_and_static_has_arguments() {
    let result = compile(
        r#"
let mood = "health"
let calm = "alert"
let initial = has(mood, calm)
event start after has("mood", calm) and not has(mood, "alert")
  查询:{has(mood, calm)}，{has("mood", "alert")}，{has(proxy, alert)}
  -> END
"#,
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    assert_eq!(story.states()["mood"], ["calm"]);
    assert!(story.states()["health"].is_empty());
    assert_eq!(story.vars()["initial"], Value::Bool(true));
    assert!(story.state_history().is_empty());
    let output = story.continue_story().unwrap();
    assert!(matches!(&output[0], Output::Text { content, .. }
        if content == "查询:true，false，true"));
}

#[test]
fn become_replaces_deduplicates_clears_and_records_explicit_same_value_actions() {
    let result = compile(
        r#"
event start
  scene room
    choice "改变"
      if has(mood, calm)
        become mood with alert, calm, alert as "替换"
        become mood with alert, calm as "同值"
        become mood with [] as "清空"
      -> END
"#,
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    assert!(story.states()["mood"].is_empty());
    assert!(story.states()["health"].is_empty());
    assert_eq!(story.states()["proxy"], ["alert"]);
    let history: &[StateRecord] = story.state_history();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].before, ["calm"]);
    assert_eq!(history[0].after, ["alert", "calm"]);
    assert_eq!(history[1].before, history[1].after);
    assert_eq!(history[2].before, ["alert", "calm"]);
    assert!(history[2].after.is_empty());
    assert_eq!(notes(&story), ["替换", "同值", "清空"]);
    for record in history {
        assert_eq!(record.state, "mood");
        assert_eq!(record.event.as_deref(), Some("start"));
        assert_eq!(record.node.as_deref(), Some("start.room"));
        assert_eq!(record.turn, 1);
    }
}

#[test]
fn natural_completion_runs_done_then_exit_in_order_with_current_conditions() {
    let result = compile(
        r#"
event start
  effect on exit if has(mood, alert)
    become mood with [] as "离开一"
  effect on exit if not has(mood, alert)
    become mood with calm as "离开二"
  effect on done
    become mood with alert as "完成"
  effect on enter
    become mood with [] as "进入"
  effect on exit if has(mood, alert)
    become mood with alert as "不执行"
  正文。
"#,
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    assert!(story.is_ended());
    assert_eq!(notes(&story), ["进入", "完成", "离开一", "离开二"]);
    assert_eq!(story.states()["mood"], ["calm"]);
    assert!(story.state_history().iter().all(|record| {
        record.event.as_deref() == Some("start") && record.node.as_deref() == Some("start")
    }));
    story.continue_story().unwrap();
    assert_eq!(story.state_history().len(), 4);
}

#[test]
fn cross_event_and_drift_run_source_exit_before_target_admission() {
    for arrow in ["->", "->>"] {
        let result = compile(&format!(
            r#"
event start
  effect on done
    become mood with [] as "不执行"
  effect on exit
    become mood with alert as "源离开"
  scene departure
    {arrow} next.phase
event next.phase after has(mood, alert)
  effect on enter
    become mood with [] as "目标进入"
  effect on exit
    become mood with calm as "目标离开"
  scene room
    choice "结束"
      -> END
"#
        ));
        let mut story = Story::new(&result.program, &result.analysis).unwrap();
        story.continue_story().unwrap();
        assert_eq!(notes(&story), ["源离开", "目标进入"]);
        assert_eq!(story.current_node().as_deref(), Some("next.phase.room"));
        assert_eq!(
            story.state_history()[0].node.as_deref(),
            Some("start.departure")
        );
        assert_eq!(
            story.state_history()[1].event.as_deref(),
            Some("next.phase")
        );
        if arrow == "->>" {
            let drift = story
                .anchors()
                .iter()
                .find(|a| a.kind == AnchorKind::Drift)
                .unwrap();
            assert_eq!(drift.node.as_deref(), Some("start.departure"));
        }
        story.choose(0).unwrap();
        story.continue_story().unwrap();
        assert_eq!(notes(&story), ["源离开", "目标进入", "目标离开"]);
        assert_eq!(
            story.state_history()[2].event.as_deref(),
            Some("next.phase")
        );
    }
}

#[test]
fn same_event_reentry_exits_and_readmits_but_end_never_runs_done() {
    let result = compile(
        r#"
event start after has(mood, calm)
  effect on enter
    become mood with alert as "进入"
  effect on exit
    become mood with calm as "离开"
  effect on done
    become mood with [] as "不执行"
  choice "重入" if turns() == 0
    -> start
  choice "结束" if turns() > 0
    -> END
"#,
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    assert_eq!(notes(&story), ["进入", "离开", "进入"]);
    assert_eq!(story.visits()["start"], 2);
    assert_eq!(story.choices()[0].label, "结束");
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    assert_eq!(notes(&story), ["进入", "离开", "进入", "离开"]);
    assert_eq!(story.state_history()[3].turn, 2);
}

#[test]
fn same_event_scene_jump_skips_exit_admission_and_enter() {
    let result = compile(
        r#"
event start after has(mood, calm)
  effect on enter
    become mood with alert as "进入"
  effect on exit
    become mood with calm as "离开"
  effect on done
    become mood with [] as "不执行"
  scene source
    become mood with [] as "正文"
    -> target
  scene target
    choice "结束"
      -> END
"#,
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    assert_eq!(notes(&story), ["进入", "正文"]);
    assert_eq!(story.visits()["start"], 1);
    assert_eq!(story.visits()["start.target"], 1);
    assert_eq!(story.current_node().as_deref(), Some("start.target"));
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    assert_eq!(notes(&story), ["进入", "正文", "离开"]);
}

#[test]
fn rejected_target_retains_source_exit_without_repeating_after_resume_or_load() {
    for gate in ["after has(mood, calm)", "perm key"] {
        let result = compile(&format!(
            r#"
event start
  effect on exit
    become mood with alert as "离开"
  -> target
event target {gate}
  effect on enter
    become mood with [] as "不执行"
  -> END
"#
        ));
        let mut story = Story::new(&result.program, &result.analysis).unwrap();
        let error = story.continue_story().unwrap_err();
        assert!(error.message.contains("无法进入节点"), "{error}");
        assert_eq!(error.node.as_deref(), Some("start"));
        assert_eq!(story.states()["mood"], ["alert"]);
        assert_eq!(notes(&story), ["离开"]);
        assert!(story.is_ended());
        assert!(!story.visits().contains_key("target"));
        let save = story.save().unwrap();
        let mut restored = Story::load(&result.program, &result.analysis, &save).unwrap();
        restored.continue_story().unwrap();
        story.continue_story().unwrap();
        assert_eq!(restored.state_history(), story.state_history());
        assert_eq!(notes(&story), ["离开"]);
    }
}

#[test]
fn save_load_and_restart_preserve_or_reset_states_and_history() {
    let result = compile(
        r#"
event start
  effect on exit
    become mood with [] as "离开"
  become mood with alert as "正文"
  choice "继续" if has(mood, alert)
    become health with calm as "选择"
    -> END
"#,
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    let save = story.save().unwrap();
    let mut restored = Story::load(&result.program, &result.analysis, &save).unwrap();
    assert_eq!(restored.states(), story.states());
    assert_eq!(restored.state_history(), story.state_history());
    restored.continue_story().unwrap();
    assert_eq!(restored.choices()[0].label, "继续");
    assert_eq!(notes(&restored), ["正文"]);
    for running in [&mut story, &mut restored] {
        running.choose(0).unwrap();
        running.continue_story().unwrap();
    }
    assert_eq!(restored.states(), story.states());
    assert_eq!(restored.state_history(), story.state_history());
    assert_eq!(notes(&restored), ["正文", "选择", "离开"]);
    let view = restored.state_view();
    assert_eq!(view["states"]["health"], serde_json::json!(["calm"]));
    assert_eq!(view["state_history"].as_array().unwrap().len(), 3);
    restored.restart().unwrap();
    assert_eq!(restored.states()["mood"], ["calm"]);
    assert!(restored.states()["health"].is_empty());
    assert!(restored.state_history().is_empty());
    assert_eq!(restored.turns(), 0);
}

#[test]
fn saves_without_state_fields_still_load_for_old_stories() {
    let result = compile_source("legacy.wl", "event start\n  choice \"结束\"\n    -> END\n");
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    story.continue_story().unwrap();
    let mut save: serde_json::Value = serde_json::from_str(&story.save().unwrap()).unwrap();
    save.as_object_mut().unwrap().remove("states");
    save.as_object_mut().unwrap().remove("state_history");
    let mut restored = Story::load(&result.program, &result.analysis, &save.to_string()).unwrap();
    assert!(restored.states().is_empty());
    assert!(restored.state_history().is_empty());
    restored.continue_story().unwrap();
    assert_eq!(restored.choices()[0].label, "结束");
    restored.choose(0).unwrap();
    restored.continue_story().unwrap();
    assert!(restored.is_ended());
}

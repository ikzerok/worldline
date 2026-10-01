use serde_json::{json, Value};
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    Output, ReplayBudget, ReplayCancellation, ReplayStatus, ReplayTrace, Story,
};

fn compile(source: &str) -> CompileResult {
    let result = compile_source_with_options("locked.wl", source, CompileOptions::v1_12());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn snapshot(story: &Story<'_>) -> Value {
    json!({"save":serde_json::from_str::<Value>(&story.save().unwrap()).unwrap(),
        "presentation":story.choice_presentations(),"trace":story.replay_trace()})
}
const SOURCE: &str = r#"
let key = false
event start
  choice "秘密" if false enable rnd(0, 1) == 1 disabled "隐藏"
    -> END
  choice once "档案室" enable key disabled "还缺银钥匙 {rnd(1, 99)}"
    -> start
  choice once "拿钥匙"
    set key = true
    -> start
  choice "离开"
    -> END
"#;

#[test]
fn mixed_choices_keep_legacy_indices_and_disabled_rejection_is_zero_progress() {
    let c = compile(SOURCE);
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 42).unwrap();
    s.continue_story().unwrap();
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.as_str())
            .collect::<Vec<_>>(),
        ["拿钥匙", "离开"]
    );
    assert_eq!(s.choice_presentations().len(), 3);
    let locked = &s.choice_presentations()[0];
    assert!(!locked.enabled);
    assert_eq!(locked.index, None);
    assert_eq!(
        locked.disabled_reason.as_deref(),
        Some("还缺银钥匙 {rnd(1, 99)}")
    );
    let id = locked.id.clone();
    let before = snapshot(&s);
    assert!(s.choose_presentation(0).is_err());
    assert!(s.choose_id(&id).is_err());
    assert!(s.choose_id("stale-id").is_err());
    assert!(s.choose_presentation(99).is_err());
    assert!(s.choose(99).is_err());
    for _ in 0..3 {
        let explanation = s.explain_choices().unwrap();
        assert_eq!(
            explanation[1].enable_condition.as_ref().unwrap().result,
            Some(false)
        );
        assert!(s.choice_evidence().is_some());
        let _ = s.choice_presentations();
    }
    assert_eq!(snapshot(&s), before);
    assert_eq!(before["save"]["rng"], 42, "隐藏条件及静态说明不能耗随机数");
    s.choose(0).unwrap();
    s.continue_story().unwrap();
    assert_eq!(s.choices()[0].label, "档案室");
    assert_eq!(s.choice_presentations()[0].id, id);
    assert!(s.choice_presentations()[0].disabled_reason.is_none());
    s.choose_id(&id).unwrap();
    s.continue_story().unwrap();
    assert_eq!(s.choice_presentations().len(), 1, "once在真正选取后隐藏");
    assert_eq!(s.choices()[0].label, "离开");
}

#[test]
fn all_disabled_group_falls_through_and_can_reach_later_choice_group() {
    let c = compile("event start\n  choice \"锁一\" enable false disabled \"提示一\"\n    -> END\n  choice \"锁二\" enable false disabled \"提示二\"\n    -> END\n  已落穿\n  choice \"继续\"\n    -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 12).unwrap();
    let outputs = s.continue_story().unwrap();
    assert!(outputs
        .iter()
        .any(|o| matches!(o, Output::Text {content,..} if content=="已落穿")));
    assert_eq!(s.choices().len(), 1);
    assert_eq!(s.choice_presentations()[0].label, "继续");
    assert_eq!(s.turns(), 0);
    s.choose(0).unwrap();
    s.continue_story().unwrap();
    assert!(s.is_ended());
    let c = compile(
        "event start\n  choice \"锁\" enable false disabled \"提示\"\n    不得输出\n  -> END\n",
    );
    let mut s = Story::new(&c.program, &c.analysis).unwrap();
    assert!(matches!(
        s.continue_story().unwrap().as_slice(),
        [Output::Ended]
    ));
    assert!(s.choice_presentations().is_empty());
}

#[test]
fn fragment_locals_pause_save_restore_and_replay_include_presentation() {
    let c = compile("fragment nested(open: bool)\n  local unlocked: bool = open\n  choice once \"进入\" enable unlocked disabled \"未开放\"\n    return\n  choice \"返回\"\n    return\nfragment outer(open: bool)\n  call nested(open)\n  返回外层\nevent start\n  call outer(false)\n  call outer(true)\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 52).unwrap();
    s.continue_story().unwrap();
    let save = s.save().unwrap();
    assert!(save.contains("runtime.choice_presentation.v1"));
    let mut restored = Story::load(&c.program, &c.analysis, &save).unwrap();
    assert_eq!(
        json!(s.choice_presentations()),
        json!(restored.choice_presentations())
    );
    for story in [&mut s, &mut restored] {
        story.choose_presentation(1).unwrap();
        story.continue_story().unwrap();
        assert!(story.choice_presentations()[0].enabled);
        story.choose(0).unwrap();
        story.continue_story().unwrap();
        assert!(story.is_ended());
    }
    assert_eq!(json!(s.state_view()), json!(restored.state_view()));
    let trace = s.replay_trace();
    assert_eq!(
        trace
            .initial_observation
            .as_ref()
            .unwrap()
            .choice_presentation
            .len(),
        2
    );
    let replay = ReplayTrace::replay(
        &c.program,
        &c.analysis,
        &trace,
        ReplayBudget::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(
        matches!(replay.status, ReplayStatus::Replayed { ended: true, .. }),
        "{:?}",
        replay.status
    );
    let mut tampered = trace;
    tampered
        .initial_observation
        .as_mut()
        .unwrap()
        .choice_presentation[0]["disabled_reason"] = json!("错误说明");
    let replay = ReplayTrace::replay(
        &c.program,
        &c.analysis,
        &tampered,
        ReplayBudget::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(!matches!(replay.status, ReplayStatus::Replayed { .. }));
    let mut value: Value = serde_json::from_str(&save).unwrap();
    value["required_features"] = json!(["runtime.language_1_11.v1"]);
    assert!(Story::load(&c.program, &c.analysis, &value.to_string()).is_err());
}

#[test]
fn legacy_language_versions_retain_fingerprint_identity_state_and_output_shape() {
    let source = "let n = 0\nevent start\n  choice once \"一{rnd(1,3)}\" if n == 0\n    -> END\n  choice \"隐藏\" if false\n    -> END\n";
    let mut snapshots = Vec::new();
    for options in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
        CompileOptions::v1_12(),
    ] {
        let c = compile_source_with_options("old.wl", source, options);
        assert!(!c.has_errors(), "{:?}", c.diagnostics);
        let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
        s.continue_story().unwrap();
        let saved: Value = serde_json::from_str(&s.save().unwrap()).unwrap();
        assert!(saved.get("required_features").is_none());
        let trace = json!(s.replay_trace());
        assert!(trace["initial_observation"]
            .get("choice_presentation")
            .is_none());
        snapshots
            .push(json!({"fingerprint":c.analysis.fingerprint,"choices":s.choices(),"save":saved}));
    }
    assert!(snapshots.windows(2).all(|v| v[0] == v[1]));
}

#[test]
fn changed_explanation_changes_fingerprint_but_not_choice_identity() {
    let first = compile(SOURCE);
    let second = compile(&format!(
        "\n\n{}",
        SOURCE.replace("还缺银钥匙", "钥匙尚未找到")
    ));
    assert_ne!(first.analysis.fingerprint, second.analysis.fingerprint);
    let mut a = Story::new_with_seed(&first.program, &first.analysis, 42).unwrap();
    let mut b = Story::new_with_seed(&second.program, &second.analysis, 42).unwrap();
    a.continue_story().unwrap();
    b.continue_story().unwrap();
    assert_eq!(
        a.choice_presentations()[0].id,
        b.choice_presentations()[0].id
    );
    assert!(Story::load(&second.program, &second.analysis, &a.save().unwrap()).is_err());
}

#[test]
fn enable_condition_rng_evaluated_once_and_reads_do_not_advance_it() {
    let c = compile("event start\n  choice \"门\" enable rnd(0, 1) == 1 disabled \"{rnd(0, 100)}\"\n    -> END\n  choice \"走\"\n    -> END\n");
    let mut a = Story::new_with_seed(&c.program, &c.analysis, 81).unwrap();
    let mut b = Story::new_with_seed(&c.program, &c.analysis, 81).unwrap();
    a.continue_story().unwrap();
    b.continue_story().unwrap();
    let before = snapshot(&a);
    for _ in 0..10 {
        a.explain_choices().unwrap();
        let _ = a.choice_presentations();
    }
    assert_eq!(snapshot(&a), before);
    assert_eq!(snapshot(&a), snapshot(&b));
    assert_ne!(before["save"]["rng"], 81);
}

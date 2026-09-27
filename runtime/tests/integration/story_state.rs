use super::*;

#[test]
fn admission_perm_gate_blocks_and_grant_unlocks() {
    let src = "event start\n  -> gated\n\nevent gated perm key\n  你进来了。\n  -> END\n";
    let result = compile_source("p.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let err = s.continue_story().unwrap_err();
    assert!(err.message.contains("权限"), "{err}");

    let src2 =
        "event start\n  grant key\n  -> gated\n\nevent gated perm key\n  你进来了。\n  -> END\n";
    let r2 = compile_source("p.wl", src2);
    let mut s2 = Story::new(&r2.program, &r2.analysis).unwrap();
    let out = transcript(&mut s2);
    assert!(out.contains("你进来了"), "{out}");
    assert!(s2.perm_list().contains(&"key".to_string()));
}

#[test]
fn admission_after_gate_uses_seen() {
    let src = "event start\n  -> gated\n\nevent gated after seen(elsewhere)\n  你进来了。\n  -> END\n\nevent elsewhere\n  -> END\n";
    let result = compile_source("a.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let err = s.continue_story().unwrap_err();
    assert!(err.message.contains("前置"), "{err}");
}

#[test]
fn effect_timing_enter_and_done() {
    let src = r#"
event start
  effect on enter
    grant early as "开场获得"
  权限测试:{perm(early)}。
  choice "去终章"
    -> finale
  -> END

event finale
  effect on done
    grant late as "完成时获得"
  正文结束。
"#;
    let result = compile_source("f.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let out = transcript(&mut s);
    assert!(out.contains("权限测试:true"), "{out}");
    assert!(s.perm_list().contains(&"early".to_string()));
    s.choose(0).unwrap();
    let out2 = transcript(&mut s);
    assert!(out2.contains("[END]"), "{out2}");
    // on done 在事件体自然结束时触发
    assert!(
        s.perm_list().contains(&"late".to_string()),
        "{:?}",
        s.perm_list()
    );
    assert!(s.is_ended());
}

#[test]
fn done_effect_skipped_on_divert_exit() {
    let src = r#"
event start
  choice "离开"
    -> END

event gated
  effect on done
    grant never as "不应触发"
  你不会执行到这里。
"#;
    let result = compile_source("d2.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut s);
    s.choose(0).unwrap();
    transcript(&mut s);
    assert!(!s.perm_list().contains(&"never".to_string()));
}

#[test]
fn drift_switches_storyline_and_records_anchor() {
    let src = "storyline awake\n  event start\n    ->> dream.entry\n  event wake\n    -> END\n\nstoryline dream\n  event dream.entry\n    梦里。\n    -> END\n";
    let result = compile_source("d.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let out = transcript(&mut s);
    assert!(out.contains("梦里"), "{out}");
    assert_eq!(s.storyline(), "dream");
    let drifts: Vec<_> = s
        .anchors()
        .iter()
        .filter(|a| a.kind == AnchorKind::Drift)
        .collect();
    assert_eq!(drifts.len(), 1);
    assert_eq!(drifts[0].detail.as_deref(), Some("dream.entry"));
}

#[test]
fn anchor_statement_records_manual() {
    let src = "event start\n  anchor \"关键转折\" as \"测试说明\"\n  -> END\n";
    let result = compile_source("an.wl", src);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    transcript(&mut s);
    assert_eq!(s.anchors().len(), 1);
    assert_eq!(s.anchors()[0].kind, AnchorKind::Manual);
    assert_eq!(s.anchors()[0].name, "关键转折");
    assert_eq!(s.anchors()[0].note.as_deref(), Some("测试说明"));
    // 分析层也有锚点声明
    assert_eq!(result.analysis.anchors.len(), 1);
    assert_eq!(result.analysis.anchors[0].node, "start");
}

#[test]
fn chronicle_example_full_walkthrough() {
    let result = compile_path(&example("chronicle.wl")).unwrap();
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = String::new();
    log.push_str(&transcript(&mut s)); // start 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["敲门", "在门廊睡下"]
    );
    s.choose(0).unwrap(); // 敲门 → hall
    log.push_str(&transcript(&mut s)); // hall 暂停
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["询问宅子的历史", "告辞"]
    );
    s.choose(0).unwrap(); // 询问 → stair → sleep → 漂流入梦
    log.push_str(&transcript(&mut s)); // dream.entry 暂停
    assert_eq!(s.storyline(), "dream");
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["追问密室", "随雾漂流"]
    );
    s.choose(0).unwrap(); // 追问 → dream.door
    log.push_str(&transcript(&mut s)); // dream.door 暂停
    s.choose(0).unwrap(); // 推门而归 → 漂流回清醒世界
    log.push_str(&transcript(&mut s));
    assert!(log.contains("雾凝成的钥匙"), "{log}");
    assert_eq!(s.storyline(), "awake");
    assert!(s.perm_list().contains(&"brave".to_string()));
    assert!(s.met_list().contains(&"keeper".to_string()));
    assert!(
        !s.met_list().contains(&"servant".to_string()),
        "入梦时女仆应离场"
    );
    let manuals: Vec<_> = s
        .anchors()
        .iter()
        .filter(|a| a.kind == AnchorKind::Manual)
        .collect();
    assert_eq!(manuals.len(), 1);
    assert_eq!(manuals[0].name, "听闻密室");
    assert_eq!(
        s.anchors()
            .iter()
            .filter(|a| a.kind == AnchorKind::Drift)
            .count(),
        2
    );
}

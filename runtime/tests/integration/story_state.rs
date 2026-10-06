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
fn gated_round_trip_preserves_effects_presence_and_anchors() {
    let result = compile_source(
        "round-trip.wl",
        r#"character local
character remote
let enabled = false
storyline primary
  event start
    choice "进入"
      set enabled = true
      -> checkpoint
    choice "跳过"
      -> transfer
  event checkpoint with local
    effect on enter
      meet local
    choice "授权"
      anchor "已授权"
      grant access
      -> transfer
    choice "结束"
      -> END
  event transfer perm access
    effect on enter
      part local
    ->> secondary.entry
  event finish
    往返完成
    -> END
storyline secondary
  event secondary.entry with remote
    effect on enter
      meet remote
    choice "继续" if enabled
      -> secondary.return
    choice "结束"
      -> END
  event secondary.return after seen(checkpoint)
    choice "返回"
      ->> finish
    choice "结束"
      -> END
"#,
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let mut s = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = transcript(&mut s);
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["进入", "跳过"]
    );
    s.choose(0).unwrap();
    log.push_str(&transcript(&mut s));
    assert!(s.met_list().contains(&"local".to_string()));
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["授权", "结束"]
    );
    s.choose(0).unwrap(); // 授权后通过准入，离场并漂流
    log.push_str(&transcript(&mut s));
    assert_eq!(s.storyline(), "secondary");
    assert_eq!(
        s.choices()
            .iter()
            .map(|c| c.label.clone())
            .collect::<Vec<_>>(),
        vec!["继续", "结束"]
    );
    s.choose(0).unwrap(); // seen(checkpoint) 准入
    log.push_str(&transcript(&mut s));
    s.choose(0).unwrap(); // 漂流回原故事线
    log.push_str(&transcript(&mut s));
    assert!(log.contains("往返完成"), "{log}");
    assert!(s.is_ended());
    assert_eq!(s.storyline(), "primary");
    assert!(s.perm_list().contains(&"access".to_string()));
    assert!(s.met_list().contains(&"remote".to_string()));
    assert!(
        !s.met_list().contains(&"local".to_string()),
        "跨线前的离场效果应保留"
    );
    let manuals: Vec<_> = s
        .anchors()
        .iter()
        .filter(|a| a.kind == AnchorKind::Manual)
        .collect();
    assert_eq!(manuals.len(), 1);
    assert_eq!(manuals[0].name, "已授权");
    assert_eq!(
        s.anchors()
            .iter()
            .filter(|a| a.kind == AnchorKind::Drift)
            .count(),
        2
    );
}

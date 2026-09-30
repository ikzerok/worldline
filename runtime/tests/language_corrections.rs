//! 全局块内声明、跨事件完整场景与有符号随机的行为纠偏。
use worldline_core::compile_source;
use worldline_runtime::{Output, Story, Value};

fn compiled(source: &str) -> worldline_core::CompileResult {
    let result = compile_source("corrections.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn text(story: &mut Story<'_>) -> String {
    story
        .continue_story()
        .unwrap()
        .into_iter()
        .filter_map(|o| match o {
            Output::Text { content, .. } => Some(content),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn block_let_is_global_but_initialized_only_on_execution() {
    let result = compiled(
        "event start\n  let fee = 2\n  {fee}\n  -> other\nevent other\n  {fee}\n  -> END\n",
    );
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    assert!(!story.vars().contains_key("fee"));
    assert_eq!(text(&mut story), "2|2");
}

#[test]
fn skipped_declaration_read_and_set_fail_at_use() {
    for instruction in ["{fee}", "set fee = 3"] {
        let source =
            format!("event start\n  if false\n    let fee = 2\n  {instruction}\n  -> END\n");
        let result = compiled(&source);
        let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
        let error = story.continue_story().unwrap_err();
        assert!(error.message.contains("尚未初始化"), "{}", error.message);
        assert_eq!(error.line, Some(4));
    }
}

#[test]
fn block_const_and_let_have_distinct_reentry_behavior() {
    let result = compiled("let n = 0\nevent start\n  const first = n\n  let current = n\n  {first}/{current}\n  set n = n + 1\n  if n < 2\n    -> start\n  -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    assert_eq!(text(&mut story), "0/0|0/1");
    let result = compiled("let n = 0\nevent start\n  const token = rnd(1, 1000)\n  set n = n + 1\n  if n < 2\n    -> start\n  choice \"停\"\n    -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    text(&mut story);
    let saved: serde_json::Value = serde_json::from_str(&story.save().unwrap()).unwrap();
    let mut rng = 31u64;
    rng ^= rng << 13;
    rng ^= rng >> 7;
    rng ^= rng << 17;
    assert_eq!(saved["rng"].as_u64(), Some(rng));
}

#[test]
fn duplicate_global_declarations_and_constant_set_are_rejected() {
    for source in [
        "let fee = 0\nevent start\n  let fee = 1\n  -> END\n",
        "event start\n  if true\n    let fee = 1\n  else\n    const fee = 2\n  -> END\n",
    ] {
        let result = compile_source("duplicates.wl", source);
        assert!(result.diagnostics.iter().any(|d| d.code == "A104"));
    }
    let result = compile_source(
        "const.wl",
        "event start\n  const fee = 2\n  set fee = 3\n  -> END\n",
    );
    assert!(result.diagnostics.iter().any(|d| d.code == "A106"));
}

#[test]
fn initialized_and_uninitialized_globals_roundtrip_without_default_values() {
    let result = compiled("let top = 1\nevent start\n  choice \"开始\"\n    let reached = 2\n  choice \"跳过\"\n    未初始化\n  choice \"继续\"\n    -> END\n");
    for selection in [None, Some(0), Some(1)] {
        let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
        text(&mut story);
        // 上面连续choice属于同组，选择后没有额外暂停，但值仍须保留。
        if let Some(choice) = selection {
            story.choose(choice).unwrap();
            text(&mut story);
        }
        let save = story.save().unwrap();
        let loaded = Story::load(&result.program, &result.analysis, &save).unwrap();
        assert_eq!(story.vars(), loaded.vars());
        assert_eq!(
            loaded.vars().get("reached"),
            if selection == Some(0) {
                Some(&Value::Num(2.0))
            } else {
                None
            }
        );
        for (key, value) in [
            ("unknown", serde_json::json!({"Num": 1.0})),
            ("top", serde_json::json!({"Str": "wrong"})),
        ] {
            let mut invalid: serde_json::Value = serde_json::from_str(&save).unwrap();
            invalid["vars"][key] = value;
            assert!(Story::load(&result.program, &result.analysis, &invalid.to_string()).is_err());
        }
        let mut invalid: serde_json::Value = serde_json::from_str(&save).unwrap();
        invalid["vars"].as_object_mut().unwrap().remove("top");
        assert!(Story::load(&result.program, &result.analysis, &invalid.to_string()).is_err());
    }
}

#[test]
fn cross_event_complete_scene_target_runs_expected_body_and_effects() {
    let result = compiled("let count = 0\ncharacter person\nevent start\n  -> destination.inner\nevent destination\n  skipped\n  scene inner\n    进入完整场景\n    -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    assert_eq!(text(&mut story), "进入完整场景");
    assert_eq!(story.visits().get("destination"), Some(&1));
    assert_eq!(story.visits().get("destination.inner"), Some(&1));
}

#[test]
fn short_scene_target_never_skips_its_parent_body() {
    let source = "event start\n  -> outer\n  scene outer\n    父场景正文\n    scene child\n      子场景正文\n      -> END\nevent outer\n  错误的全局同名事件\n  -> END\n";
    // 每次编译重建符号表，检查目标不受 HashMap 随机遍历顺序影响。
    for _ in 0..64 {
        let result = compiled(source);
        let path = result
            .analysis
            .symbols
            .resolve_target("outer", Some("start"))
            .unwrap();
        assert_eq!(path.full_name("start"), "start.outer");
        let graph = &result.analysis.graph;
        assert!(graph.edges.iter().any(|edge| {
            edge.from == graph.ids["start"]
                && edge.to == graph.ids["start.outer"]
                && edge.kind == worldline_core::graph::EdgeKind::Divert
        }));
        let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
        assert_eq!(text(&mut story), "父场景正文|子场景正文");
        assert_eq!(story.visits().get("outer"), None);
    }
}

#[test]
fn scene_paths_resolve_exactly_and_preserve_declared_priority() {
    for (target, expected) in [
        ("outer.child.grand", "孙场景"),
        ("start.outer.child.grand", "孙场景"),
        ("destination.outer.child", "跨事件子场景"),
        ("destination", "本事件同路径优先"),
        ("destination.inner", "完整名身份优先"),
        ("remote", "全局事件"),
    ] {
        let source = format!(
            "event start\n  -> {target}\n  scene outer\n    父场景\n    scene child\n      子场景\n      scene grand\n        孙场景\n        -> END\n  scene destination\n    scene inner\n      本事件同路径优先\n      -> END\n  scene start\n    scene outer\n      scene child\n        scene grand\n          错误的相对路径遮蔽\n          -> END\nevent destination\n  跨事件根\n  scene outer\n    跨事件父场景\n    scene child\n      跨事件子场景\n      -> END\n  scene inner\n    完整名身份优先\n    -> END\nevent remote\n  全局事件\n  -> END\n"
        );
        let result = compiled(&source);
        let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
        assert_eq!(text(&mut story), expected, "target={target}");
    }
}

#[test]
fn scene_targets_do_not_guess_a_nested_or_cross_event_leaf() {
    for target in ["child", "grand", "missing"] {
        let source = format!(
            "event start\n  -> {target}\n  scene outer\n    scene child\n      -> END\nevent destination\n  scene grand\n    -> END\n"
        );
        let result = compile_source("unknown-scene.wl", &source);
        assert!(result.diagnostics.iter().any(|d| d.code == "A101"));
        assert!(result
            .analysis
            .symbols
            .resolve_target(target, Some("start"))
            .is_none());
    }
}

#[test]
fn negative_random_singleton_and_invalid_intervals_are_explicit() {
    let result = compiled("event start\n  {rnd(-2, -2)}\n  -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    assert_eq!(text(&mut story), "-2");
    for expression in ["rnd(-1, -2)", "rnd(0.1, 0.9)", "rnd(0, 9007199254740992)"] {
        let result = compile_source(
            "invalid-random.wl",
            &format!("event start\n  {{{expression}}}\n  -> END\n"),
        );
        assert!(
            result.diagnostics.iter().any(|d| d.code == "A103"),
            "{:?}",
            result.diagnostics
        );
    }
    let result =
        compiled("let lower = -1\nlet upper = -2\nevent start\n  {rnd(lower, upper)}\n  -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    let error = story.continue_story().unwrap_err();
    assert_eq!(error.line, Some(4));
}

use worldline_core::{compile_source_with_options, CompileOptions};
use worldline_runtime::{Output, Story};
fn compile(src: &str) -> worldline_core::CompileResult {
    let c = compile_source_with_options(
        "story.wl",
        src,
        CompileOptions::v1_11().with_localization_ids(true),
    );
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    c
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
const COMBINED: &str = r#"
world coast
tag map
tag log
tag key
character doctor as "林医生"
state evidence on world coast with map, log, key
let fuel = 8
let people = 2
rule fee(n: num) -> num = n * 2
rule ready() -> bool = when(people > 0, fuel / people >= 2, false)
fragment explain(place: str, selected: tag)
  local charge: num = fee(people)
  local held: tagset = members(state(evidence))
  say doctor "{place}需要{charge}罐油，现有{count(held)}件证物。" direction "低声" #wl-localization:line_1
  call confirm(selected)
  回到说明:{place}/{charge}/{count(held)}。
fragment confirm(selected: tag)
  local roll: num = rnd(-2, 2)
  choice once "交出证物" if ready() and contains(members(state(evidence)), selected)
    become state(evidence) remove from tags(selected)
    return
  choice "保留"
    return
event start
  call explain("医院", tag(map))
  医院后文。
  set people = 0
  惰性规则:{ready()}。
  -> END
"#;
#[test]
fn combined_nested_pause_save_return_types_once_rng_history() {
    let c = compile(COMBINED);
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    let out = s.continue_story().unwrap();
    assert!(
        matches!(&out[0],Output::Text{speaker:Some(speaker),content,..} if speaker.id=="doctor" && content.contains("3件证物"))
    );
    assert_eq!(s.choices().len(), 2);
    let saved = s.save().unwrap();
    assert!(saved.contains("runtime.language_1_11.v1"));
    let mut restored = Story::load(&c.program, &c.analysis, &saved).unwrap();
    assert_eq!(restored.choices().len(), 2);
    s.choose(0).unwrap();
    restored.choose(0).unwrap();
    assert_eq!(text(&mut s), text(&mut restored));
    assert_eq!(s.states()["evidence"], vec!["key", "log"]);
    assert_eq!(s.state_history().len(), 1);
    assert!(s.is_ended());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&s.save().unwrap()).unwrap(),
        serde_json::from_str::<serde_json::Value>(&restored.save().unwrap()).unwrap()
    );
}
#[test]
fn rules_read_current_globals_and_lazy_only_chosen_branch() {
    let c=compile("let n = 2\nrule value() -> num = when(n > 0, 10 / n, 0)\nevent start\n  {value()}\n  set n = 0\n  {value()}\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    assert_eq!(text(&mut s), "5|0");
}
#[test]
fn legacy_text_and_unused_language_version_preserve_fingerprint() {
    let src = "event start\n  call missing()\n  return\n  say nobody \"正文\"\n  -> END\n";
    for options in [CompileOptions::v1_9(), CompileOptions::v1_10()] {
        let c = compile_source_with_options("story.wl", src, options);
        assert!(!c.has_errors());
        let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
        assert!(text(&mut s).starts_with("call missing()|return|say"));
        assert!(!s.save().unwrap().contains("required_features"));
    }
    let src = "event start\n  普通正文\n  -> END\n";
    let a = compile_source_with_options("s.wl", src, CompileOptions::v1_9());
    let b = compile(src);
    assert_eq!(a.analysis.fingerprint, b.analysis.fingerprint);
}
#[test]
fn pure_rules_and_fragment_cycles_and_types_are_rejected() {
    for src in [
        "rule bad() -> num = rnd(1,2)\nevent start\n  {bad()}\n",
        "rule a() -> num = b()\nrule b() -> num = a()\nevent start\n  {a()}\n",
        "fragment a()\n  call b()\nfragment b()\n  call a()\nevent start\n  call a()\n",
        "rule f(n: num) -> num = n\nevent start\n  {f(\"x\")}\n",
        "event start\n  return\n",
        "event start\n  say ghost \"x\"\n",
        "event start\n  {tag(missing)}\n",
        "fragment f(n: num)\n  set n = 4\nevent start\n  call f(1)\n",
        "fragment f(n: num)\n  local n: str = \"x\"\nevent start\n  call f(1)\n",
    ] {
        let c = compile_source_with_options("bad.wl", src, CompileOptions::v1_11());
        assert!(c.has_errors(), "{src}");
    }
}
#[test]
fn skipped_local_never_reads_global_and_tail_jump_discards_calls() {
    let c=compile("let n = 9\nfragment f()\n  if false\n    local n: num = 1\n  {n}\nevent start\n  call f()\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    assert!(s
        .continue_story()
        .unwrap_err()
        .message
        .contains("尚未初始化"));
    let c=compile("fragment f()\n  -> other\nevent start\n  call f()\n  不该输出\nevent other\n  完成\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    assert_eq!(text(&mut s), "完成");
}
#[test]
fn save_rejects_unknown_features_bad_frame_missing_parameter_and_unknown_tag() {
    let c = compile(COMBINED);
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    text(&mut s);
    let base: serde_json::Value = serde_json::from_str(&s.save().unwrap()).unwrap();
    for mutation in 0..4 {
        let mut v = base.clone();
        match mutation {
            0 => v["required_features"] = serde_json::json!(["future"]),
            1 => v["frames"][1]["idx"] = 999.into(),
            2 => {
                v["frames"][1]["locals"]
                    .as_object_mut()
                    .unwrap()
                    .remove("place");
            }
            _ => v["frames"][2]["locals"]["selected"] = serde_json::json!({"Tag":"ghost"}),
        }
        assert!(
            Story::load(&c.program, &c.analysis, &v.to_string()).is_err(),
            "mutation {mutation}"
        );
    }
}
#[test]
fn directions_and_localization_identity_are_metadata_but_speaker_and_rules_are_semantic() {
    let a = compile(COMBINED);
    let b = compile(
        &COMBINED
            .replace("低声", "非常轻声")
            .replace("line_1", "stable_line"),
    );
    assert_eq!(a.analysis.fingerprint, b.analysis.fingerprint);
    let c = compile(&COMBINED.replace("n * 2", "n * 3"));
    assert_ne!(a.analysis.fingerprint, c.analysis.fingerprint);
}
#[test]
fn collections_are_deduplicated_typed_and_do_not_mutate_state() {
    let c=compile("world w\ntag a\ntag b\nstate s on world w with a\nevent start\n  {count(tags(tag(a), tag(a)))} / {count(difference(union(members(state(s)), tags(tag(b))), tags(tag(a))))}\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    assert_eq!(text(&mut s), "1 / 1");
    assert_eq!(s.states()["s"], vec!["a"]);
    assert!(s.state_history().is_empty());
}
#[test]
fn once_is_shared_per_fragment_definition_and_returns_one_layer() {
    let c=compile("fragment inner()\n  choice once \"一次\"\n    一次正文\n    return\n  choice \"跳过\"\n    return\nfragment outer()\n  call inner()\n  外层后文\n  return\nevent start\n  call outer()\n  第一次返回\n  call outer()\n  第二次返回\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    assert_eq!(text(&mut s), "");
    s.choose(0).unwrap();
    assert_eq!(text(&mut s), "一次正文|外层后文|第一次返回");
    assert_eq!(s.choices().len(), 1);
    assert_eq!(s.choices()[0].label, "跳过");
    s.choose(0).unwrap();
    assert_eq!(text(&mut s), "外层后文|第二次返回");
}
#[test]
fn rule_scope_does_not_capture_caller_locals_and_evidence_records_lazy_branch() {
    let c=compile("let n = 7\nrule global() -> num = n\nrule safe(x: num) -> bool = when(x == 0, true, 1 / x > 0)\nfragment f(n: num)\n  {global()}\n  choice \"继续\" if safe(n)\n    return\nevent start\n  call f(0)\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    assert_eq!(text(&mut s), "7");
    let before = s.save().unwrap();
    let evidence = s.choice_evidence().unwrap();
    let json = serde_json::to_string(evidence).unwrap();
    assert!(json.contains("not_evaluated"));
    assert!(json.contains("safe"));
    assert_eq!(before, s.save().unwrap());
}
#[test]
fn fragment_global_initializer_type_is_known_before_cross_event_assignment() {
    let c=compile_source_with_options("bad.wl","fragment init(value: num)\n  let global = value\nevent start\n  call init(2)\n  -> other\nevent other\n  set global = \"wrong\"\n  -> END\n",CompileOptions::v1_11());
    assert!(c.diagnostics.iter().any(|d| d.code == "A103"));
}
#[test]
fn malformed_collections_and_lazy_branch_types_are_rejected() {
    for expr in [
        "count(\"x\")",
        "tags(\"x\")",
        "when(true, 1, false)",
        "members(tag(a))",
        "contains(tags(), \"a\")",
    ] {
        let src = format!("tag a\nevent start\n  {{{expr}}}\n  -> END\n");
        let c = compile_source_with_options("bad.wl", &src, CompileOptions::v1_11());
        assert!(c.has_errors(), "{expr}");
    }
    let c=compile("tag a\nevent start\n  {count(tags())}/{count(intersect(tags(tag(a)), tags()))}\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    assert_eq!(text(&mut s), "0/0");
}
#[test]
fn released_v070_fingerprint_and_save_shape_are_preserved() {
    let src="let fuel = 8\nconst known = true\ncharacter doctor as \"岚医生\"\nevent start\n  还剩{fuel}罐油，掷骰{rnd(1, 6)}。\n  choice once \"继续\" if known\n    set fuel = fuel - 1\n    -> END\n";
    let c = worldline_core::compile_source("world.wl", src);
    assert!(!c.has_errors());
    // v0.7.0 release baseline 76bd48b0, genuine saved-state compatibility vector.
    assert_eq!(c.analysis.fingerprint, 3_181_198_091_909_106_838);
    let old = r#"{"fingerprint":3181198091909106838,"vars":{"known":{"Bool":true},"fuel":{"Num":8.0}},"visits":{"start":1},"turns":0,"taken_once":[],"frames":[{"node":"start","idx":1,"src":null}],"glue_pending":false,"paused":true,"rng":33550100447,"seed":31,"choice_coverage":{},"storyline":"main","met":[],"anchors":[],"states":{},"state_history":[]}"#;
    let mut loaded = Story::load(&c.program, &c.analysis, old).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&loaded.save().unwrap()).unwrap();
    assert_eq!(
        saved,
        serde_json::from_str::<serde_json::Value>(old).unwrap()
    );
    loaded.choose(0).unwrap();
    text(&mut loaded);
    assert!(loaded.is_ended());
}
#[test]
fn legacy_permission_inputs_inside_rules_and_fragments_are_normalized() {
    let c=compile("rule allowed() -> bool = perm(open)\nfragment grant_it()\n  grant open\n  return\nevent start\n  {allowed()}\n  call grant_it()\n  {allowed()}\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    assert_eq!(text(&mut s), "false|true");
}
#[test]
fn nested_fragment_file_links_resolve_from_definition_and_manifest_accepts_111() {
    use std::{collections::BTreeMap, path::Path};
    let root = std::env::temp_dir().join(format!("language111-links-{}", std::process::id()));
    let entry = root.join("world.wl");
    let fragment = root.join("chapters/fragment.wl");
    let sources = BTreeMap::from([
        (entry.clone(), "event start\n  call f()\n  -> END\n".into()),
        (
            fragment,
            "fragment f()\n  [[file:../world.wl|入口]]\n  return\n".into(),
        ),
    ]);
    let c = worldline_core::compile_sources_with_options(&entry, &sources, CompileOptions::v1_11());
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    let out = s.continue_story().unwrap();
    assert!(matches!(&out[0],Output::Text{links,..} if Path::new(&links[0].target.id)==entry));
}
#[test]
fn old_static_become_note_is_not_mistaken_for_dynamic_operation() {
    let src="world w\ntag a\nstate s on world w with []\nevent start\n  become s add a as \"literal add from ordinary note\"\n  -> END\n";
    let old = compile_source_with_options("s.wl", src, CompileOptions::v1_9());
    let new = compile(src);
    assert!(!old.has_errors());
    assert_eq!(old.analysis.fingerprint, new.analysis.fingerprint);
}
#[test]
fn fragment_static_changes_are_validated_and_dynamic_changes_are_indexed() {
    for action in ["become missing add a", "become s add missing"] {
        let src=format!("world w\ntag a\nstate s on world w with []\nfragment f()\n  {action}\nevent start\n  call f()\n  -> END\n");
        let c = compile_source_with_options("bad.wl", &src, CompileOptions::v1_11());
        assert!(c.diagnostics.iter().any(|d| d.code == "A216"), "{action}");
    }
    let c=compile("world w\ntag a\nstate s on world w with []\nfragment f(target: state)\n  become s add a\n  become state(s) remove from tags(tag(a))\n  become target add from tags(tag(a))\nevent start\n  call f(state(s))\n  -> END\n");
    assert_eq!(c.analysis.catalog.states["s"].changes.len(), 2);
    assert_eq!(c.analysis.catalog.dynamic_state_changes.len(), 2);
    let site = &c.analysis.catalog.states["s"].changes[0];
    assert_eq!(site.source.as_ref().unwrap().kind, "fragment");
    assert!(site.event.is_empty());
}
#[test]
fn real_tail_transitions_in_fragments_are_reachable_with_condition_context() {
    let c=compile("fragment transfer()\n  if true\n    -> destination\nevent start\n  call transfer()\n  -> END\nevent destination\n  到达\n  -> END\n");
    assert!(!c.diagnostics.iter().any(|d| d.code == "A201"));
    let edge = c
        .analysis
        .graph
        .edges
        .iter()
        .find(|e| e.label.as_deref() == Some("经片段 transfer"))
        .unwrap();
    assert_eq!(edge.contexts[0].conditions, vec!["true"]);
}
#[test]
fn signature_and_local_type_errors_keep_definition_source_lines() {
    for (src, line) in [
        ("rule f() -> num = true\nevent start\n  -> END\n", 1),
        (
            "fragment f()\n  local n: num = true\nevent start\n  call f()\n",
            2,
        ),
        ("rule f(n: num) -> num = n\nevent start\n  {f(true)}\n", 3),
    ] {
        let c = compile_source_with_options("bad.wl", src, CompileOptions::v1_11());
        assert!(
            c.diagnostics
                .iter()
                .any(|d| d.code == "A103" && d.span.line == line),
            "{:?}",
            c.diagnostics
        );
    }
}
#[test]
fn state_and_history_identity_corruption_and_duplicate_json_keys_are_rejected() {
    let c = compile(COMBINED);
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    text(&mut s);
    s.choose(0).unwrap();
    text(&mut s);
    let json = s.save().unwrap();
    let base: serde_json::Value = serde_json::from_str(&json).unwrap();
    for index in 0..4 {
        let mut v = base.clone();
        match index {
            0 => v["states"]["evidence"] = serde_json::json!(["ghost"]),
            1 => v["states"]["evidence"] = serde_json::json!(["key", "key"]),
            2 => v["state_history"][0]["before"] = serde_json::json!(["ghost"]),
            _ => v["state_history"][0]["after"] = serde_json::json!(["log", "log"]),
        }
        assert!(Story::load(&c.program, &c.analysis, &v.to_string()).is_err());
    }
    let duplicate = json.replacen("\"turns\": 1", "\"turns\": 1, \"turns\": 0", 1);
    assert!(Story::load(&c.program, &c.analysis, &duplicate).is_err());
}
#[test]
fn opaque_text_tags_do_not_authorize_saved_typed_identities() {
    let c = compile(
        "tag a\nlet chosen = tag(a)\nevent start\n  正文 #ghost\n  choice \"继续\"\n    -> END\n",
    );
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    text(&mut s);
    let mut saved: serde_json::Value = serde_json::from_str(&s.save().unwrap()).unwrap();
    saved["vars"]["chosen"] = serde_json::json!({"Tag":"ghost"});
    assert!(Story::load(&c.program, &c.analysis, &saved.to_string()).is_err());
}
#[test]
fn all_six_parameter_types_and_collection_globals_survive_pause() {
    let c=compile("world w\ntag a\nstate s on world w with a\nlet shared = tags(tag(a), tag(a))\nfragment f(n: num, title: str, flag: bool, item: tag, bag: tagset, target: state)\n  local copied: tagset = bag\n  local where: state = target\n  choice \"继续\"\n    {n}/{title}/{flag}/{item}/{count(copied)}/{count(members(where))}\n    return\nevent start\n  call f(2, \"中文🙂\", true, tag(a), shared, state(s))\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    text(&mut s);
    let mut restored = Story::load(&c.program, &c.analysis, &s.save().unwrap()).unwrap();
    restored.choose(0).unwrap();
    assert_eq!(text(&mut restored), "2/中文🙂/true/a/1/1");
}
#[test]
fn calls_and_returns_do_not_fire_event_effects_or_visit_counts() {
    let c=compile("world w\ntag entered\ntag exited\nstate ledger on world w with []\nfragment inner()\n  内文\n  return\nfragment outer()\n  call inner()\n  return\nevent start\n  effect on enter\n    become ledger add entered\n  effect on exit\n    become ledger add exited\n  call outer()\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    assert_eq!(text(&mut s), "内文");
    assert_eq!(s.state_history().len(), 2);
    assert_eq!(s.visits().len(), 1);
    assert_eq!(s.visits()["start"], 1);
}
#[test]
fn speaker_id_is_semantic_even_with_identical_display_names() {
    let src="character a as \"同名\"\ncharacter b as \"同名\"\nevent start\n  say a \"你好\" direction \"\"\n  -> END\n";
    let a = compile(src);
    let b = compile(&src.replace("say a", "say b"));
    assert_ne!(a.analysis.fingerprint, b.analysis.fingerprint);
}
#[test]
fn say_decodes_literal_escapes_once_and_preserves_expression_string_layers() {
    let source = r#"character speaker
let number = 3
event start
  say speaker "中文🙂 שלום العربية：\{unknown\} / \\ / \[[character:ghost|伪链接]] / \"引号\" / {number}"
  say speaker "{when(true, \"a\\nb\", \"no\")}；{when(true, \"brace } quote \\\" ok\", \"no\")}"
  -> END
"#;
    let c = compile(source);
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 31).unwrap();
    let output = s.continue_story().unwrap();
    let Output::Text { content, links, .. } = &output[0] else {
        panic!()
    };
    assert_eq!(
        content,
        "中文🙂 שלום العربية：{unknown} / \\ / [[character:ghost|伪链接]] / \"引号\" / 3"
    );
    assert!(links.is_empty());
    let Output::Text { content, .. } = &output[1] else {
        panic!()
    };
    assert_eq!(content, "a\nb；brace } quote \" ok");
}
#[test]
fn say_link_labels_keep_existing_delimiter_restrictions() {
    let source = r#"character a
event start
  say a "[[character:a|她说\"好\"，{字面}]]"
  -> END
"#;
    let c = compile_source_with_options("bad.wl", source, CompileOptions::v1_11());
    assert!(c.diagnostics.iter().any(|d| d.code == "P004"));
    assert!(!c.diagnostics.iter().any(|d| d.code == "A102"));
}
#[test]
fn new_definitions_exist_before_state_targets_and_static_stats_count_once() {
    let c=compile("tag a\nrule ready() -> bool = true\nfragment body()\n  唯一正文\n  choice \"继续\"\n    return\nstate rule_state on rule ready with []\nstate fragment_state on fragment body with a\nevent start\n  call body()\n  call body()\n  -> END\n");
    assert_eq!(c.analysis.stats.words, 4);
    assert_eq!(c.analysis.stats.choices, 1);
    assert_eq!(
        c.analysis.catalog.states["fragment_state"].target.kind,
        "fragment"
    );
}
#[test]
fn opening_and_reopening_migrates_quoted_permission_in_say_without_touching_direction() {
    use std::fs;
    let root = std::env::temp_dir().join(format!(
        "language111-quoted-permission-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
    )
    .unwrap();
    fs::write(
        root.join("world.wl"),
        r#"character c
rule allowed() -> bool = perm("open")
event start
  grant open
  say c "{perm(\"open\")}/{allowed()}" direction "perm(open) private note"
  -> END
"#,
    )
    .unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 31).unwrap();
    assert_eq!(text(&mut story), "true/true");
    assert!(project
        .document(&root.join("world.wl"))
        .unwrap()
        .contains("perm(open) private note"));
    project.save().unwrap();
    let mut reopened = worldline_core::project::Project::open(&root).unwrap();
    let again = reopened.compile();
    assert!(!again.has_errors(), "{:?}", again.diagnostics);
    assert_eq!(compiled.analysis.fingerprint, again.analysis.fingerprint);
    fs::remove_dir_all(root).unwrap();
}

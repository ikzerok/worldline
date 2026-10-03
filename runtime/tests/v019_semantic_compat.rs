//! 0.18 真实CLI黄金：来源侧表不能污染指纹、选择、once、存档或真实回放。
use serde_json::Value;
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    ReplayBudget, ReplayCancellation, ReplayCheckpoint, ReplayStatus, ReplayTrace, Story,
};

struct Fixture {
    source: &'static str,
    paused: &'static str,
    saves: [&'static str; 2],
    traces: [&'static str; 2],
    checkpoints: [&'static str; 2],
    checkpoint_traces: [&'static str; 2],
    paths: [&'static [usize]; 2],
}
macro_rules! fixture {
    ($name:literal, $a:expr, $b:expr) => {
        Fixture {
            source: include_str!(concat!("v019_fixtures/", $name, ".wl")),
            paused: include_str!(concat!("v019_fixtures/", $name, "-paused-save.json")),
            saves: [
                include_str!(concat!("v019_fixtures/", $name, "-branch-0-save.json")),
                include_str!(concat!("v019_fixtures/", $name, "-branch-1-save.json")),
            ],
            traces: [
                include_str!(concat!("v019_fixtures/", $name, "-branch-0-trace.json")),
                include_str!(concat!("v019_fixtures/", $name, "-branch-1-trace.json")),
            ],
            checkpoints: [
                include_str!(concat!(
                    "v019_fixtures/",
                    $name,
                    "-branch-0-checkpoint.json"
                )),
                include_str!(concat!(
                    "v019_fixtures/",
                    $name,
                    "-branch-1-checkpoint.json"
                )),
            ],
            checkpoint_traces: [
                include_str!(concat!(
                    "v019_fixtures/",
                    $name,
                    "-branch-0-checkpoint-trace.json"
                )),
                include_str!(concat!(
                    "v019_fixtures/",
                    $name,
                    "-branch-1-checkpoint-trace.json"
                )),
            ],
            paths: [$a, $b],
        }
    };
}
fn fixtures() -> [Fixture; 2] {
    [
        fixture!("last-bell", &[0], &[1]),
        fixture!("consumer", &[0, 0], &[1, 1]),
    ]
}
fn value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap()
}
fn fixture_options() -> CompileOptions {
    CompileOptions::v1_13()
        .with_object_refs(true)
        .with_character_refs(true)
}
fn compiled(f: &Fixture) -> CompileResult {
    let old: ReplayTrace = serde_json::from_str(f.traces[0]).unwrap();
    // 使用黄金记录中的真实名义来源路径，不读取该路径，也不删除任何语义字段。
    let file = old
        .initial_observation
        .as_ref()
        .unwrap()
        .state
        .pointer("/calls/0/file")
        .and_then(Value::as_str)
        .unwrap_or("world.wl");
    let result = compile_source_with_options(file, f.source, fixture_options());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.fingerprint, old.fingerprint);
    result
}
fn advance(story: &mut Story<'_>, path: &[usize]) {
    for index in path {
        story.choose(*index).unwrap();
        story.continue_story().unwrap();
    }
    assert!(story.is_ended());
}
fn replay(
    c: &CompileResult,
    trace: &ReplayTrace,
) -> Result<worldline_runtime::ReplayResult, worldline_runtime::RunError> {
    ReplayTrace::replay(
        &c.program,
        &c.analysis,
        trace,
        ReplayBudget::new(10_000, 5_000),
        &ReplayCancellation::new(),
    )
}

#[test]
fn real_v018_paused_story_saves_continue_to_identical_complete_saves() {
    for f in fixtures() {
        let c = compiled(&f);
        let mut fresh = Story::new_with_seed(&c.program, &c.analysis, 7).unwrap();
        fresh.continue_story().unwrap();
        assert_eq!(value(&fresh.save().unwrap()), value(f.paused));
        for branch in 0..2 {
            let mut story = Story::load(&c.program, &c.analysis, f.paused).unwrap();
            assert_eq!(value(&story.save().unwrap()), value(f.paused));
            advance(&mut story, f.paths[branch]);
            assert_eq!(value(&story.save().unwrap()), value(f.saves[branch]));
            assert!(!story.save().unwrap().contains("problem_source_context"));
        }
    }
}

#[test]
fn genuine_current_traces_keep_all_old_observations_choice_ids_once_and_coverage() {
    for f in fixtures() {
        let c = compiled(&f);
        for branch in 0..2 {
            let old: ReplayTrace = serde_json::from_str(f.traces[branch]).unwrap();
            assert_eq!(old.runtime_version, "0.18.0");
            let mut story = Story::new_with_seed(&c.program, &c.analysis, 7).unwrap();
            story.continue_story().unwrap();
            advance(&mut story, f.paths[branch]);
            let current = story.replay_trace();
            assert_eq!(current.runtime_version, env!("CARGO_PKG_VERSION"));
            assert_eq!(current.schema_version, old.schema_version);
            assert_eq!(current.fingerprint, old.fingerprint);
            assert_eq!(current.origin, old.origin);
            assert_eq!(current.initial_observation, old.initial_observation);
            assert_eq!(current.steps, old.steps);
            assert_eq!(current.complete, old.complete);
            let outcome = replay(&c, &current).unwrap();
            assert_eq!(
                outcome.status,
                ReplayStatus::Replayed {
                    ended: true,
                    complete: true
                }
            );
            assert_eq!(outcome.current_state, story.state_view());
            assert_eq!(value(&story.save().unwrap()), value(f.saves[branch]));
        }
    }
}

#[test]
fn untouched_v018_traces_and_checkpoints_obey_the_real_runtime_version_guard() {
    for f in fixtures() {
        let c = compiled(&f);
        for branch in 0..2 {
            let checkpoint: ReplayCheckpoint = serde_json::from_str(f.checkpoints[branch]).unwrap();
            assert_eq!(checkpoint.runtime_version, "0.18.0");
            let restored = Story::from_checkpoint(&c.program, &c.analysis, &checkpoint);
            if env!("CARGO_PKG_VERSION") == "0.18.0" {
                assert!(restored.is_ok());
            } else {
                assert!(restored.err().unwrap().message.contains("runtime_version"));
            }
            for raw in [f.traces[branch], f.checkpoint_traces[branch]] {
                let old: ReplayTrace = serde_json::from_str(raw).unwrap();
                let before = serde_json::to_string(&old).unwrap();
                let result = replay(&c, &old);
                if env!("CARGO_PKG_VERSION") == "0.18.0" {
                    assert!(result.is_ok());
                } else {
                    assert!(result.unwrap_err().message.contains("runtime_version"));
                }
                assert_eq!(serde_json::to_string(&old).unwrap(), before);
            }
        }
    }
}

#[test]
fn every_supported_language_version_keeps_plain_semantics_and_no_context_save_feature() {
    let source = "let file = \"语义\"\nlet line = 7\nevent start\n  choice once \"保留{file}/{line}\" if line > 0\n    -> END\n";
    let mut fingerprints = Vec::new();
    let mut saves = Vec::new();
    for options in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
        CompileOptions::v1_12(),
        CompileOptions::v1_13(),
    ] {
        let c = compile_source_with_options("versions.wl", source, options);
        assert!(!c.has_errors(), "{:?}", c.diagnostics);
        fingerprints.push(c.analysis.fingerprint);
        let mut story = Story::new_with_seed(&c.program, &c.analysis, 7).unwrap();
        story.continue_story().unwrap();
        saves.push(value(&story.save().unwrap()));
    }
    assert!(fingerprints.windows(2).all(|x| x[0] == x[1]));
    assert!(saves.windows(2).all(|x| x[0] == x[1]));
}

#[test]
fn real_choice_and_rule_source_reads_do_not_reexecute_or_mutate_runtime() {
    let f = fixture!("consumer", &[0, 0], &[1, 1]);
    let c = compiled(&f);
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 7).unwrap();
    story.continue_story().unwrap();
    let before = (story.save().unwrap(), story.replay_trace());
    let mut saw_rule = false;
    for _ in 0..4 {
        for choice in story.choice_evidence().unwrap() {
            let source = choice.source.as_ref().unwrap();
            let target =
                worldline_core::evidence_source::resolve_evidence_source(&c, source).unwrap();
            assert!(c.sources[&target.path][target.range].starts_with("choice "));
            if let Some(condition) = &choice.condition {
                for node in &condition.evidence.as_ref().unwrap().nodes {
                    if let Some(source) = &node.source {
                        let target =
                            worldline_core::evidence_source::resolve_evidence_source(&c, source)
                                .unwrap();
                        assert!(c.sources[&target.path][target.range].starts_with("rule ready("));
                        saw_rule = true;
                    }
                }
            }
        }
    }
    assert!(saw_rule);
    assert_eq!((story.save().unwrap(), story.replay_trace()), before);
}

#[test]
fn fresh_trace_checkpoint_and_story_save_survive_quoted_fragment_source_relocation() {
    let f = fixture!("consumer", &[0, 0], &[1, 1]);
    let original = compiled(&f);
    let mut story = Story::new_with_seed(&original.program, &original.analysis, 7).unwrap();
    story.continue_story().unwrap();
    let save = story.save().unwrap();
    let checkpoint = story.checkpoint().unwrap();
    let old_state = story.state_view();
    advance(&mut story, f.paths[0]);
    let current_trace = story.replay_trace();
    let moved_source = f.source;
    let moved = compile_source_with_options("章节/新稿.wl", moved_source, fixture_options());
    assert!(!moved.has_errors(), "{:?}", moved.diagnostics);
    assert_eq!(moved.analysis.fingerprint, original.analysis.fingerprint);
    let mut restored = Story::load(&moved.program, &moved.analysis, &save).unwrap();
    assert_eq!(value(&restored.save().unwrap()), value(&save));
    let state = restored.state_view();
    assert_eq!(state["calls"][0]["file"], "章节/新稿.wl");
    assert_eq!(
        state["calls"][0]["line"].as_u64().unwrap(),
        old_state["calls"][0]["line"].as_u64().unwrap()
    );
    assert_eq!(state["calls"][0]["locals"], old_state["calls"][0]["locals"]);
    assert_eq!(state["vars"], old_state["vars"]);
    let restored_checkpoint =
        Story::from_checkpoint(&moved.program, &moved.analysis, &checkpoint).unwrap();
    assert_eq!(value(&restored_checkpoint.save().unwrap()), value(&save));
    advance(&mut restored, f.paths[0]);
    assert_eq!(value(&restored.save().unwrap()), value(f.saves[0]));
    let outcome = replay(&moved, &current_trace).unwrap();
    assert_eq!(
        outcome.status,
        ReplayStatus::Replayed {
            ended: true,
            complete: true
        }
    );
    assert_eq!(outcome.current_state, restored.state_view());
}

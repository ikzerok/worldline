//! 源码定位变化不改变重放语义，原始定位与严格检查点契约仍保留。
use std::{fs, path::PathBuf};
use worldline_core::{
    compile_source_with_options, project::Project, CompileOptions, CompileResult,
};
use worldline_runtime::{
    ReplayBudget, ReplayCancellation, ReplayOrigin, ReplayResult, ReplaySession, ReplayStatus,
    ReplayTrace, Story,
};

const SINGLE: &str = "fragment gate(file: str)\n  local line: num = 7\n  choice \"继续\"\n    return\nevent start\n  call gate(\"地图\")\n  结束。\n  -> END\n";
const NESTED: &str = "fragment outer(file: str)\n  local line: num = 7\n  call inner(file)\n  返回。\n  return\nfragment inner(file: str)\n  local line: num = 8\n  choice \"继续\"\n    return\nevent start\n  call outer(\"地图\")\n  结束。\n  -> END\n";

fn compile(file: &str, source: &str) -> CompileResult {
    let result = compile_source_with_options(file, source, CompileOptions::v1_11());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

fn trace(result: &CompileResult, checkpoint: bool, complete: bool) -> ReplayTrace {
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 31).unwrap();
    story.continue_story().unwrap();
    if checkpoint {
        story.start_trace_from_here().unwrap();
    }
    if complete {
        story.choose(0).unwrap();
        story.continue_story().unwrap();
        assert!(story.is_ended());
    }
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

fn assert_replayed(result: &ReplayResult, complete: bool) {
    assert_eq!(
        result.status,
        ReplayStatus::Replayed {
            ended: complete,
            complete,
        }
    );
}

#[test]
fn single_and_nested_calls_ignore_only_changed_locations_in_entry_and_checkpoint_traces() {
    for (source, depth) in [(SINGLE, 1), (NESTED, 2)] {
        let original = compile("original/story.wl", source);
        for (file, changed_source, line_delta) in [
            ("original/story.wl", format!("\n{source}"), 1),
            (
                "original/story.wl",
                format!("// 只改作者注释\n/* 说明 */\n{source}"),
                2,
            ),
            ("copied/story.wl", source.to_owned(), 0),
        ] {
            let changed = compile(file, &changed_source);
            assert_eq!(original.analysis.fingerprint, changed.analysis.fingerprint);
            for checkpoint in [false, true] {
                for complete in [false, true] {
                    let original_trace = trace(&original, checkpoint, complete);
                    let before = serde_json::to_string(&original_trace).unwrap();
                    let result = replay(&changed, &original_trace);
                    assert_replayed(&result, complete);
                    assert_eq!(serde_json::to_string(&original_trace).unwrap(), before);
                    assert_eq!(result.completed_choices, usize::from(complete));
                    if complete {
                        continue;
                    }
                    let expected = &original_trace.initial_observation.as_ref().unwrap().state;
                    let current = &result.current_state["calls"];
                    assert_eq!(current.as_array().unwrap().len(), depth);
                    for index in 0..depth {
                        assert_eq!(current[index]["file"], file);
                        assert_eq!(
                            current[index]["line"].as_u64().unwrap(),
                            expected["calls"][index]["line"].as_u64().unwrap() + line_delta
                        );
                        assert_eq!(current[index]["locals"], expected["calls"][index]["locals"]);
                    }
                    let mut story =
                        Story::new_with_seed(&changed.program, &changed.analysis, 31).unwrap();
                    story.continue_story().unwrap();
                    assert_eq!(result.current_state, story.state_view());
                }
            }
        }
    }
}

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "wl-replay-location-workspace-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("original/.world")).unwrap();
        fs::write(
            root.join("original/.world/project.json"),
            r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
        )
        .unwrap();
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn copied_workspace_and_moved_shared_fragment_replay_with_current_navigation() {
    let workspace = Workspace::new();
    let original_root = workspace.0.join("original");
    fs::create_dir(original_root.join("shared")).unwrap();
    fs::write(
        original_root.join("world.wl"),
        "include \"shared/gate.wl\"\nevent start\n  call gate(\"地图\")\n  结束。\n  -> END\n",
    )
    .unwrap();
    fs::write(
        original_root.join("shared/gate.wl"),
        SINGLE.split("event start").next().unwrap(),
    )
    .unwrap();
    let mut project = Project::open(&original_root).unwrap();
    let original = project.compile();
    assert!(!original.has_errors(), "{:?}", original.diagnostics);
    let traces = [trace(&original, false, true), trace(&original, true, true)];
    let paused_trace = trace(&original, false, false);
    let copied_root = workspace.0.join("copied");
    project.export(&copied_root).unwrap();

    for moved in [false, true] {
        let fragment_path = if moved {
            "parts/checkpoint.wl"
        } else {
            "shared/gate.wl"
        };
        if moved {
            fs::create_dir(copied_root.join("parts")).unwrap();
            fs::rename(
                copied_root.join("shared/gate.wl"),
                copied_root.join(fragment_path),
            )
            .unwrap();
            let entry = fs::read_to_string(copied_root.join("world.wl")).unwrap();
            fs::write(
                copied_root.join("world.wl"),
                entry.replace("shared/gate.wl", fragment_path),
            )
            .unwrap();
        }
        let mut copy = Project::open(&copied_root).unwrap();
        let changed = copy.compile();
        assert!(!changed.has_errors(), "{:?}", changed.diagnostics);
        assert_eq!(original.analysis.fingerprint, changed.analysis.fingerprint);
        for trace in &traces {
            assert_replayed(&replay(&changed, trace), true);
        }
        let paused = replay(&changed, &paused_trace);
        assert_replayed(&paused, false);
        let expected_file = fragment_path
            .split('/')
            .fold(copy.root.clone(), |path, component| path.join(component));
        assert_eq!(
            paused.current_state["calls"][0]["file"],
            expected_file.to_string_lossy().as_ref()
        );
        assert_eq!(paused.current_state["calls"][0]["line"], 3);
    }
}

#[test]
fn cooperative_replay_uses_the_same_location_comparison_after_each_resume() {
    let original = compile("original/story.wl", NESTED);
    let changed = compile("copied/story.wl", &format!("\n// 注释\n{NESTED}"));
    let original_trace = trace(&original, false, true);
    let mut session = ReplaySession::new(
        original_trace.clone(),
        ReplayBudget::new(1_000, 5_000),
        ReplayCancellation::new(),
    )
    .unwrap();
    let mut yields = 0;
    let result = loop {
        match session
            .advance(
                &changed.program,
                &changed.analysis,
                ReplayBudget::new(1, 5_000),
            )
            .unwrap()
        {
            Some(result) => break result,
            None => yields += 1,
        }
    };
    assert!(yields > 1);
    assert_replayed(&result, true);
    assert_eq!(result, replay(&changed, &original_trace));
}

#[test]
fn legacy_json_shapes_with_and_without_calls_need_no_location_migration() {
    for source in ["event start\n  choice \"继续\"\n    -> END\n", SINGLE] {
        let original = compile("old/story.wl", source);
        let changed = compile("new/story.wl", &format!("\n{source}"));
        let mut old_json = serde_json::to_value(trace(&original, false, true)).unwrap();
        // These optional presentation fields were absent in the original DTO.
        old_json["initial_observation"]
            .as_object_mut()
            .unwrap()
            .remove("choice_presentation");
        for step in old_json["steps"].as_array_mut().unwrap() {
            step["observation"]
                .as_object_mut()
                .unwrap()
                .remove("choice_presentation");
        }
        assert_eq!(old_json["schema_version"], 1);
        let has_calls = old_json["initial_observation"]["state"]
            .get("calls")
            .is_some();
        assert_eq!(has_calls, source == SINGLE);
        let loaded: ReplayTrace = serde_json::from_value(old_json).unwrap();
        assert_replayed(&replay(&changed, &loaded), true);
    }
}

#[test]
fn location_normalization_does_not_relax_checkpoint_versions_seed_or_fingerprint() {
    let original = compile("old/story.wl", NESTED);
    let changed = compile("new/story.wl", &format!("\n{NESTED}"));
    let original_trace = trace(&original, true, true);
    let ReplayOrigin::Checkpoint { checkpoint } = &original_trace.origin else {
        panic!("checkpoint origin required");
    };
    let original_snapshot = checkpoint.state.clone();
    assert_replayed(&replay(&changed, &original_trace), true);
    assert_eq!(checkpoint.state, original_snapshot);
    let semantic_change = compile("new/story.wl", &NESTED.replace("= 8", "= 9"));
    let error = ReplayTrace::replay(
        &semantic_change.program,
        &semantic_change.analysis,
        &original_trace,
        ReplayBudget::default(),
        &ReplayCancellation::new(),
    )
    .unwrap_err();
    assert!(error.message.contains("fingerprint"));
    for field in ["schema_version", "runtime_version", "fingerprint", "seed"] {
        let mut invalid = original_trace.clone();
        let ReplayOrigin::Checkpoint { checkpoint } = &mut invalid.origin else {
            unreachable!();
        };
        match field {
            "schema_version" => checkpoint.schema_version += 1,
            "runtime_version" => checkpoint.runtime_version = "unsupported".into(),
            "fingerprint" => checkpoint.fingerprint ^= 1,
            "seed" => checkpoint.seed += 1,
            _ => unreachable!(),
        }
        let error = ReplayTrace::replay(
            &changed.program,
            &changed.analysis,
            &invalid,
            ReplayBudget::default(),
            &ReplayCancellation::new(),
        )
        .unwrap_err();
        assert!(error.message.contains(field), "{field}: {}", error.message);
    }
}

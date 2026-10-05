use worldline_core::{compile_source, compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    generate_playthrough_report, PlaythroughReport, PlaythroughReportOptions,
    PlaythroughReportSession, ReplayBudget, ReplayCancellation, ReplayTrace, RouteStatus, Story,
};

const SOURCE: &str = "let secret = 7\nevent start\n  水库😀 <script>alert(1)</script> [点击](https://example.invalid)\n  choice \"留下\"\n    set secret = 7\n    归来\n    -> END\n  choice \"离开\"\n    -> END\n";
fn compile(source: &str) -> CompileResult {
    let result = compile_source("/private/machine/story/world.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn trace(snapshot: &CompileResult) -> ReplayTrace {
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    story.continue_story().unwrap();
    if !story.is_ended() {
        story.choose(0).unwrap();
        story.continue_story().unwrap();
    }
    story.replay_trace()
}
fn report(snapshot: &CompileResult, trace: &ReplayTrace) -> PlaythroughReport {
    generate_playthrough_report(
        snapshot,
        trace,
        Default::default(),
        &ReplayCancellation::new(),
    )
    .unwrap()
}
#[test]
fn report_uses_real_outputs_sources_and_counts_without_private_variables() {
    let snapshot = compile(SOURCE);
    let trace = trace(&snapshot);
    let result = report(&snapshot, &trace);
    assert_eq!(result.status, RouteStatus::Replayed);
    assert!(result.complete && result.ended);
    assert_eq!(result.observations.len(), 2);
    assert_eq!(result.observations[1].variable_writes, 1);
    assert_eq!(
        result.observations[0].texts[0]
            .source
            .as_ref()
            .unwrap()
            .line,
        3
    );
    assert_eq!(
        result.observations[1].texts[0]
            .source
            .as_ref()
            .unwrap()
            .line,
        6
    );
    assert_eq!(
        result.observations[1]
            .choice
            .as_ref()
            .unwrap()
            .source
            .as_ref()
            .unwrap()
            .line,
        4
    );
    assert!(result.generated_at_unix_ms.is_some());
    let json = serde_json::to_string(&result).unwrap();
    assert!(!json.contains("/private/machine"));
    assert!(!json.contains("secret"));
    assert_eq!(result.source_manifest[0].file, "world.wl");
    assert!(!result.markdown.contains("<script>"));
    assert!(!result.markdown.contains("[点击](https://"));
    assert!(result.markdown.contains("&lt;script&gt;"));
    assert!(result.markdown.contains("未探索"));
    assert_eq!(
        serde_json::from_str::<PlaythroughReport>(&json).unwrap(),
        result
    );
}
#[test]
fn tampered_observation_is_not_formatted_as_verified() {
    let snapshot = compile(SOURCE);
    let mut original = trace(&snapshot);
    original.steps[0].observation.as_mut().unwrap().outputs[0]["content"] =
        "UNTRUSTED_SPOOF".into();
    let result = report(&snapshot, &original);
    assert_eq!(result.status, RouteStatus::Diverged);
    assert!(!result.complete);
    assert_eq!(result.observations.len(), 1);
    assert!(!result.markdown.contains("UNTRUSTED_SPOOF"));
    assert!(!result.markdown.contains("归来"));
    assert_eq!(result.pending_choice.unwrap().label, "留下");
    original.initial_observation.as_mut().unwrap().outputs[0]["content"] = "INITIAL_SPOOF".into();
    let result = report(&snapshot, &original);
    assert!(result.observations.is_empty());
    assert!(!result.markdown.contains("INITIAL_SPOOF"));
}
#[test]
fn untrusted_input_choice_metadata_cannot_replace_actual_label_or_source() {
    let snapshot = compile(SOURCE);
    let mut trace = trace(&snapshot);
    trace.steps[0].choice.label = "SPOOF_LABEL".into();
    trace.steps[0].choice.line = 99999;
    trace.steps[0].choice.node = "/private/path".into();
    let result = report(&snapshot, &trace);
    assert_eq!(result.status, RouteStatus::Replayed);
    let choice = result.observations[1].choice.as_ref().unwrap();
    assert_eq!(choice.label, "留下");
    assert_eq!(choice.source.as_ref().unwrap().line, 4);
    assert!(!result.markdown.contains("SPOOF_LABEL"));
}
#[test]
fn partial_checkpoint_and_missing_observation_are_explicit() {
    let snapshot = compile(SOURCE);
    let mut story = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 42).unwrap();
    story.continue_story().unwrap();
    let partial = report(&snapshot, &story.replay_trace());
    assert_eq!(partial.status, RouteStatus::Replayed);
    assert!(!partial.complete && !partial.ended);
    story.start_trace_from_here().unwrap();
    story.choose(0).unwrap();
    let missing = report(&snapshot, &story.replay_trace());
    assert_eq!(missing.status, RouteStatus::IncompleteTrace);
    assert_eq!(missing.origin.kind, "checkpoint");
    assert_eq!(missing.inherited_visited_nodes, 1);
    assert!(missing.pending_choice.is_some());
    assert!(!missing.markdown.contains("水库"));
    story.continue_story().unwrap();
    let checkpoint = report(&snapshot, &story.replay_trace());
    assert!(checkpoint.complete && checkpoint.ended);
    assert_eq!(checkpoint.observations[1].variable_writes, 1);
    assert!(checkpoint.markdown.contains("检查点之前"));
    let mut missing = trace(&snapshot);
    missing.initial_observation = None;
    let result = report(&snapshot, &missing);
    assert_eq!(result.status, RouteStatus::IncompleteTrace);
    assert_eq!(result.executed_steps, 0);
    assert!(result.observations.is_empty());
}
#[test]
fn report_is_cooperative_and_never_changes_live_source_trace_save_or_rng() {
    let snapshot = compile(SOURCE);
    let original = trace(&snapshot);
    let mut live = Story::new_with_seed(&snapshot.program, &snapshot.analysis, 77).unwrap();
    live.continue_story().unwrap();
    let before = (
        live.save().unwrap(),
        live.replay_trace(),
        snapshot.sources.clone(),
        original.clone(),
    );
    let mut session = PlaythroughReportSession::new(
        &snapshot,
        original.clone(),
        Default::default(),
        ReplayCancellation::new(),
    )
    .unwrap();
    let actual = loop {
        if let Some(result) = session
            .advance(&snapshot, ReplayBudget::new(1, 1000))
            .unwrap()
        {
            break result;
        }
    };
    let expected = report(&snapshot, &original);
    assert_eq!(actual.observations, expected.observations);
    assert_eq!(actual.executed_steps, expected.executed_steps);
    assert_eq!(
        before,
        (
            live.save().unwrap(),
            live.replay_trace(),
            snapshot.sources.clone(),
            original
        )
    );
    assert_eq!(
        session
            .advance(&snapshot, ReplayBudget::default())
            .unwrap_err()
            .code,
        "session_finished"
    );
}
#[test]
fn bounded_inputs_cancel_zero_budgets_and_changed_snapshot_fail_honestly() {
    let snapshot = compile(SOURCE);
    let original = trace(&snapshot);
    let token = ReplayCancellation::new();
    token.cancel();
    let result =
        generate_playthrough_report(&snapshot, &original, Default::default(), &token).unwrap();
    assert_eq!(result.status, RouteStatus::Cancelled);
    assert!(result.observations.is_empty());
    for (budget, status) in [
        (ReplayBudget::new(0, 1000), RouteStatus::StepBudgetExceeded),
        (ReplayBudget::new(1000, 0), RouteStatus::TimeBudgetExceeded),
    ] {
        let result = generate_playthrough_report(
            &snapshot,
            &original,
            PlaythroughReportOptions {
                budget,
                ..Default::default()
            },
            &ReplayCancellation::new(),
        )
        .unwrap();
        assert_eq!(result.status, status);
        assert_eq!(result.executed_steps, 0);
    }
    for options in [
        PlaythroughReportOptions {
            max_trace_steps: 0,
            ..Default::default()
        },
        PlaythroughReportOptions {
            max_trace_bytes: 0,
            ..Default::default()
        },
    ] {
        assert_eq!(
            generate_playthrough_report(&snapshot, &original, options, &ReplayCancellation::new())
                .unwrap_err()
                .code,
            "input_limit"
        );
    }
    let options = PlaythroughReportOptions {
        max_output_bytes: 0,
        ..Default::default()
    };
    assert_eq!(
        generate_playthrough_report(&snapshot, &original, options, &ReplayCancellation::new())
            .unwrap_err()
            .code,
        "output_limit"
    );
    let mut session = PlaythroughReportSession::new(
        &snapshot,
        original.clone(),
        Default::default(),
        ReplayCancellation::new(),
    )
    .unwrap();
    let changed = compile(&(SOURCE.to_string() + "// changed\n"));
    assert_eq!(
        session
            .advance(&changed, ReplayBudget::default())
            .unwrap_err()
            .code,
        "snapshot_changed"
    );
    let mut old = original;
    old.runtime_version = "0.1.0".into();
    assert_eq!(
        generate_playthrough_report(
            &snapshot,
            &old,
            Default::default(),
            &ReplayCancellation::new()
        )
        .unwrap_err()
        .code,
        "invalid_trace"
    );
}
#[test]
fn speaker_display_is_from_current_snapshot_and_fragment_sources_are_real() {
    let source = "character keeper as \"守门人 <b>\"\nfragment greeting()\n  say keeper \"你好😀\"\n  return\nevent start\n  call greeting()\n  -> END\n";
    let snapshot = compile_source_with_options("speakers.wl", source, CompileOptions::v1_11());
    assert!(!snapshot.has_errors(), "{:?}", snapshot.diagnostics);
    let result = report(&snapshot, &trace(&snapshot));
    let text = &result.observations[0].texts[0];
    assert_eq!(text.speaker_label.as_deref(), Some("守门人 <b>"));
    assert_eq!(text.source.as_ref().unwrap().line, 3);
    assert!(!result.markdown.contains("<b>"));
}

#[test]
fn physical_included_sources_and_ambiguous_lines_never_fall_back_to_event_file() {
    use std::{collections::BTreeMap, path::PathBuf};
    let root = PathBuf::from("/private/machine/included");
    let sources = BTreeMap::from([
        (
            root.join("world.wl"),
            "event start\ninclude \"parts/body.wl\"\n".into(),
        ),
        (
            root.join("parts/body.wl"),
            "  第一行\n  choice \"继续\"\n    下一行\n    -> END\n".into(),
        ),
    ]);
    let snapshot = worldline_core::compile_sources(&root.join("world.wl"), &sources);
    assert!(!snapshot.has_errors(), "{:?}", snapshot.diagnostics);
    let result = report(&snapshot, &trace(&snapshot));
    assert_eq!(
        result.observations[0].texts[0]
            .source
            .as_ref()
            .unwrap()
            .file,
        "parts/body.wl"
    );
    assert_eq!(
        result.observations[1]
            .choice
            .as_ref()
            .unwrap()
            .source
            .as_ref()
            .unwrap()
            .file,
        "parts/body.wl"
    );
    assert_eq!(
        result.observations[1].texts[0]
            .source
            .as_ref()
            .unwrap()
            .line,
        3
    );

    let sources = BTreeMap::from([
        (
            root.join("world.wl"),
            "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n".into(),
        ),
        (root.join("a.wl"), "  来自甲\n".into()),
        (root.join("b.wl"), "  来自乙\n".into()),
    ]);
    let snapshot = worldline_core::compile_sources(&root.join("world.wl"), &sources);
    assert!(!snapshot.has_errors(), "{:?}", snapshot.diagnostics);
    let result = report(&snapshot, &trace(&snapshot));
    assert!(result.observations[0]
        .texts
        .iter()
        .all(|text| text.source.is_none()));
    assert!(result.markdown.contains("来源不可用"));
}
#[test]
fn long_unicode_and_glued_text_are_bounded_and_keep_real_content() {
    let text = "长中文😀".repeat(2000);
    let snapshot = compile(&format!("event start\n  {text}~\n  后半句\n  -> END\n"));
    let trace = trace(&snapshot);
    let result = report(&snapshot, &trace);
    assert_eq!(result.observations[0].texts[0].content, text);
    assert!(!result.observations[0].texts[1].new_line);
    assert!(result.markdown.contains(&format!("{text}后半句")));
    let limited = PlaythroughReportOptions {
        max_output_bytes: 4096,
        ..Default::default()
    };
    assert_eq!(
        generate_playthrough_report(&snapshot, &trace, limited, &ReplayCancellation::new())
            .unwrap_err()
            .code,
        "output_limit"
    );
}
#[test]
fn cancellation_after_yield_and_post_start_option_changes_cannot_show_success() {
    let snapshot = compile(SOURCE);
    let trace = trace(&snapshot);
    let token = ReplayCancellation::new();
    let mut session =
        PlaythroughReportSession::new(&snapshot, trace.clone(), Default::default(), token.clone())
            .unwrap();
    assert!(session
        .advance(&snapshot, ReplayBudget::new(1, 1000))
        .unwrap()
        .is_none());
    token.cancel();
    let result = session
        .advance(&snapshot, ReplayBudget::new(1000, 1000))
        .unwrap()
        .unwrap();
    assert_eq!(result.status, RouteStatus::Cancelled);
    assert!(!result.complete);
    let mut session = PlaythroughReportSession::new(
        &snapshot,
        trace,
        Default::default(),
        ReplayCancellation::new(),
    )
    .unwrap();
    let mut altered = compile(SOURCE);
    altered.options.object_refs = true;
    assert_eq!(
        session
            .advance(&altered, ReplayBudget::default())
            .unwrap_err()
            .code,
        "snapshot_changed"
    );
}

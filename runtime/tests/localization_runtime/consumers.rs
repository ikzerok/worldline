use super::*;
use worldline_runtime::{
    compare_routes, compare_routes_with_presentation, generate_playthrough_report,
    generate_playthrough_report_with_presentation, PlaythroughReportOptions, ReplayBudget,
    ReplayCancellation, RouteComparisonOptions, RouteStatus,
};

#[test]
fn locale_routes_and_reports_use_bound_snapshot_and_show_actual_translated_choice() {
    let mut f = localized_fixture("route-report");
    let c = f.project.compile();
    let presentation = f.presentation(Policy::Strict);
    let mut story =
        Story::new_with_presentation(&c.program, &c.analysis, 18, &presentation).unwrap();
    story.continue_story().unwrap();
    let displayed = story.choices()[0].label.clone();
    let id = story.choices()[0].id.clone();
    story.choose_id(&id).unwrap();
    story.continue_story().unwrap();
    let trace = story.replay_trace();
    let cancellation = ReplayCancellation::new();
    assert!(compare_routes(
        &c,
        &trace,
        &trace,
        RouteComparisonOptions::default(),
        &cancellation
    )
    .is_err());
    assert!(generate_playthrough_report(
        &c,
        &trace,
        PlaythroughReportOptions::default(),
        &cancellation
    )
    .is_err());
    let comparison = compare_routes_with_presentation(
        &c,
        &trace,
        &trace,
        RouteComparisonOptions::default(),
        &cancellation,
        &presentation,
    )
    .unwrap();
    assert_eq!(comparison.left.status, RouteStatus::Replayed);
    assert_eq!(comparison.right.status, RouteStatus::Replayed);
    assert!(comparison.state_differences.is_empty());
    assert_eq!(
        comparison.presentation.as_ref(),
        story.presentation_identity()
    );
    let report = generate_playthrough_report_with_presentation(
        &c,
        &trace,
        PlaythroughReportOptions::default(),
        &cancellation,
        &presentation,
    )
    .unwrap();
    assert_eq!(report.status, RouteStatus::Replayed);
    assert_eq!(
        report.observations[1].choice.as_ref().unwrap().label,
        displayed
    );
    assert_eq!(report.presentation.as_ref(), story.presentation_identity());
    assert!(report.markdown.contains("zh"));
    assert!(report.observations[0].texts[0].content.starts_with("译 "));
}

#[test]
fn draft_runtime_uses_actual_draft_revisions_and_keeps_author_project_untouched() {
    use worldline_core::draft_rehearsal::DraftRehearsalRequest;
    use worldline_runtime::draft_rehearsal::DraftRehearsal;
    let mut f = localized_fixture("draft");
    let baseline = f.project.content_baseline();
    let mut buffer = f
        .project
        .open_source_writing_buffer(&f.root.join("world.wl"))
        .unwrap();
    buffer.replace_source(SOURCE.replace("Done {total}", "Draft done {total}"));
    let request =
        DraftRehearsalRequest::from_writing_buffers(&f.project, &[buffer], vec![], false).unwrap();
    let draft = f.project.compile_draft_rehearsal(&request).unwrap();
    assert!(f
        .project
        .prepare_draft_localization_presentation(&draft, &super::request(Policy::Strict),)
        .is_err());
    let presentation = f
        .project
        .prepare_draft_localization_presentation(&draft, &super::request(Policy::SourceFallback))
        .unwrap();
    let mut session = DraftRehearsal::new_with_presentation(draft, 31, &presentation).unwrap();
    let first = session
        .continue_bounded(ReplayBudget::default(), &ReplayCancellation::new())
        .unwrap();
    assert!(
        matches!(&first.outputs[0], Output::Text { content, localization:Some(_), .. } if content.starts_with("译 "))
    );
    assert_eq!(
        session
            .presentation_identity()
            .unwrap()
            .request
            .target_locale,
        "zh-Hant"
    );
    let id = session
        .choice_presentations()
        .iter()
        .find(|choice| choice.enabled)
        .unwrap()
        .id
        .clone();
    session.choose_id(&id).unwrap();
    let last = session
        .continue_bounded(ReplayBudget::default(), &ReplayCancellation::new())
        .unwrap();
    assert!(last.outputs.iter().any(|output| matches!(output,
        Output::Text { localization: Some(metadata), content, .. }
            if metadata.status == LocalizationStatus::StaleSource && content == "Draft done 7")));
    assert_eq!(f.project.content_baseline(), baseline);
    assert_eq!(
        f.project.document(&f.root.join("world.wl")).unwrap(),
        SOURCE
    );
    let _ = f.project.compile();
}

#[test]
fn fallback_report_and_worker_metadata_preserve_status_without_extra_private_fields() {
    let mut f = fixture(
        "fallback-report",
        "event start\n  Source line\n  choice \"Continue\"\n    -> END\n",
        &[],
    );
    let c = f.project.compile();
    let presentation = f.presentation(Policy::SourceFallback);
    let mut story =
        Story::new_with_presentation(&c.program, &c.analysis, 11, &presentation).unwrap();
    let outputs = story.continue_story().unwrap();
    let output = serde_json::to_value(&outputs[0]).unwrap();
    assert!(output["localization"].get("source_links").is_none());
    let metadata: worldline_runtime::LocalizedPresentation =
        serde_json::from_value(output["localization"].clone()).unwrap();
    assert_eq!(metadata.status, LocalizationStatus::MissingId);
    let Output::Text {
        localization: Some(boxed),
        ..
    } = &outputs[0]
    else {
        panic!("localized text")
    };
    assert_eq!(
        output["localization"],
        serde_json::to_value(boxed.as_ref()).unwrap()
    );
    assert_eq!(
        output["localization"],
        serde_json::to_value(&metadata).unwrap()
    );
    let id = story.choices()[0].id.clone();
    story.choose_id(&id).unwrap();
    story.continue_story().unwrap();
    let report = generate_playthrough_report_with_presentation(
        &c,
        &story.replay_trace(),
        PlaythroughReportOptions::default(),
        &ReplayCancellation::new(),
        &presentation,
    )
    .unwrap();
    assert_eq!(
        report.observations[0].texts[0].localization_status,
        Some(LocalizationStatus::MissingId)
    );
    assert_eq!(
        report.observations[1]
            .choice
            .as_ref()
            .unwrap()
            .localization_status,
        Some(LocalizationStatus::MissingId)
    );
    assert!(report.markdown.contains("源文回退：缺少稳定ID"));
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(!encoded.contains("source_content"));
    assert!(!encoded.contains("source_links"));
    assert!(!story
        .replay_trace()
        .initial_observation
        .unwrap()
        .choice_presentation
        .is_empty());
}

#[test]
fn machine_locale_draft_checks_project_budget_before_compilation_without_restricting_source_only() {
    use worldline_core::{
        draft_rehearsal::DraftRehearsalRequest, localization::MAX_LOCALIZATION_TRACKED_FILES,
        project::Project,
    };
    use worldline_runtime::{
        draft_rehearsal::{run_draft_rehearsal, DraftRehearsalRunRequest},
        ContinuationOutcome, StateInspectionQuery,
    };
    let f = fixture("draft-machine-preflight", "event initial\n  -> END\n", &[]);
    let root = f.root.join("in-memory-only");
    let mut project = Project::new(&root);
    let applied = "event start\n  Source\n  -> END\n";
    project
        .set_text(&project.entry.clone(), applied.into())
        .unwrap();
    project.create_authoring_document(
        &root.join(".world/project.json"),
        br#"{"schema_version":1,"language_version":"1.13","required_features":["content.localization.v1"]}"#.to_vec(),
    ).unwrap();
    let mut document = project.documents[&project.entry].clone();
    document.text = "// retained budget fixture\n".into();
    while project.documents.len() + project.authoring_documents.len()
        <= MAX_LOCALIZATION_TRACKED_FILES
    {
        let index = project.documents.len();
        project
            .documents
            .insert(root.join(format!("tracked_{index}.wl")), document.clone());
    }
    assert_eq!(
        project.documents.len() + project.authoring_documents.len(),
        4097
    );
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source(applied.replace("Source", "Actual draft"));
    let mut run = DraftRehearsalRunRequest {
        presentation: Some(request(Policy::SourceFallback)),
        input: DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], vec![], false)
            .unwrap(),
        seed: 1,
        budget: ReplayBudget::new(100, 2000),
        choice_ids: vec![],
        inspection: StateInspectionQuery::default(),
    };
    std::fs::write(
        f.root.join("preflight-request.json"),
        serde_json::to_vec_pretty(&run).unwrap(),
    )
    .unwrap();
    let baseline = project.content_baseline();
    let budget_error = project.check_localization_budget().unwrap_err();
    assert_eq!(budget_error.code, "BUDGET_EXCEEDED");
    run.input.content_baseline = "intentionally-stale-but-structurally-valid".into();
    let rejected = run_draft_rehearsal(&project, &run, &ReplayCancellation::new()).unwrap();
    assert!(!rejected.ok);
    assert_eq!(
        rejected.error.as_deref(),
        Some(budget_error.message.as_str())
    );
    assert!(
        rejected.scope.is_none(),
        "no draft compile or snapshot was started"
    );
    assert!(rejected.state.is_none() && rejected.presentation.is_none());
    assert!(rejected.outputs.is_empty() && rejected.choices.is_empty());
    assert_eq!(rejected.executed_steps, 0);
    run.presentation = None;
    run.input.content_baseline = baseline.clone();
    let original_error = project.compile_draft_rehearsal(&run.input).err().unwrap();
    let source = run_draft_rehearsal(&project, &run, &ReplayCancellation::new()).unwrap();
    assert!(!source.ok);
    assert_eq!(source.error.as_deref(), Some(original_error.as_str()));
    assert!(original_error.contains("源码组织工作区超过 4096 文件预算"));
    assert!(source.scope.is_none() && source.presentation.is_none());
    assert_eq!(source.executed_steps, 0);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.document(&project.entry).unwrap(), applied);

    let mut small = Project::new(&f.root.join("source-only-small"));
    small
        .set_text(&small.entry.clone(), applied.into())
        .unwrap();
    let small_baseline = small.content_baseline();
    let mut buffer = small.open_source_writing_buffer(&small.entry).unwrap();
    buffer.replace_source(applied.replace("Source", "Actual draft"));
    run.input =
        DraftRehearsalRequest::from_writing_buffers(&small, &[buffer], vec![], false).unwrap();
    let source = run_draft_rehearsal(&small, &run, &ReplayCancellation::new()).unwrap();
    assert!(source.ok, "{:?}", source.error);
    assert_eq!(source.outcome, Some(ContinuationOutcome::Ended));
    assert!(
        matches!(&source.outputs[0], Output::Text { content, localization: None, .. } if content == "Actual draft")
    );
    assert!(source.presentation.is_none());
    assert_eq!(small.content_baseline(), small_baseline);
    assert_eq!(small.document(&small.entry).unwrap(), applied);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.document(&project.entry).unwrap(), applied);
    assert!(!root.exists());
}

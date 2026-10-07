use worldline_core::{
    catalog::TargetRef, manuscript::*, presentation_commands::Revision, project::Project,
};
use worldline_runtime::Story;

#[test]
fn new_chapter_source_changes_real_save_checkpoint_fingerprint_without_migrating_state() {
    let root = std::env::temp_dir().join(format!("wl-chapter-runtime-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut project = Project::new(&root);
    let before = project.compile();
    let story = Story::new_with_seed(&before.program, &before.analysis, 23).unwrap();
    let save = story.save().unwrap();
    let checkpoint = story.checkpoint().unwrap();
    let original_state = story.state_view();
    let mut revision = Revision::default();
    let mut request = ManuscriptChapterCreateRequest {
        schema_version: 1,
        expected_baseline: project.content_baseline(),
        expected_revision: revision,
        book: ManuscriptBookDestination::New {
            id: "book".into(),
            title: "书稿".into(),
        },
        chapter: ManuscriptChapterDraft {
            id: "one".into(),
            title: "第一章".into(),
            parent_section_id: None,
            after_sibling_id: None,
        },
        source: ManuscriptChapterSource::Existing {
            target: TargetRef::new("event", "start"),
        },
    };
    let plan = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    project
        .apply_manuscript_chapter_create(&mut revision, &request, &plan.plan_digest)
        .unwrap();
    let arranged = project.compile();
    assert_eq!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
    assert_eq!(
        Story::load(&arranged.program, &arranged.analysis, &save)
            .unwrap()
            .state_view(),
        original_state
    );
    assert!(Story::from_checkpoint(&arranged.program, &arranged.analysis, &checkpoint).is_ok());
    request.expected_baseline = project.content_baseline();
    request.expected_revision = revision;
    request.book = ManuscriptBookDestination::Existing { id: "book".into() };
    request.chapter.id = "two".into();
    request.source = ManuscriptChapterSource::NewEvent {
        id: "second".into(),
        storyline: "main".into(),
        destination: ManuscriptSourceDestination::ExistingActiveSource {
            relative_path: "world.wl".into(),
        },
    };
    let plan = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    assert_ne!(
        plan.runtime_fingerprint_before,
        plan.runtime_fingerprint_after
    );
    project
        .apply_manuscript_chapter_create(&mut revision, &request, &plan.plan_digest)
        .unwrap();
    let after = project.compile();
    assert_eq!(after.program.entry, before.program.entry);
    assert!(Story::load(&after.program, &after.analysis, &save).is_err());
    assert!(Story::from_checkpoint(&after.program, &after.analysis, &checkpoint).is_err());
    assert_eq!(story.save().unwrap(), save);
    assert_eq!(story.state_view(), original_state);
    assert!(!root.exists());
}

#[test]
fn tool_version_gate_is_independent_of_unchanged_source_fingerprint() {
    let content = worldline_core::compile_source("version.wl", "event start\n  -> END\n");
    let story = Story::new(&content.program, &content.analysis).unwrap();
    let save = story.save().unwrap();
    let value: serde_json::Value = serde_json::from_str(&save).unwrap();
    assert!(value.get("runtime_version").is_none());
    assert!(Story::load(&content.program, &content.analysis, &save).is_ok());
    let previous = if env!("CARGO_PKG_VERSION") == "0.29.0" {
        "0.28.0"
    } else {
        "0.29.0"
    };
    let mut checkpoint = story.checkpoint().unwrap();
    assert_eq!(checkpoint.fingerprint, content.analysis.fingerprint);
    checkpoint.runtime_version = previous.into();
    let error = match Story::from_checkpoint(&content.program, &content.analysis, &checkpoint) {
        Ok(_) => panic!("旧工具检查点不应绕过 runtime_version"),
        Err(error) => error,
    };
    assert!(error.message.contains("runtime_version"));
    let mut trace = story.replay_trace();
    trace.runtime_version = previous.into();
    let error = match worldline_runtime::ReplaySession::new(
        trace,
        Default::default(),
        worldline_runtime::ReplayCancellation::new(),
    ) {
        Ok(_) => panic!("旧工具路线不应绕过 runtime_version"),
        Err(error) => error,
    };
    assert!(error.message.contains("runtime_version"));
}

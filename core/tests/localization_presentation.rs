#![cfg(not(target_arch = "wasm32"))]
#[path = "localization_workbench/support.rs"]
mod support;
use support::*;
use worldline_core::localization::*;

#[test]
fn strict_snapshot_is_opaque_verified_and_does_not_change_the_project() {
    let mut fixture = fixture("strict", SOURCE, None);
    let request = presentation_request(LocalizationPresentationPolicy::Strict);
    assert_eq!(
        fixture
            .project
            .prepare_localization_presentation(&request)
            .unwrap_err()
            .code,
        "UNKNOWN_LOCALE"
    );
    let draft = edit(&fixture.project, &["greeting", "choice"]);
    apply(&mut fixture.project, &draft);
    let before = fixture.project.content_baseline();
    let on_disk = disk(&fixture.root);
    let snapshot = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    let compiled = fixture.project.clone().compile();
    snapshot
        .validate_program(&compiled.program, &compiled.analysis)
        .unwrap();
    assert_eq!(snapshot.entries().len(), 2);
    let entry = snapshot
        .find_entry(fixture.root.join("world.wl").to_str().unwrap(), 3, "text")
        .unwrap();
    assert_eq!(entry.id.as_deref(), Some("greeting"));
    assert_eq!(entry.status, LocalizationStatus::Translated);
    assert_eq!(
        entry.translation_pointer.as_deref(),
        Some("/entries/greeting/translation_parts")
    );
    assert_eq!(snapshot.target_locale(), "zh-Hant");
    assert_eq!(fixture.project.content_baseline(), before);
    assert_eq!(disk(&fixture.root), on_disk);
    assert!(!serde_json::to_string(&snapshot)
        .unwrap()
        .contains(fixture.root.to_str().unwrap()));
}

#[test]
fn fallback_keeps_truthful_missing_stale_invalid_and_duplicate_states() {
    let source = SOURCE.replace(
        "  -> END\nevent target",
        "  Missing identity\n  -> END\nevent target",
    );
    let mut fixture = fixture("fallback", &source, Some(sidecar(serde_json::json!({}))));
    let strict = presentation_request(LocalizationPresentationPolicy::Strict);
    assert_eq!(
        fixture
            .project
            .prepare_localization_presentation(&strict)
            .unwrap_err()
            .code,
        "INVALID_PRESENTATION"
    );
    let fallback = presentation_request(LocalizationPresentationPolicy::SourceFallback);
    let snapshot = fixture
        .project
        .prepare_localization_presentation(&fallback)
        .unwrap();
    assert!(snapshot
        .entries()
        .iter()
        .any(|entry| entry.status == LocalizationStatus::MissingId));
    assert!(snapshot
        .entries()
        .iter()
        .all(|entry| entry.translation_parts.is_none()));
    let draft = edit(&fixture.project, &["greeting"]);
    apply(&mut fixture.project, &draft);
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            source.replace("Hello", "Changed"),
        )
        .unwrap();
    let snapshot = fixture
        .project
        .prepare_localization_presentation(&fallback)
        .unwrap();
    assert_eq!(
        snapshot.entries()[0].status,
        LocalizationStatus::StaleSource
    );
    assert!(snapshot.entries()[0].translation_parts.is_none());
    let path = fixture.root.join(".world/localization/zh-Hant.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(fixture.project.authoring_document(&path).unwrap().bytes()).unwrap();
    value["entries"]["greeting"]["translation_parts"] =
        serde_json::json!([{"type":"placeholder","token":"unknown"}]);
    fixture
        .project
        .set_authoring_document(&path, serde_json::to_vec(&value).unwrap())
        .unwrap();
    let snapshot = fixture
        .project
        .prepare_localization_presentation(&fallback)
        .unwrap();
    assert_eq!(
        snapshot.entries()[0].status,
        LocalizationStatus::InvalidTranslation
    );
    assert!(snapshot.entries()[0].translation_parts.is_none());
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            source.replace("#wl-localization:choice", "#wl-localization:greeting"),
        )
        .unwrap();
    let snapshot = fixture
        .project
        .prepare_localization_presentation(&fallback)
        .unwrap();
    assert_eq!(
        snapshot.entries()[0].status,
        LocalizationStatus::DuplicateId
    );
}

#[test]
fn same_runtime_fingerprint_cannot_reuse_a_snapshot_for_moved_or_reidentified_source() {
    let mut fixture = fixture("identity", SOURCE, None);
    let draft = edit(&fixture.project, &["greeting", "choice"]);
    apply(&mut fixture.project, &draft);
    let request = presentation_request(LocalizationPresentationPolicy::Strict);
    let original = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            format!("// moved\n{SOURCE}"),
        )
        .unwrap();
    let moved = fixture.project.clone().compile();
    assert_eq!(
        original.program_fingerprint(),
        moved.analysis.fingerprint.to_string()
    );
    assert_eq!(
        original
            .validate_program(&moved.program, &moved.analysis)
            .unwrap_err()
            .code,
        "INVALID_PRESENTATION"
    );
    let fresh = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    assert_eq!(original.presentation_digest(), fresh.presentation_digest());
    assert_ne!(original.source_baseline(), fresh.source_baseline());
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            SOURCE.replace("#wl-localization:greeting", "#wl-localization:another"),
        )
        .unwrap();
    let renamed = fixture.project.clone().compile();
    assert_eq!(
        original.program_fingerprint(),
        renamed.analysis.fingerprint.to_string()
    );
    assert_eq!(
        original
            .validate_program(&renamed.program, &renamed.analysis)
            .unwrap_err()
            .code,
        "INVALID_PRESENTATION"
    );
}

#[test]
fn changing_translation_changes_presentation_identity_only() {
    let mut fixture = fixture("display-identity", SOURCE, None);
    let mut draft = edit(&fixture.project, &["greeting", "choice"]);
    apply(&mut fixture.project, &draft);
    let request = presentation_request(LocalizationPresentationPolicy::Strict);
    let first = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    draft.edits[0].translation_parts.insert(
        0,
        LocalizationPart::Text {
            text: "新译文".into(),
        },
    );
    apply(&mut fixture.project, &draft);
    let second = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    assert_eq!(first.program_fingerprint(), second.program_fingerprint());
    assert_eq!(first.source_baseline(), second.source_baseline());
    assert_ne!(first.presentation_digest(), second.presentation_digest());
}

#[test]
fn draft_presentation_uses_actual_unapplied_draft_and_keeps_all_input() {
    let mut fixture = fixture("draft", SOURCE, None);
    let draft = edit(&fixture.project, &["greeting", "choice"]);
    apply(&mut fixture.project, &draft);
    let mut buffer = fixture
        .project
        .open_source_writing_buffer(&fixture.root.join("world.wl"))
        .unwrap();
    buffer.replace_source(SOURCE.replace("Hello", "Unapplied source"));
    let request = worldline_core::draft_rehearsal::DraftRehearsalRequest::from_writing_buffers(
        &fixture.project,
        &[buffer.clone()],
        Vec::new(),
        false,
    )
    .unwrap();
    let rehearsal = fixture.project.compile_draft_rehearsal(&request).unwrap();
    let baseline = fixture.project.content_baseline();
    let request = presentation_request(LocalizationPresentationPolicy::Strict);
    assert_eq!(
        fixture
            .project
            .prepare_draft_localization_presentation(&rehearsal, &request)
            .unwrap_err()
            .code,
        "INVALID_PRESENTATION"
    );
    let request = presentation_request(LocalizationPresentationPolicy::SourceFallback);
    let snapshot = fixture
        .project
        .prepare_draft_localization_presentation(&rehearsal, &request)
        .unwrap();
    assert_eq!(
        snapshot.entries()[0].status,
        LocalizationStatus::StaleSource
    );
    let hit = rehearsal
        .localization_source(&snapshot.entries()[0].source)
        .unwrap();
    assert!(hit.draft);
    assert!(hit.preview.contains("Unapplied source"));
    assert_eq!(&buffer.source()[hit.range], hit.preview);
    let mut forged = snapshot.entries()[0].source.clone();
    forged.kind = "say".into();
    assert!(rehearsal.localization_source(&forged).is_err());
    snapshot
        .validate_program(
            &rehearsal.compiled().program,
            &rehearsal.compiled().analysis,
        )
        .unwrap();
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(buffer.source().contains("Unapplied source"));
    assert_eq!(
        fixture
            .project
            .document(&fixture.root.join("world.wl"))
            .unwrap(),
        SOURCE
    );
}

#[test]
fn source_file_reordering_and_other_locale_edits_do_not_change_presentation_content() {
    let mut fixture = fixture(
        "file-order",
        "event start\n  Entry #wl-localization:entry\n  -> END\n",
        None,
    );
    std::fs::write(
        fixture.root.join("a.wl"),
        "event first\n  First #wl-localization:first\n  -> END\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("b.wl"),
        "event second\n  Second #wl-localization:second\n  -> END\n",
    )
    .unwrap();
    fixture.project = worldline_core::project::Project::open(&fixture.root).unwrap();
    let draft = edit(&fixture.project, &["entry", "first", "second"]);
    apply(&mut fixture.project, &draft);
    fixture.project.save().unwrap();
    let request = presentation_request(LocalizationPresentationPolicy::Strict);
    let original = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    std::fs::rename(fixture.root.join("a.wl"), fixture.root.join("z.wl")).unwrap();
    fixture.project = worldline_core::project::Project::open(&fixture.root).unwrap();
    let moved = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    assert_eq!(original.presentation_digest(), moved.presentation_digest());
    assert_ne!(original.source_baseline(), moved.source_baseline());
    assert_ne!(
        original.entries()[0].source.file,
        moved.entries()[0].source.file
    );
    let mut unrelated = edit(&fixture.project, &["entry"]);
    unrelated.target_locale = "fr".into();
    apply(&mut fixture.project, &unrelated);
    let after = fixture
        .project
        .prepare_localization_presentation(&request)
        .unwrap();
    assert_eq!(moved.presentation_digest(), after.presentation_digest());
    assert_eq!(moved.program_fingerprint(), after.program_fingerprint());
    assert_eq!(moved.source_baseline(), after.source_baseline());
}

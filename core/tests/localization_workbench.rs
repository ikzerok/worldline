#![cfg(not(target_arch = "wasm32"))]
#[path = "localization_workbench/support.rs"]
mod support;
use std::fs;
use support::*;
use worldline_core::localization::*;
use worldline_core::project::Project;

#[test]
fn catalog_is_read_only_exact_paginated_and_bound_to_content() {
    let source = SOURCE.replace(
        "  -> END\nevent target",
        "  Unassigned 中文🙂\n  -> END\nevent target",
    );
    let fixture = fixture("catalog", &source, None);
    let before = disk(&fixture.root);
    let baseline = fixture.project.content_baseline();
    let mut request = query();
    request.limit = 1;
    let first = fixture
        .project
        .query_localization_catalog(&request)
        .unwrap();
    assert_eq!((first.all_total, first.total), (3, 3));
    assert_eq!(first.status_counts[&LocalizationStatus::MissingId], 1);
    assert_eq!(
        first.status_counts[&LocalizationStatus::MissingTranslation],
        2
    );
    assert_eq!(first.next_offset, Some(1));
    request.offset = 1;
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "STALE_QUERY"
    );
    request.expected_content_baseline = Some(first.content_baseline.clone());
    request.expected_source_baseline = Some(first.source_baseline.clone());
    let second = fixture
        .project
        .query_localization_catalog(&request)
        .unwrap();
    assert_ne!(first.entries[0].unit_key, second.entries[0].unit_key);
    let mut changed = fixture.project.clone();
    changed
        .set_text(
            &fixture.root.join("world.wl"),
            format!("// moved\n{source}"),
        )
        .unwrap();
    assert_eq!(
        changed
            .query_localization_catalog(&request)
            .unwrap_err()
            .code,
        "STALE_QUERY"
    );
    let mut exact = query();
    exact.string_ids = vec!["choice".into()];
    let filtered = fixture.project.query_localization_catalog(&exact).unwrap();
    assert_eq!((filtered.all_total, filtered.total), (3, 1));
    assert_eq!(filtered.entries[0].id.as_deref(), Some("choice"));
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(disk(&fixture.root), before);
}

#[test]
fn catalog_discovers_missing_ids_without_implicitly_enabling_feature() {
    let mut fixture = fixture("feature", "event start\n  Plain 中文🙂\n  -> END\n", None);
    let path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["required_features"] = serde_json::json!([]);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fixture.project = Project::open(&fixture.root).unwrap();
    let before = disk(&fixture.root);
    let current = page(&fixture.project);
    assert_eq!(current.entries[0].status, LocalizationStatus::MissingId);
    let draft = id_draft(&fixture.project, 2, "plain");
    assert_eq!(
        fixture
            .project
            .preview_localization_ids(&draft)
            .unwrap_err()
            .code,
        "FEATURE_REQUIRED"
    );
    assert_eq!(disk(&fixture.root), before);
}

#[test]
fn typed_candidate_is_one_undoable_memory_transaction_then_save_reopen() {
    let mut fixture = fixture("memory", SOURCE, None);
    let before = disk(&fixture.root);
    let undo = fixture.project.clone();
    let draft = edit(&fixture.project, &["greeting", "choice"]);
    let preview = fixture.project.preview_localization_edit(&draft).unwrap();
    assert!(preview.can_apply);
    assert_eq!(disk(&fixture.root), before, "preview performs no writes");
    let result = fixture
        .project
        .apply_localization_edit(&draft, &preview.plan_digest)
        .unwrap();
    assert_eq!(result.changed_files.len(), 2);
    assert!(fixture.project.is_dirty());
    assert_eq!(
        disk(&fixture.root),
        before,
        "candidate apply performs no writes"
    );
    assert!(page(&fixture.project)
        .entries
        .iter()
        .all(|entry| entry.status == LocalizationStatus::Translated));
    assert!(fixture
        .project
        .apply_localization_edit(&draft, &preview.plan_digest)
        .is_err());
    let redo = fixture.project.clone();
    assert!(fixture.project.restore(undo));
    assert!(page(&fixture.project)
        .entries
        .iter()
        .all(|entry| entry.status == LocalizationStatus::MissingTranslation));
    assert!(fixture.project.restore(redo));
    fixture.project.save().unwrap();
    let reopened = Project::open(&fixture.root).unwrap();
    assert!(page(&reopened)
        .entries
        .iter()
        .all(|entry| entry.status == LocalizationStatus::Translated));
    let no_change = reopened.preview_localization_edit(&draft).unwrap();
    assert!(!no_change.can_apply);
    assert_eq!(no_change.diagnostics[0].code, "NO_CHANGE");
}

#[test]
fn dirty_applied_source_is_supported_but_legacy_import_stays_clean_and_persisting() {
    let mut fixture = fixture("dirty", SOURCE, None);
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            SOURCE.replace("Hello", "Hello again"),
        )
        .unwrap();
    let selection = LocalizationSelection {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hant".into(),
        string_ids: vec!["greeting".into()],
    };
    let mut exchange = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap()
        .exchange;
    exchange.entries[0].translation_parts = Some(translate(&exchange.entries[0].source_parts));
    let legacy = fixture
        .project
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(!legacy.can_apply);
    assert!(legacy.diagnostics.iter().any(|d| d.code == "DIRTY_PROJECT"));
    let before = disk(&fixture.root);
    let candidate = fixture
        .project
        .preview_localization_import_candidate(&selection, &exchange)
        .unwrap();
    assert!(candidate.can_apply);
    fixture
        .project
        .apply_localization_import_candidate(&selection, &exchange, &candidate.plan_digest)
        .unwrap();
    assert_eq!(disk(&fixture.root), before);
    fixture.project.save().unwrap();
    exchange.entries[0]
        .translation_parts
        .as_mut()
        .unwrap()
        .insert(
            0,
            LocalizationPart::Text {
                text: "旧API立即保存".into(),
            },
        );
    let legacy = fixture
        .project
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(legacy.can_apply);
    fixture
        .project
        .apply_localization_import(&selection, &exchange, &legacy.plan_digest)
        .unwrap();
    assert!(!fixture.project.is_dirty());
    assert!(
        fs::read_to_string(fixture.root.join(".world/localization/zh-Hant.json"))
            .unwrap()
            .contains("旧API立即保存")
    );
}

#[test]
fn token_errors_stale_edits_and_unselected_extensions_are_atomic() {
    let mut fixture = fixture(
        "preserve",
        SOURCE,
        Some(sidecar(serde_json::json!({
            "orphan":{"source_revision":"old","translation_parts":[{"type":"text","text":"未选译文"}],"x_entry":42}
        }))),
    );
    let baseline = fixture.project.content_baseline();
    let before = disk(&fixture.root);
    let mut bad = edit(&fixture.project, &["greeting"]);
    bad.edits[0].translation_parts = vec![
        LocalizationPart::Placeholder { token: "p0".into() },
        LocalizationPart::Placeholder { token: "p0".into() },
    ];
    let plan = fixture.project.preview_localization_edit(&bad).unwrap();
    assert!(!plan.can_apply);
    assert!(plan.diagnostics.iter().any(|d| d.code == "INVALID_TOKEN"));
    assert!(fixture
        .project
        .apply_localization_edit(&bad, &plan.plan_digest)
        .is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(disk(&fixture.root), before);
    let draft = edit(&fixture.project, &["greeting"]);
    apply(&mut fixture.project, &draft);
    let sidecar_path = fixture.root.join(".world/localization/zh-Hant.json");
    let value: serde_json::Value = serde_json::from_slice(
        fixture
            .project
            .authoring_document(&sidecar_path)
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(value["entries"]["orphan"]["x_entry"], 42);
    assert_eq!(value["x_sidecar"]["keep"], true);
    let state = page(&fixture.project);
    assert_eq!(
        state.status_counts[&LocalizationStatus::OrphanTranslation],
        1
    );
    let orphan = state
        .entries
        .iter()
        .find(|entry| entry.status == LocalizationStatus::OrphanTranslation)
        .unwrap();
    assert!(orphan.source.is_none() && orphan.source_revision.is_none());
    fixture
        .project
        .set_text(
            &fixture.root.join("world.wl"),
            SOURCE.replace("Hello", "Changed"),
        )
        .unwrap();
    assert_eq!(
        page(&fixture.project).entries[0].status,
        LocalizationStatus::StaleSource
    );
    assert!(
        !fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap()
            .can_apply
    );
    let reviewed = edit(&fixture.project, &["greeting"]);
    apply(&mut fixture.project, &reviewed);
    assert_eq!(
        page(&fixture.project).entries[0].status,
        LocalizationStatus::Translated
    );
}

#[test]
fn id_assignment_preserves_comments_crlf_unicode_and_runtime_identity() {
    let source = "event start\r\n  中文🙂 // 保留🌊\r\n  Other #tag /* 尾注 */\r\n  -> END\r\n";
    let mut fixture = fixture("ids", source, None);
    let before = disk(&fixture.root);
    let original = fixture.project.clone().compile().analysis.fingerprint;
    let undo = fixture.project.clone();
    let draft = id_draft(&fixture.project, 2, "hello");
    let preview = fixture.project.preview_localization_ids(&draft).unwrap();
    assert!(preview.can_apply, "{:?}", preview.diagnostics);
    assert_eq!(preview.changes.len(), 1);
    assert!(preview.changes[0]
        .after
        .contains("中文🙂 #wl-localization:hello // 保留🌊\r\n"));
    fixture
        .project
        .apply_localization_ids(&draft, &preview.plan_digest)
        .unwrap();
    assert_eq!(
        fixture.project.clone().compile().analysis.fingerprint,
        original
    );
    assert_eq!(disk(&fixture.root), before);
    let second = id_draft(&fixture.project, 3, "other");
    let plan = fixture.project.preview_localization_ids(&second).unwrap();
    fixture
        .project
        .apply_localization_ids(&second, &plan.plan_digest)
        .unwrap();
    assert!(fixture
        .project
        .document(&fixture.root.join("world.wl"))
        .unwrap()
        .contains("Other #tag #wl-localization:other /* 尾注 */\r\n"));
    assert!(fixture.project.restore(undo));
    assert_eq!(
        fixture
            .project
            .document(&fixture.root.join("world.wl"))
            .unwrap(),
        source
    );
}

#[test]
fn duplicate_id_requires_explicit_statement_and_never_moves_old_translations() {
    let source = "event start\n  First #wl-localization:duplicate\n  Second #wl-localization:duplicate\n  -> END\n";
    let mut fixture = fixture(
        "duplicate-id",
        source,
        Some(sidecar(
            serde_json::json!({"duplicate":{"source_revision":"old","translation_parts":[]}}),
        )),
    );
    let state = page(&fixture.project);
    assert_eq!(state.status_counts[&LocalizationStatus::DuplicateId], 2);
    let mut draft = id_draft(&fixture.project, 3, "second");
    let preview = fixture.project.preview_localization_ids(&draft).unwrap();
    assert!(preview.can_apply);
    fixture
        .project
        .apply_localization_ids(&draft, &preview.plan_digest)
        .unwrap();
    let text = fixture
        .project
        .document(&fixture.root.join("world.wl"))
        .unwrap();
    assert!(text.contains("First #wl-localization:duplicate"));
    assert!(text.contains("Second #wl-localization:second"));
    assert!(fixture
        .project
        .apply_localization_ids(&draft, &preview.plan_digest)
        .is_err());
    draft = id_draft(&fixture.project, 3, "duplicate");
    let invalid = fixture.project.preview_localization_ids(&draft).unwrap();
    assert!(!invalid.can_apply);
    assert!(invalid.diagnostics.iter().any(|d| d.code == "DUPLICATE_ID"));
    let value: serde_json::Value = serde_json::from_slice(
        fixture
            .project
            .authoring_document(&fixture.root.join(".world/localization/zh-Hant.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert!(value["entries"].get("second").is_none());
}

#[test]
fn external_changes_and_unknown_sidecar_versions_keep_all_buffers() {
    let mut fixture = fixture("external", SOURCE, None);
    let draft = edit(&fixture.project, &["greeting"]);
    let plan = fixture.project.preview_localization_edit(&draft).unwrap();
    let baseline = fixture.project.content_baseline();
    fs::write(
        fixture.root.join("world.wl"),
        SOURCE.replace("Hello", "Externally changed"),
    )
    .unwrap();
    let before = disk(&fixture.root);
    assert_eq!(
        fixture
            .project
            .apply_localization_edit(&draft, &plan.plan_digest)
            .unwrap_err()
            .code,
        "EXTERNAL_CONFLICT"
    );
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(disk(&fixture.root), before);
    let mut unknown = sidecar(serde_json::json!({}));
    unknown["schema_version"] = 99.into();
    let fixture = support::fixture("readonly", SOURCE, Some(unknown));
    let state = page(&fixture.project);
    assert!(state.read_only);
    assert_eq!(
        state.entries[0].status,
        LocalizationStatus::InvalidTranslation
    );
    let draft = edit(&fixture.project, &["greeting"]);
    let result = fixture.project.preview_localization_edit(&draft);
    assert!(result.is_err() || !result.unwrap().can_apply);
}

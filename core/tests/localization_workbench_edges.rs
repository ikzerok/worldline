#![cfg(not(target_arch = "wasm32"))]
#[path = "localization_workbench/support.rs"]
mod support;
use support::*;
use worldline_core::localization::*;
use worldline_core::project::Project;

#[test]
fn selected_entry_extensions_and_manifest_raw_unselected_fields_survive_edit() {
    let mut fixture = fixture(
        "entry-extension",
        SOURCE,
        Some(sidecar(serde_json::json!({
            "greeting":{"source_revision":"old","translation_parts":[],"review_vendor":{"notes":"保留🙂","number":17}},
            "never_selected":{"source_revision":"old","translation_parts":[],"raw_extension":[null,true,"🌊"]}
        }))),
    );
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
    assert_eq!(
        value["entries"]["greeting"]["review_vendor"],
        serde_json::json!({"notes":"保留🙂","number":17})
    );
    assert_eq!(
        value["entries"]["never_selected"]["raw_extension"],
        serde_json::json!([null, true, "🌊"])
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        fixture
            .project
            .authoring_document(&fixture.root.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(manifest["x_manifest"]["sentinel"], "keep");
}

#[test]
fn explicit_invalid_unknown_or_duplicate_selection_never_mutates_buffers() {
    let mut fixture = fixture("selection-errors", SOURCE, None);
    let mut draft = edit(&fixture.project, &["greeting"]);
    draft.edits[0].id = "not_present".into();
    assert_eq!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap_err()
            .code,
        "UNKNOWN_ID"
    );
    draft.edits[0].id = "greeting".into();
    draft.edits.push(draft.edits[0].clone());
    assert_eq!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap_err()
            .code,
        "DUPLICATE_ID"
    );
    let draft = id_draft(&fixture.project, 3, "invalid.id");
    assert_eq!(
        fixture
            .project
            .preview_localization_ids(&draft)
            .unwrap_err()
            .code,
        "INVALID_ID"
    );
    let draft = edit(&fixture.project, &["greeting"]);
    let plan = fixture.project.preview_localization_edit(&draft).unwrap();
    let baseline = fixture.project.content_baseline();
    assert_eq!(
        fixture
            .project
            .apply_localization_edit(&draft, "forged_digest")
            .unwrap_err()
            .code,
        "STALE_PLAN"
    );
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(plan.can_apply);
}

#[test]
fn brand_new_unsaved_workspace_can_enable_assign_and_translate_without_creating_a_directory() {
    let root = std::env::temp_dir().join(format!(
        "worldline-localization-uncreated-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut project = Project::new(&root);
    project
        .set_text(
            &project.entry.clone(),
            "event start\n  Hello 中文🙂\n  -> END\n".into(),
        )
        .unwrap();
    assert!(!root.exists());
    let request = worldline_core::capabilities::CapabilityEnableRequest {
        target_language: worldline_core::LanguageVersion::V1_10,
        enable_features: vec![LOCALIZATION_REQUIRED_FEATURE.into()],
        expected_baseline: project.content_baseline(),
    };
    let enable = project.plan_capability_enable(&request).unwrap();
    assert!(enable.can_apply);
    project.apply_capability_enable(&enable).unwrap();
    let id = id_draft(&project, 2, "hello");
    let id_plan = project.preview_localization_ids(&id).unwrap();
    project
        .apply_localization_ids(&id, &id_plan.plan_digest)
        .unwrap();
    let draft = edit(&project, &["hello"]);
    let before = project.clone();
    apply(&mut project, &draft);
    assert!(
        !root.exists(),
        "neither manifest nor sidecar preparation may create a directory"
    );
    assert_eq!(
        page(&project).entries[0].status,
        LocalizationStatus::Translated
    );
    assert!(project.restore(before));
    assert!(!root.exists());
    assert_eq!(
        page(&project).entries[0].status,
        LocalizationStatus::MissingTranslation
    );
}

#[test]
fn new_sidecar_that_would_exceed_document_budget_is_rejected_during_preview() {
    let mut fixture = fixture("projected-document-count", SOURCE, None);
    let mut empty = Project::new(&fixture.root)
        .documents
        .values()
        .next()
        .unwrap()
        .clone();
    empty.text.clear();
    for index in 0..MAX_LOCALIZATION_TRACKED_FILES - 2 {
        fixture.project.documents.insert(
            fixture.root.join(format!("empty_{index}.wl")),
            empty.clone(),
        );
    }
    assert_eq!(
        fixture.project.documents.len() + fixture.project.authoring_documents.len(),
        MAX_LOCALIZATION_TRACKED_FILES
    );
    fixture.project.check_localization_budget().unwrap();
    let draft = edit(&fixture.project, &["greeting"]);
    let baseline = fixture.project.content_baseline();
    let before = disk(&fixture.root);
    assert_eq!(
        fixture
            .project
            .preview_localization_edit(&draft)
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(disk(&fixture.root), before);
    assert_eq!(fixture.project.authoring_documents.len(), 1);
}

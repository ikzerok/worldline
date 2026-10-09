//! These tests exercise the real wasm32-only mounted-file backend when run by a WASM test runner.
#![cfg(target_arch = "wasm32")]
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use worldline_core::localization::*;
use worldline_core::project::Project;

fn mounted() -> (Project, BTreeMap<PathBuf, Vec<u8>>) {
    let root = Path::new("/localization-wasm-fixture");
    let files = BTreeMap::from([
        (PathBuf::from("world.wl"), b"event start\n  Hello #wl-localization:hello\n  -> END\n".to_vec()),
        (PathBuf::from(".world/project.json"), br#"{"schema_version":1,"language_version":"1.13","required_features":["content.localization.v1"]}"#.to_vec()),
    ]);
    worldline_core::file_access::mount(
        files
            .iter()
            .map(|(path, bytes)| (root.join(path), bytes.clone()))
            .collect(),
    );
    (
        Project::from_snapshot(root, Path::new("world.wl"), &files).unwrap(),
        files,
    )
}

#[test]
fn wasm_candidate_and_undo_never_modify_mounted_file_bytes() {
    let (mut project, files) = mounted();
    let query = LocalizationCatalogQuery {
        target_locale: Some("zh".into()),
        ..Default::default()
    };
    let page = project.query_localization_catalog(&query).unwrap();
    let edit = LocalizationEditDraft {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh".into(),
        source_baseline: page.source_baseline,
        edits: vec![LocalizationEdit {
            id: "hello".into(),
            source_revision: page.entries[0].source_revision.clone().unwrap(),
            translation_parts: vec![LocalizationPart::Text {
                text: "你好🙂".into(),
            }],
        }],
    };
    let plan = project.preview_localization_edit(&edit).unwrap();
    assert!(plan.can_apply);
    let undo = project.clone();
    project
        .apply_localization_edit(&edit, &plan.plan_digest)
        .unwrap();
    assert!(project.is_dirty());
    assert_eq!(
        project.query_localization_catalog(&query).unwrap().entries[0].status,
        LocalizationStatus::Translated
    );
    for (path, bytes) in &files {
        assert_eq!(
            worldline_core::file_access::read(project.root.join(path)).unwrap(),
            *bytes
        );
    }
    assert!(
        worldline_core::file_access::read(project.root.join(".world/localization/zh.json"))
            .is_err()
    );
    assert!(project.restore(undo));
    assert_eq!(
        project.query_localization_catalog(&query).unwrap().entries[0].status,
        LocalizationStatus::MissingTranslation
    );
    assert!(
        worldline_core::file_access::read(project.root.join(".world/localization/zh.json"))
            .is_err()
    );
}

#[test]
fn wasm_remounted_external_source_rejects_old_plan_with_zero_mutation() {
    let (mut project, mut files) = mounted();
    let query = LocalizationCatalogQuery::default();
    let page = project.query_localization_catalog(&query).unwrap();
    let draft = LocalizationIdDraft {
        schema_version: 1,
        source_baseline: page.source_baseline,
        assignments: vec![LocalizationIdAssignment {
            source: page.entries[0].source.clone().unwrap(),
            source_revision: page.entries[0].source_revision.clone().unwrap(),
            expected_id: Some("hello".into()),
            id: "renamed".into(),
        }],
    };
    let plan = project.preview_localization_ids(&draft).unwrap();
    let baseline = project.content_baseline();
    files.insert(
        PathBuf::from("world.wl"),
        b"event start\n  External #wl-localization:hello\n  -> END\n".to_vec(),
    );
    worldline_core::file_access::mount(
        files
            .iter()
            .map(|(path, bytes)| (project.root.join(path), bytes.clone()))
            .collect(),
    );
    assert_eq!(
        project
            .apply_localization_ids(&draft, &plan.plan_digest)
            .unwrap_err()
            .code,
        "EXTERNAL_CONFLICT"
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        worldline_core::file_access::read(project.root.join("world.wl")).unwrap(),
        files[Path::new("world.wl")]
    );
}

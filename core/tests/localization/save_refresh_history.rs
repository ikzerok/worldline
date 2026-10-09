//! Own-save refresh preserves deletion baselines without weakening external guards.
use super::*;

fn saved_locale_undo(name: &str) -> (Fixture, Project, Vec<u8>, Vec<u8>) {
    let mut work = fixture(name, SOURCE, false);
    let before = work.project.clone();
    let original_manifest = fs::read(work.root.join(".world/project.json")).unwrap();
    let mut selection = selection(&["welcome"]);
    selection.target_locale = "fr".into();
    let exported = work
        .project
        .preview_localization_export(&selection)
        .unwrap();
    let exchange = translated(exported.exchange);
    let plan = work
        .project
        .preview_localization_import_candidate(&selection, &exchange)
        .unwrap();
    assert!(plan.can_apply, "{:?}", plan.diagnostics);
    work.project
        .apply_localization_import_candidate(&selection, &exchange, &plan.plan_digest)
        .unwrap();
    let sidecar = work.root.join(".world/localization/fr.json");
    assert!(
        !sidecar.exists(),
        "candidate application must stay in memory"
    );
    work.project.save().unwrap();
    let translation = fs::read(&sidecar).unwrap();
    let redo = work.project.clone();
    assert!(work.project.restore(before));
    assert!(work
        .project
        .authoring_document(&sidecar)
        .unwrap()
        .is_deleted());
    work.project.save().unwrap();
    assert!(!sidecar.exists());
    assert!(!work.project.is_dirty());
    assert_eq!(
        fs::read(work.root.join(".world/project.json")).unwrap(),
        original_manifest
    );
    assert!(!work
        .project
        .authoring_document(&sidecar)
        .unwrap()
        .is_dirty());
    (work, redo, translation, original_manifest)
}

#[test]
fn own_saved_locale_deletion_refresh_preserves_content_and_restore_generation() {
    let (mut work, _, _, _) = saved_locale_undo("own-delete-poll");
    let sidecar = work.root.join(".world/localization/fr.json");
    let before_poll = work.project.clone();
    let baseline = work.project.content_baseline();
    assert!(work.project.refresh().unwrap().is_empty());
    // Public restore is the authoritative generation probe; do not expose or bypass it.
    let mut guard_probe = work.project.clone();
    assert!(guard_probe.restore(before_poll));
    assert_eq!(
        work.project.content_baseline(),
        baseline,
        "observing our own completed deletion must not change authoring content"
    );
    let tombstone = work.project.authoring_document(&sidecar).unwrap();
    assert!(tombstone.is_deleted());
    assert!(!tombstone.is_dirty());
}

#[test]
fn redo_after_own_saved_locale_deletion_refresh_recreates_missing_sidecar() {
    let (mut work, redo, translation, _) = saved_locale_undo("redo-delete-poll");
    let sidecar = work.root.join(".world/localization/fr.json");
    assert!(work.project.refresh().unwrap().is_empty());
    assert!(work.project.restore(redo));
    let restored = work.project.authoring_document(&sidecar).unwrap();
    assert!(!restored.is_deleted());
    assert_eq!(restored.bytes(), translation);
    assert!(
        restored.is_dirty(),
        "redo must use the current absent disk baseline, not its old saved Some(bytes)"
    );
    work.project.save().unwrap();
    assert_eq!(fs::read(&sidecar).unwrap(), translation);
    let reopened = Project::open(&work.root).unwrap();
    assert_eq!(
        reopened.authoring_document(&sidecar).unwrap().bytes(),
        translation
    );
    assert!(!reopened.is_dirty());
}

#[test]
fn external_recreation_of_unregistered_deleted_locale_stays_ordinary_and_rejects_history() {
    let (mut work, redo, _, _) = saved_locale_undo("external-recreate");
    let sidecar = work.root.join(".world/localization/fr.json");
    let external = br#"{"schema_version":1,"outside":"do not adopt or overwrite"}"#;
    fs::write(&sidecar, external).unwrap();
    assert!(work.project.refresh().unwrap().is_empty());
    assert!(
        !work.project.authoring_documents.contains_key(&sidecar),
        "unregistered external bytes must not be resurrected as an authoring document"
    );
    let current = work.project.content_baseline();
    assert!(!work.project.restore(redo));
    assert_eq!(work.project.content_baseline(), current);
    work.project.save().unwrap();
    assert_eq!(fs::read(&sidecar).unwrap(), external);
}

#[test]
fn genuine_external_source_edit_still_rejects_old_locale_history() {
    let (mut work, redo, _, _) = saved_locale_undo("external-source");
    let source = work.root.join("world.wl");
    let external = format!("{SOURCE}\n// genuine external edit\n");
    fs::write(&source, &external).unwrap();
    assert!(work.project.refresh().unwrap().is_empty());
    let current = work.project.content_baseline();
    assert!(!work.project.restore(redo));
    assert_eq!(work.project.content_baseline(), current);
    assert_eq!(work.project.document(&source).unwrap(), external);
    assert_eq!(fs::read_to_string(&source).unwrap(), external);
}

#[test]
fn external_locale_unregistration_unloads_clean_live_document_without_deleting_disk() {
    let (mut work, redo, translation, original_manifest) = saved_locale_undo("external-unregister");
    let sidecar = work.root.join(".world/localization/fr.json");
    assert!(work.project.restore(redo));
    work.project.save().unwrap();
    let prior = work.project.clone();
    fs::write(work.root.join(".world/project.json"), original_manifest).unwrap();
    assert!(work.project.refresh().unwrap().is_empty());
    assert!(!work.project.authoring_documents.contains_key(&sidecar));
    let current = work.project.content_baseline();
    assert!(!work.project.restore(prior));
    assert_eq!(work.project.content_baseline(), current);
    work.project.save().unwrap();
    assert_eq!(fs::read(&sidecar).unwrap(), translation);
}

#[test]
fn local_locale_unregistration_still_unloads_clean_live_document_on_refresh() {
    let (mut work, redo, translation, original_manifest) = saved_locale_undo("local-unregister");
    let sidecar = work.root.join(".world/localization/fr.json");
    assert!(work.project.restore(redo));
    work.project.save().unwrap();
    work.project
        .set_authoring_document(&work.root.join(".world/project.json"), original_manifest)
        .unwrap();
    work.project.save().unwrap();
    assert!(work.project.refresh().unwrap().is_empty());
    assert!(!work.project.authoring_documents.contains_key(&sidecar));
    assert_eq!(fs::read(&sidecar).unwrap(), translation);
}

#[test]
fn external_recreation_with_explicit_locale_registration_loads_clean_but_rejects_history() {
    let (mut work, redo, translation, _) = saved_locale_undo("external-reregister");
    let sidecar = work.root.join(".world/localization/fr.json");
    let manifest = work.root.join(".world/project.json");
    let registered_manifest = redo.authoring_document(&manifest).unwrap().bytes().to_vec();
    fs::write(&sidecar, &translation).unwrap();
    fs::write(&manifest, registered_manifest).unwrap();
    assert!(work.project.refresh().unwrap().is_empty());
    let loaded = work.project.authoring_document(&sidecar).unwrap();
    assert!(!loaded.is_deleted());
    assert!(!loaded.is_dirty());
    assert_eq!(loaded.bytes(), translation);
    let current = work.project.content_baseline();
    assert!(!work.project.restore(redo));
    assert_eq!(work.project.content_baseline(), current);
    assert_eq!(fs::read(&sidecar).unwrap(), translation);
}

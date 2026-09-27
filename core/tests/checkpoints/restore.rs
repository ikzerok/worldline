use super::support::{default_limits, TempWorkspace};
use std::fs;
use std::path::Path;
#[test]
fn checkpoint_restore_replaces_all_files_and_can_be_undone_as_a_draft() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let original_source = project.document(&source).unwrap().to_owned();
    let original_map = fs::read(workspace.path().join(".world/maps/raw.json")).unwrap();
    let original_note = fs::read(workspace.path().join("notes.bin")).unwrap();
    let checkpoint = project
        .create_checkpoint(Some("恢复点".into()), default_limits())
        .unwrap();

    project
        .set_text(&source, "event start\n  当前稿。\n  -> END\n".into())
        .unwrap();
    let added = project.add_file(Path::new("extra.wl")).unwrap();
    project
        .delete_authoring_document(&workspace.path().join(".world/maps/raw.json"))
        .unwrap();
    fs::write(workspace.path().join("notes.bin"), b"changed bytes").unwrap();
    let undo = project.clone();
    let undo_source = undo.document(&source).unwrap().to_owned();
    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let result = project.restore_checkpoint(&plan).unwrap();

    assert_eq!(project.document(&source).unwrap(), original_source);
    assert!(project.documents[&added].is_deleted());
    assert_eq!(
        fs::read(workspace.path().join(".world/maps/raw.json")).unwrap(),
        original_map
    );
    assert_eq!(
        fs::read(workspace.path().join("notes.bin")).unwrap(),
        original_note
    );
    assert_eq!(result.restored_files, plan.changes.len());
    assert_ne!(result.fingerprint_before, result.fingerprint_after);
    assert!(project.restore(undo));
    assert_eq!(project.document(&source).unwrap(), undo_source);
    assert!(project.is_dirty());
    assert_eq!(
        fs::read(source).unwrap(),
        original_source.as_bytes(),
        "undo restores the in-memory draft; saving remains explicit"
    );
}

#[test]
fn checkpoint_restore_rejects_stale_drafts_and_external_changes_without_writes() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    project
        .set_text(&source, "event start\n  用户新稿。\n  -> END\n".into())
        .unwrap();
    let draft = project.document(&source).unwrap().to_owned();
    let disk_before = fs::read(&source).unwrap();
    assert!(project.restore_checkpoint(&plan).is_err());
    assert_eq!(project.document(&source).unwrap(), draft);
    assert_eq!(fs::read(&source).unwrap(), disk_before);

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let external = "event start\n  外部修改。\n  -> END\n".as_bytes();
    fs::write(&source, external).unwrap();
    assert!(project.restore_checkpoint(&plan).is_err());
    assert_eq!(project.document(&source).unwrap(), draft);
    assert_eq!(fs::read(&source).unwrap(), external);
}
#[test]
fn checkpoint_restore_preserves_crlf_and_unknown_raw_bytes() {
    let workspace = TempWorkspace::new();
    let source = workspace.path().join("world.wl");
    let original_source = "event start\r\n  CRLF 原稿。\r\n  -> END\r\n".as_bytes();
    fs::write(&source, original_source).unwrap();
    let mut project = workspace.open();
    let original_map = fs::read(workspace.path().join(".world/maps/raw.json")).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project
        .set_text(
            &source,
            "event start\r\n  临时改稿。\r\n  -> END\r\n".into(),
        )
        .unwrap();
    project
        .delete_authoring_document(&workspace.path().join(".world/maps/raw.json"))
        .unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    project.restore_checkpoint(&plan).unwrap();

    assert_eq!(fs::read(&source).unwrap(), original_source);
    assert_eq!(
        project.document(&source).unwrap().as_bytes(),
        original_source
    );
    assert_eq!(
        fs::read(workspace.path().join(".world/maps/raw.json")).unwrap(),
        original_map
    );
}
#[test]
fn checkpoint_restore_plan_rejects_mutation_before_any_write() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project
        .set_text(&source, "event start\n  当前稿。\n  -> END\n".into())
        .unwrap();
    let mut plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    plan.expected_content_baseline = "forged".into();
    let draft = project.document(&source).unwrap().to_owned();
    let disk = fs::read(&source).unwrap();

    assert!(project.restore_checkpoint(&plan).is_err());
    assert_eq!(project.document(&source).unwrap(), draft);
    assert_eq!(fs::read(source).unwrap(), disk);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn legacy_checkpoint_remains_restorable_without_fabricating_a_text_base() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let checkpoint_text = project.document(&source).unwrap().to_owned();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    let manifest_path = workspace
        .path()
        .join(".world/.checkpoints/v1")
        .join(&checkpoint.id)
        .join("checkpoint.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = serde_json::json!(1);
    manifest.as_object_mut().unwrap().remove("text_base");
    manifest.as_object_mut().unwrap().remove("text_base_digest");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    project
        .set_text(&source, "event start\n  当前草稿。\n  -> END\n".into())
        .unwrap();
    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let diff = plan
        .text_differences
        .iter()
        .find(|diff| diff.path == Path::new("world.wl"))
        .unwrap();
    assert!(!diff.base_available);
    assert!(diff.alignment_uncertain);
    assert_eq!(diff.raw.base, None);
    assert_eq!(
        diff.raw.current.as_deref(),
        Some("event start\n  当前草稿。\n  -> END\n")
    );
    assert_eq!(
        diff.raw.checkpoint.as_deref(),
        Some(checkpoint_text.as_str())
    );

    project.restore_checkpoint(&plan).unwrap();
    assert_eq!(project.document(&source).unwrap(), checkpoint_text);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn readonly_restore_target_is_rejected_before_any_file_or_buffer_write() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let original_disk = fs::read(&source).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project
        .set_text(&source, "event start\n  当前稿。\n  -> END\n".into())
        .unwrap();
    let draft = project.document(&source).unwrap().to_owned();
    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let original_permissions = fs::metadata(&source).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&source, permissions).unwrap();

    assert!(project.restore_checkpoint(&plan).is_err());
    assert_eq!(project.document(&source).unwrap(), draft);
    assert_eq!(fs::read(&source).unwrap(), original_disk);

    fs::set_permissions(&source, original_permissions).unwrap();
}

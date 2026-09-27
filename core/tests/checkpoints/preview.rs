use super::support::{default_limits, TempWorkspace};
#[cfg(not(target_arch = "wasm32"))]
use super::support::{test_checksum, test_files_digest};
use std::fs;
use std::path::{Path, PathBuf};
use worldline_core::catalog::TargetRef;
use worldline_core::project::CheckpointFileOperation;
#[test]
fn checkpoint_preview_lists_add_delete_modify_and_object_impacts() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let checkpoint = project
        .create_checkpoint(Some("原稿".into()), default_limits())
        .unwrap();
    let source = project.entry.clone();
    let modified = "event start\n  并行改写正文。\n  -> END\n";
    project.set_text(&source, modified.into()).unwrap();
    let added = project.add_file(Path::new("chapters/new.wl")).unwrap();
    project
        .set_text(
            &added,
            "event checkpoint_candidate_object\n  新章节对象。\n  -> END\n".into(),
        )
        .unwrap();
    project
        .delete_authoring_document(&workspace.path().join(".world/maps/raw.json"))
        .unwrap();
    fs::remove_file(workspace.path().join("notes.bin")).unwrap();
    fs::write(workspace.path().join("new.bin"), [9, 8, 7]).unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();

    assert!(plan.changes.iter().any(|change| {
        change.path == Path::new("world.wl")
            && change.operation == CheckpointFileOperation::Modified
            && change
                .affected_objects
                .contains(&TargetRef::new("event", "start"))
    }));
    assert!(plan.changes.iter().any(|change| {
        change.path == Path::new("chapters/new.wl")
            && change.operation == CheckpointFileOperation::Deleted
            && change
                .affected_objects
                .contains(&TargetRef::new("event", "checkpoint_candidate_object"))
    }));
    assert!(plan.changes.iter().any(|change| {
        change.path == Path::new(".world/maps/raw.json")
            && change.operation == CheckpointFileOperation::Added
    }));
    assert!(plan.changes.iter().any(|change| {
        change.path == Path::new("notes.bin") && change.operation == CheckpointFileOperation::Added
    }));
    assert!(plan.changes.iter().any(|change| {
        change.path == Path::new("new.bin") && change.operation == CheckpointFileOperation::Deleted
    }));
    assert!(plan.changes.iter().all(|change| change.objects_complete));
    assert_eq!(
        project.document(&added).unwrap(),
        "event checkpoint_candidate_object\n  新章节对象。\n  -> END\n"
    );
}
#[test]
fn checkpoint_object_impacts_are_explicitly_incomplete_when_a_candidate_has_errors() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project
        .set_text(&source, "this draft has a syntax error\n".into())
        .unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();

    let source_change = plan
        .changes
        .iter()
        .find(|change| change.path == Path::new("world.wl"))
        .unwrap();
    assert!(!source_change.objects_complete);
}

#[test]
fn checkpoint_restore_preview_projects_base_current_and_checkpoint_text() {
    let workspace = TempWorkspace::new();
    let source = workspace.path().join("world.wl");
    let base = "alpha α\r\n\r\nstable\r\n";
    let checkpoint_text = "checkpoint α\r\n\r\nstable\r\n";
    let current_text = "current α\r\n\r\nstable\r\n";
    fs::write(&source, base.as_bytes()).unwrap();
    let mut project = workspace.open();
    project.set_text(&source, checkpoint_text.into()).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project.set_text(&source, current_text.into()).unwrap();
    project.save().unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();

    assert!(!plan.expected_content_baseline.is_empty());
    assert!(!plan.expected_workspace_digest.is_empty());
    assert!(!plan.expected_disk_digest.is_empty());
    assert!(!plan.checkpoint_digest.is_empty());
    let diff = plan
        .text_differences
        .iter()
        .find(|diff| diff.path == Path::new("world.wl"))
        .unwrap();
    assert!(diff.base_available);
    assert!(!diff.alignment_uncertain);
    assert!(!diff.undecodable);
    assert!(!diff.truncated);
    assert_eq!(diff.raw.base.as_deref(), Some(base));
    assert_eq!(diff.raw.current.as_deref(), Some(current_text));
    assert_eq!(diff.raw.checkpoint.as_deref(), Some(checkpoint_text));
    assert_eq!(diff.summary.base_lines, Some(3));
    assert_eq!(diff.summary.current_lines, Some(3));
    assert_eq!(diff.summary.checkpoint_lines, Some(3));
    assert_eq!(diff.summary.difference_count, 1);
    let changed_paragraph = diff.differences.first().unwrap();
    assert_eq!(changed_paragraph.base.as_deref(), Some("alpha α\r\n\r\n"));
    assert_eq!(
        changed_paragraph.current.as_deref(),
        Some("current α\r\n\r\n")
    );
    assert_eq!(
        changed_paragraph.checkpoint.as_deref(),
        Some("checkpoint α\r\n\r\n")
    );
    let base_range = changed_paragraph.base_range.as_ref().unwrap();
    assert_eq!(
        &base[base_range.start_byte..base_range.end_byte],
        "alpha α\r\n\r\n"
    );
}

#[test]
fn checkpoint_text_projection_aligns_inserted_and_deleted_paragraphs() {
    let workspace = TempWorkspace::new();
    let source = workspace.path().join("world.wl");
    let base = "one\r\n\r\nbase paragraph\r\n\r\nlast\r\n";
    let checkpoint_text = "one\r\n\r\nlast\r\n";
    let current_text = "one\r\n\r\ninserted paragraph\r\n\r\nbase paragraph\r\n\r\nlast\r\n";
    fs::write(&source, base.as_bytes()).unwrap();
    let mut project = workspace.open();
    project.set_text(&source, checkpoint_text.into()).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project.set_text(&source, current_text.into()).unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let diff = plan
        .text_differences
        .iter()
        .find(|diff| diff.path == Path::new("world.wl"))
        .unwrap();
    assert!(!diff.alignment_uncertain);
    assert!(diff.differences.len() >= 2);
    assert!(diff.differences.iter().any(|change| {
        change.base.is_none()
            && change.current.as_deref() == Some("inserted paragraph\r\n\r\n")
            && change.checkpoint.is_none()
    }));
    assert!(diff.differences.iter().any(|change| {
        change.base.as_deref() == Some("base paragraph\r\n\r\n")
            && change.current.as_deref() == Some("base paragraph\r\n\r\n")
            && change.checkpoint.is_none()
    }));
}
#[test]
fn untracked_source_attachment_does_not_claim_an_absent_saved_base() {
    let workspace = TempWorkspace::new();
    let project = workspace.open();
    let extra = workspace.path().join("extra.wl");
    let checkpoint_text = "event extra\n  检查点附件。\n  -> END\n";
    let current_text = "event extra\n  当前附件。\n  -> END\n";
    fs::write(&extra, checkpoint_text).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    fs::write(&extra, current_text).unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let diff = plan
        .text_differences
        .iter()
        .find(|diff| diff.path == Path::new("extra.wl"))
        .unwrap();
    assert!(!diff.base_available);
    assert!(diff.alignment_uncertain);
    assert_eq!(diff.raw.base, None);
    assert_eq!(diff.raw.current.as_deref(), Some(current_text));
    assert_eq!(diff.raw.checkpoint.as_deref(), Some(checkpoint_text));
}

#[test]
fn tracked_new_source_records_a_known_absent_persistence_base() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.add_file(Path::new("new.wl")).unwrap();
    let checkpoint_text = "event new\n  检查点新增稿。\n  -> END\n";
    project.set_text(&source, checkpoint_text.into()).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project
        .set_text(&source, "event new\n  当前新增稿。\n  -> END\n".into())
        .unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let diff = plan
        .text_differences
        .iter()
        .find(|diff| diff.path == Path::new("new.wl"))
        .unwrap();
    assert!(diff.base_available);
    assert_eq!(diff.summary.base_lines, Some(0));
    assert_eq!(diff.raw.base, None);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn undecodable_checkpoint_text_uses_a_bounded_byte_fallback_without_ranges() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let checkpoint_text = "event start\n  检查点正文。\n  -> END\n";
    project.set_text(&source, checkpoint_text.into()).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project
        .set_text(&source, "event start\n  当前草稿。\n  -> END\n".into())
        .unwrap();

    let record = workspace
        .path()
        .join(".world/.checkpoints/v1")
        .join(&checkpoint.id);
    let manifest_path = record.join("checkpoint.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let entry = manifest["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["path"] == "world.wl")
        .unwrap();
    let payload = entry["payload"].as_str().unwrap().to_owned();
    let payload_path = record.join(payload);
    let mut bytes = fs::read(&payload_path).unwrap();
    bytes[0] = 0xff;
    fs::write(&payload_path, &bytes).unwrap();
    entry["bytes"] = serde_json::json!(bytes.len());
    entry["checksum"] = serde_json::json!(test_checksum(&bytes));

    let mut files = std::collections::BTreeMap::new();
    for entry in manifest["files"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap().to_owned();
        let payload = entry["payload"].as_str().unwrap();
        files.insert(path, fs::read(record.join(payload)).unwrap());
    }
    manifest["snapshot_digest"] = serde_json::json!(test_files_digest(&files));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let diff = plan
        .text_differences
        .iter()
        .find(|diff| diff.path == Path::new("world.wl"))
        .unwrap();
    assert!(diff.base_available);
    assert!(diff.undecodable);
    assert!(diff.alignment_uncertain);
    assert!(diff
        .raw
        .checkpoint
        .as_deref()
        .unwrap()
        .starts_with("hex:ff"));
    assert!(diff.differences.is_empty());
    assert!(diff
        .differences
        .iter()
        .all(|difference| difference.checkpoint_range.is_none()));
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn checkpoint_text_base_digest_rejects_a_rechecksummed_baseline_change() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    project
        .set_text(
            &source,
            "event start\n  不同于已保存基线。\n  -> END\n".into(),
        )
        .unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    let draft = project.document(&source).unwrap().to_owned();
    let disk_before = fs::read(&source).unwrap();
    let record = workspace
        .path()
        .join(".world/.checkpoints/v1")
        .join(&checkpoint.id);
    let manifest_path = record.join("checkpoint.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    let entry = manifest["text_base"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["path"] == "world.wl" && entry["source"]["storage"] == "stored")
        .unwrap();
    let payload = entry["source"]["payload"].as_str().unwrap().to_owned();
    let payload_path = record.join(payload);
    let mut bytes = fs::read(&payload_path).unwrap();
    bytes[0] ^= 1;
    fs::write(&payload_path, &bytes).unwrap();
    entry["source"]["checksum"] = serde_json::json!(test_checksum(&bytes));
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let records = project.list_checkpoints().unwrap();
    assert_eq!(records.len(), 1);
    assert!(!records[0].available);
    assert!(project.preview_checkpoint_restore(&checkpoint.id).is_err());
    assert_eq!(project.document(&source).unwrap(), draft);
    assert_eq!(fs::read(&source).unwrap(), disk_before);
}
#[test]
fn checkpoint_text_projection_is_bounded_and_marks_expired_or_tampered_plans() {
    let workspace = TempWorkspace::new();
    let source = workspace.path().join("world.wl");
    let mut base = format!("{}\r\n\r\n", "b".repeat(18_000));
    let mut checkpoint_text = format!("{}\r\n\r\n", "k".repeat(18_000));
    let mut current_text = format!("{}\r\n\r\n", "c".repeat(18_000));
    for index in 0..300 {
        base.push_str(&format!("base-{index:03}\r\n\r\n"));
        checkpoint_text.push_str(&format!("checkpoint-{index:03}\r\n\r\n"));
        current_text.push_str(&format!("current-{index:03}\r\n\r\n"));
    }
    fs::write(&source, base.as_bytes()).unwrap();
    let mut project = workspace.open();
    project.set_text(&source, checkpoint_text).unwrap();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    project.set_text(&source, current_text.clone()).unwrap();

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    let diff = plan
        .text_differences
        .iter()
        .find(|diff| diff.path == Path::new("world.wl"))
        .unwrap();
    assert!(diff.truncated);
    assert!(diff.summary.difference_count <= 256);
    for snippet in [
        diff.raw.base.as_deref(),
        diff.raw.current.as_deref(),
        diff.raw.checkpoint.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        assert!(snippet.len() <= 16 * 1024);
        assert!(snippet.is_char_boundary(snippet.len()));
    }
    let hunk_bytes = diff
        .differences
        .iter()
        .flat_map(|difference| {
            [
                difference.base.as_deref(),
                difference.current.as_deref(),
                difference.checkpoint.as_deref(),
            ]
        })
        .flatten()
        .map(str::len)
        .sum::<usize>();
    assert!(
        hunk_bytes <= 16 * 1024,
        "hunk snippets used {hunk_bytes} bytes"
    );

    let mut changed = project.clone();
    changed
        .set_text(&source, "event start\n  已过期的新稿。\n  -> END\n".into())
        .unwrap();
    let disk_before = fs::read(&source).unwrap();
    assert!(changed.restore_checkpoint(&plan).is_err());
    assert_eq!(fs::read(&source).unwrap(), disk_before);

    let mut forged = plan.clone();
    forged.text_differences[0].raw.current = Some("被改写的展示投影".into());
    assert!(project.restore_checkpoint(&forged).is_err());
    assert_eq!(project.document(&source).unwrap(), current_text);
}

#[test]
fn checkpoint_text_projection_bounds_files_per_restore_plan() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let mut paths = Vec::new();
    for index in 0..40 {
        let relative = PathBuf::from(format!("extra-{index:02}.wl"));
        let path = project.add_file(&relative).unwrap();
        project
            .set_text(
                &path,
                format!("event extra_{index}\n  第 {index} 个检查点版本。\n  -> END\n"),
            )
            .unwrap();
        paths.push(path);
    }
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    for (index, path) in paths.iter().enumerate() {
        project
            .set_text(
                path,
                format!("event extra_{index}\n  第 {index} 个当前版本。\n  -> END\n"),
            )
            .unwrap();
    }

    let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();
    assert_eq!(plan.text_differences.len(), 32);
    assert!(plan.text_differences_truncated);
}

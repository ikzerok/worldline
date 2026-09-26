use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::project::{CheckpointFileOperation, CheckpointLimits, Project};

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-checkpoints-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world/maps")).unwrap();
        fs::write(
            root.join("world.wl"),
            "event start\n  第一版正文。\n  -> END\n",
        )
        .unwrap();
        fs::write(
            root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"raw":".world/maps/raw.json"}}"#,
        )
        .unwrap();
        fs::write(root.join(".world/maps/raw.json"), br#"{"unknown":true}"#).unwrap();
        fs::write(root.join("notes.bin"), [0, 17, 255]).unwrap();
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn open(&self) -> Project {
        Project::open(self.path()).unwrap()
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn default_limits() -> CheckpointLimits {
    CheckpointLimits::default()
}

#[cfg(not(target_arch = "wasm32"))]
fn test_checksum(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(not(target_arch = "wasm32"))]
fn test_files_digest(files: &std::collections::BTreeMap<String, Vec<u8>>) -> String {
    fn mix(hash: &mut u64, bytes: &[u8]) {
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-files-v1");
    for (path, bytes) in files {
        mix(&mut hash, path.as_bytes());
        mix(&mut hash, bytes);
    }
    format!("{hash:016x}")
}

#[test]
fn checkpoint_captures_current_buffers_without_saving_or_entering_exports() {
    let workspace = TempWorkspace::new();
    let mut project = workspace.open();
    let source = project.entry.clone();
    let original_disk = fs::read(&source).unwrap();
    let draft = "event start\n  检查点中的未保存正文。\n  -> END\n";
    project.set_text(&source, draft.into()).unwrap();
    let baseline = project.content_baseline();
    let fingerprint = project.compile().analysis.fingerprint;

    let checkpoint = project
        .create_checkpoint(Some("审阅前".into()), default_limits())
        .unwrap();
    let listed = project.list_checkpoints().unwrap();

    assert_eq!(listed.len(), 1);
    assert!(listed[0].available);
    assert_eq!(listed[0].id, checkpoint.id);
    assert_eq!(listed[0].label.as_deref(), Some("审阅前"));
    assert_eq!(project.document(&source).unwrap(), draft);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    assert!(project.is_dirty());
    assert_eq!(fs::read(source).unwrap(), original_disk);
    assert!(!project
        .export_files()
        .unwrap()
        .keys()
        .any(|path| path.starts_with(".world/.checkpoints")));
}

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
fn checkpoint_quota_rejects_without_evicting_and_explicit_delete_reclaims_space() {
    let workspace = TempWorkspace::new();
    let project = workspace.open();
    let limits = CheckpointLimits {
        max_count: 1,
        max_checkpoint_bytes: 1024 * 1024,
        max_total_bytes: 1024 * 1024,
    };
    let checkpoint = project.create_checkpoint(None, limits.clone()).unwrap();
    assert!(project.create_checkpoint(None, limits).is_err());
    let too_small = CheckpointLimits {
        max_count: 1,
        max_checkpoint_bytes: 1,
        max_total_bytes: 1,
    };
    assert!(project.create_checkpoint(None, too_small).is_err());
    assert!(project
        .create_checkpoint(
            None,
            CheckpointLimits {
                max_count: 21,
                ..CheckpointLimits::default()
            }
        )
        .is_err());
    assert_eq!(project.list_checkpoints().unwrap().len(), 1);
    project.delete_checkpoint(&checkpoint.id).unwrap();
    assert!(project.list_checkpoints().unwrap().is_empty());
    assert!(project
        .create_checkpoint(None, CheckpointLimits::default())
        .is_ok());
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_publication_never_exposes_a_partial_checkpoint() {
    let workspace = TempWorkspace::new();
    let project = workspace.open();
    let history = workspace.path().join(".world/.checkpoints/v1");
    let _restore = CheckpointFailureEnvironment::capture();
    std::env::set_var(
        "WORLDLINE_CHECKPOINT_FAIL_THREAD",
        format!("{:?}", std::thread::current().id()),
    );

    for phase in ["payload", "manifest", "publish"] {
        std::env::set_var("WORLDLINE_CHECKPOINT_FAIL_PHASE", phase);
        assert!(project.create_checkpoint(None, default_limits()).is_err());
        assert!(project.list_checkpoints().unwrap().is_empty());
        assert_eq!(fs::read_dir(&history).unwrap().count(), 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct CheckpointFailureEnvironment {
    phase: Option<std::ffi::OsString>,
    thread: Option<std::ffi::OsString>,
}

#[cfg(not(target_arch = "wasm32"))]
impl CheckpointFailureEnvironment {
    fn capture() -> Self {
        Self {
            phase: std::env::var_os("WORLDLINE_CHECKPOINT_FAIL_PHASE"),
            thread: std::env::var_os("WORLDLINE_CHECKPOINT_FAIL_THREAD"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for CheckpointFailureEnvironment {
    fn drop(&mut self) {
        for (key, value) in [
            ("WORLDLINE_CHECKPOINT_FAIL_PHASE", self.phase.as_ref()),
            ("WORLDLINE_CHECKPOINT_FAIL_THREAD", self.thread.as_ref()),
        ] {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[test]
fn checkpoint_rejects_workspaces_over_the_file_count_bound_before_capture() {
    let workspace = TempWorkspace::new();
    let project = workspace.open();
    for index in 0..4093 {
        fs::write(workspace.path().join(format!("extra-{index:04}.bin")), []).unwrap();
    }

    assert!(project.create_checkpoint(None, default_limits()).is_err());
    assert!(!workspace.path().join(".world/.checkpoints").exists());
}

#[test]
fn damaged_checkpoint_is_listed_unavailable_and_never_restored() {
    let workspace = TempWorkspace::new();
    let project = workspace.open();
    let source = project.entry.clone();
    let before = project.document(&source).unwrap().to_owned();
    let checkpoint = project.create_checkpoint(None, default_limits()).unwrap();
    let payloads = workspace
        .path()
        .join(".world/.checkpoints/v1")
        .join(&checkpoint.id)
        .join("files");
    let payload = fs::read_dir(payloads)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&payload, b"truncated").unwrap();

    let listed = project.list_checkpoints().unwrap();
    assert_eq!(listed.len(), 1);
    assert!(!listed[0].available);
    assert!(listed[0].unavailable_reason.is_some());
    assert!(project.preview_checkpoint_restore(&checkpoint.id).is_err());
    assert_eq!(project.document(&source).unwrap(), before);
    assert_eq!(fs::read(source).unwrap(), before.as_bytes());
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

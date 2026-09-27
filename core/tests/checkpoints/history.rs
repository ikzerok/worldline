use super::support::{default_limits, TempWorkspace};
use std::fs;
use worldline_core::project::CheckpointLimits;
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

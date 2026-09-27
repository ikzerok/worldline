use super::support::{default_limits, TempWorkspace};
use std::fs;
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

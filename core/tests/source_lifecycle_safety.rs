//! 紧凑合法语法、缓冲目录身份和受限文件读取的回归。
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;
use worldline_core::source_lifecycle::SourceLifecycleRequest as Request;
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-lifecycle-safe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("world.wl"),
            "include\"old.wl\"\nevent start\n  -> END\n",
        )
        .unwrap();
        std::fs::write(root.join("old.wl"), "// chapter\n").unwrap();
        Self(root)
    }
    fn open(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn moved(to: &str) -> Request {
    Request::Move {
        from: "old.wl".into(),
        to: to.into(),
    }
}

#[test]
fn compact_include_inbound_and_outbound_in_archived_sources_are_rebased() {
    let ws = Workspace::new();
    std::fs::create_dir_all(ws.0.join(".world")).unwrap();
    std::fs::write(
        ws.0.join("archive.wl"),
        "include\"old.wl\" // untouched old.wl\n",
    )
    .unwrap();
    std::fs::write(ws.0.join("old.wl"), "include\"world.wl\"\n").unwrap();
    std::fs::write(ws.0.join("world.wl"), "event start\n  -> END\n").unwrap();
    std::fs::write(ws.0.join(".world/project.json"), r#"{"schema_version":1,"required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["old.wl","archive.wl"]}}"#).unwrap();
    let mut project = ws.open();
    let plan = project
        .preview_source_lifecycle(&moved("章节/new.wl"))
        .unwrap();
    project.apply_source_lifecycle_plan(&plan).unwrap();
    assert_eq!(
        project.document(&ws.0.join("archive.wl")).unwrap(),
        "include\"章节/new.wl\" // untouched old.wl\n"
    );
    assert_eq!(
        project.document(&ws.0.join("章节/new.wl")).unwrap(),
        "include\"../world.wl\"\n"
    );
    assert_eq!(project.sources().len(), 1);
}

#[test]
fn destinations_cannot_be_ancestors_or_case_aliases_of_buffer_only_paths() {
    let ws = Workspace::new();
    let mut project = ws.open();
    project
        .add_file(std::path::Path::new("folder.wl/child.wl"))
        .unwrap();
    project
        .add_file(std::path::Path::new("Dir/keep.wl"))
        .unwrap();
    let baseline = project.content_baseline();
    for destination in ["folder.wl", "dir/new.wl", "DIR/other.wl"] {
        assert!(
            project
                .preview_source_lifecycle(&moved(destination))
                .is_err(),
            "{destination}"
        );
        assert_eq!(project.content_baseline(), baseline);
    }
    assert!(project
        .preview_source_lifecycle(&moved("Dir/new.wl"))
        .is_ok());
    assert!(project
        .preview_source_lifecycle(&moved("different/new.wl"))
        .is_ok());
}

#[test]
fn direct_legacy_include_can_load_an_existing_untracked_normal_source() {
    let ws = Workspace::new();
    let mut project = ws.open();
    std::fs::write(ws.0.join("added.wl"), "// added outside after opening\n").unwrap();
    project.include_file(&ws.0.join("added.wl")).unwrap();
    assert!(project.document(&ws.0.join("added.wl")).is_ok());
    assert!(!project.compile().has_errors());
}

#[cfg(unix)]
#[test]
fn fifo_resource_is_rejected_without_opening_or_blocking() {
    let ws = Workspace::new();
    std::fs::write(ws.0.join("old.wl"), "asset a file \"pipe.txt\"\n").unwrap();
    let project = ws.open();
    assert!(std::process::Command::new("mkfifo")
        .arg(ws.0.join("pipe.txt"))
        .status()
        .unwrap()
        .success());
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        sender
            .send(project.preview_source_lifecycle(&moved("new.wl")))
            .unwrap();
    });
    assert!(receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("FIFO must not block lifecycle preview")
        .is_err());
}

#[test]
fn huge_external_tracked_source_is_rejected_before_whole_file_read() {
    let ws = Workspace::new();
    let project = ws.open();
    std::fs::OpenOptions::new()
        .write(true)
        .open(ws.0.join("old.wl"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let error = project
        .preview_source_lifecycle(&moved("new.wl"))
        .unwrap_err();
    assert!(error.contains("64 MiB"), "{error}");
}

#[test]
fn untracked_include_large_file_and_registered_slot_explosion_are_bounded() {
    let ws = Workspace::new();
    let mut project = ws.open();
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(ws.0.join("huge.wl"))
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let baseline = project.content_baseline();
    assert!(project
        .include_file(&ws.0.join("huge.wl"))
        .unwrap_err()
        .contains("64 MiB"));
    assert_eq!(project.content_baseline(), baseline);
    std::fs::remove_file(ws.0.join("huge.wl")).unwrap();
    let mut active = vec!["old.wl"; 16384];
    active.push("world.wl");
    project.create_authoring_document(&ws.0.join(".world/project.json"), serde_json::to_vec(&serde_json::json!({"schema_version":1,"required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":active,"archived":[]}})).unwrap()).unwrap();
    let baseline = project.content_baseline();
    assert!(project
        .preview_source_lifecycle(&moved("new.wl"))
        .unwrap_err()
        .contains("16384"));
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn relative_paths_reject_all_control_characters() {
    let ws = Workspace::new();
    let project = ws.open();
    let baseline = project.content_baseline();
    for destination in ["tab\tfile.wl", "low\u{1}file.wl", "delete\u{7f}file.wl"] {
        assert!(project
            .preview_source_lifecycle(&moved(destination))
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
}

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
        .add_file(&PathBuf::from("folder.wl").join("child.wl"))
        .unwrap();
    project
        .add_file(&PathBuf::from("Dir").join("keep.wl"))
        .unwrap();
    project.add_file(std::path::Path::new("buffer.wl")).unwrap();
    let baseline = project.content_baseline();
    for relative in [
        PathBuf::from("folder.wl"),
        PathBuf::from("buffer.wl").join("child.wl"),
        PathBuf::from("dir").join("new.wl"),
    ] {
        assert!(project.add_file(&relative).is_err(), "{relative:?}");
        assert_eq!(project.content_baseline(), baseline);
    }
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
    let path = ws.0.join("native").join("added.wl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "// added outside after opening\n").unwrap();
    project.include_file(&path).unwrap();
    assert!(project.document(&path).is_ok());
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("include \"native/added.wl\""));
    assert!(!project.compile().has_errors());
}

#[test]
fn direct_create_and_include_accept_native_joined_relative_paths() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let relative = PathBuf::from("native").join("章节").join("new.wl");
    #[cfg(windows)]
    assert!(relative.to_string_lossy().contains('\\'));
    let path = project.add_file(&relative).unwrap();
    assert_eq!(path, project.root.join(&relative));
    project.include_file(&path).unwrap();
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("include \"native/章节/new.wl\""));
    assert!(!project.compile().has_errors());
    project.save().unwrap();
    assert!(ws.open().document(&path).is_ok());
}

#[test]
fn slash_move_request_uses_the_same_native_identity_as_compiler_load_order() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let before = project.compile();
    let old = project.root.join("old.wl").to_string_lossy().into_owned();
    let expected = project.root.join("native").join("章节").join("new.wl");
    let expected_id = expected.to_string_lossy().into_owned();
    let plan = project
        .preview_source_lifecycle(&moved("native/章节/new.wl"))
        .unwrap();
    // Path equality 已按组件忽略 Windows 分隔符差异；这里必须比较用于目录/加载顺序的字符串。
    assert_eq!(
        plan.destination_path.as_ref().unwrap().to_string_lossy(),
        expected_id
    );
    let expected_order: Vec<_> = before
        .program
        .files
        .iter()
        .map(|file| {
            if file == &old {
                expected_id.clone()
            } else {
                file.clone()
            }
        })
        .collect();
    assert_eq!(plan.load_order_before, before.program.files);
    assert_eq!(plan.load_order_after, expected_order);
    assert_eq!(plan.entry_before, "start");
    assert_eq!(plan.entry_after, plan.entry_before);
    project.apply_source_lifecycle_plan(&plan).unwrap();
    assert_eq!(project.compile().program.files, expected_order);
    assert!(project.document(&expected).is_ok());
}

#[test]
fn native_path_validation_preserves_relative_and_reserved_boundaries() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let baseline = project.content_baseline();
    for relative in [
        PathBuf::from("..").join("outside.wl"),
        PathBuf::from("nested").join("..").join("outside.wl"),
        ws.0.join("absolute.wl"),
        PathBuf::from("nested").join("bad\tname.wl"),
        PathBuf::from(".world").join(".transactions").join("bad.wl"),
        PathBuf::from(".world").join(".checkpoints").join("bad.wl"),
    ] {
        assert!(project.add_file(&relative).is_err(), "{relative:?}");
        assert_eq!(project.content_baseline(), baseline);
    }
}

#[test]
fn lifecycle_request_paths_keep_portable_slash_format() {
    let ws = Workspace::new();
    let project = ws.open();
    for request in [
        Request::Create {
            path: r"native\new.wl".into(),
        },
        Request::Include {
            path: r"native\old.wl".into(),
        },
        moved(r"native\moved.wl"),
    ] {
        assert!(project.preview_source_lifecycle(&request).is_err());
    }
}

#[cfg(not(windows))]
#[test]
fn direct_native_paths_reject_non_windows_backslash_names() {
    let ws = Workspace::new();
    let mut project = ws.open();
    let baseline = project.content_baseline();
    for relative in [r"native\new.wl", r"..\outside.wl"] {
        assert!(project.add_file(std::path::Path::new(relative)).is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
    let misleading = ws.0.join(r"native\existing.wl");
    std::fs::write(&misleading, "// literal backslash filename\n").unwrap();
    assert!(project.include_file(&misleading).is_err());
    assert_eq!(project.content_baseline(), baseline);
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

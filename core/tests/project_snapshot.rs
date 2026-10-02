use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::project::Project;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "worldline-snapshot-v15-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("world.wl"), "event start:\n    END\n").unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn snapshot_keeps_exact_draft_and_deleted_tracking_without_writing() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("old.wl"), "event old:\n    END\n").unwrap();
    let mut project = Project::open(&fixture.0.join("world.wl")).unwrap();
    project
        .set_text(&fixture.0.join("world.wl"), "unfinished draft".into())
        .unwrap();
    project.delete_document(&fixture.0.join("old.wl")).unwrap();
    let baseline = project.content_baseline();
    let files = project.snapshot_files_limited(1, 64).unwrap();
    assert_eq!(files.len(), 1);
    assert!(!files.contains_key(Path::new("old.wl")));
    let state = project.snapshot_state().unwrap();
    let isolated = fixture.0.join("not-created");
    let rebuilt =
        Project::from_snapshot_with_state(&isolated, Path::new("world.wl"), &files, &state)
            .unwrap();
    assert_eq!(rebuilt.content_baseline(), baseline);
    assert!(rebuilt.documents[&rebuilt.root.join("old.wl")].is_deleted());
    assert!(!isolated.exists());
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        "event start:\n    END\n"
    );
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn snapshot_preserves_readonly_and_bad_utf8_authoring_bytes() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join(".world/maps")).unwrap();
    fs::write(
        fixture.0.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"broken":".world/maps/broken.json"}}"#,
    )
    .unwrap();
    let bad = vec![b'{', 255, b'}'];
    fs::write(fixture.0.join(".world/maps/broken.json"), &bad).unwrap();
    let project = Project::open(&fixture.0.join("world.wl")).unwrap();
    let files = project.snapshot_files_limited(3, 1024).unwrap();
    let state = project.snapshot_state().unwrap();
    let root = fixture.0.join("isolated");
    let rebuilt =
        Project::from_snapshot_with_state(&root, Path::new("world.wl"), &files, &state).unwrap();
    let original = project
        .authoring_document(&fixture.0.join(".world/maps/broken.json"))
        .unwrap();
    let copy = rebuilt
        .authoring_document(&root.join(".world/maps/broken.json"))
        .unwrap();
    assert_eq!(copy.bytes(), bad);
    assert_eq!(copy.is_read_only(), original.is_read_only());
    assert_eq!(rebuilt.content_baseline(), project.content_baseline());
    assert!(!root.exists());
}

#[test]
fn snapshot_state_rejects_forged_paths_duplicates_and_payloads() {
    let fixture = Fixture::new();
    let project = Project::open(&fixture.0.join("world.wl")).unwrap();
    let files = project.snapshot_files().unwrap();
    let state = project.snapshot_state().unwrap();
    let check = |state| {
        Project::from_snapshot_with_state(&fixture.0, Path::new("world.wl"), &files, state).is_err()
    };
    let mut wrong = state.clone();
    wrong.schema_version = 99;
    assert!(check(&wrong));
    let mut wrong = state.clone();
    wrong.documents.push(wrong.documents[0].clone());
    assert!(check(&wrong));
    let mut wrong = state.clone();
    wrong.documents[0].path = "../outside.wl".into();
    assert!(check(&wrong));
    let mut wrong = state.clone();
    wrong.documents[0].retained_bytes = Some(b"forged".to_vec());
    assert!(check(&wrong));
    let mut wrong = state.clone();
    wrong.documents.clear();
    assert!(check(&wrong));
    let mut wrong = state.clone();
    wrong.documents[0].deleted = true;
    wrong.documents[0].retained_bytes = Some(b"old".to_vec());
    assert!(check(&wrong));
    let paths = BTreeMap::from([(PathBuf::from("nested/../outside.wl"), vec![])]);
    assert!(Project::from_snapshot(&fixture.0, Path::new("world.wl"), &paths).is_err());
    assert_eq!(
        project.content_baseline(),
        Project::open(&fixture.0.join("world.wl"))
            .unwrap()
            .content_baseline()
    );
}

#[test]
fn limits_reject_large_resources_and_do_not_count_deleted_disk_files_as_active() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("old.wl"), "event old:\n    END\n").unwrap();
    let mut project = Project::open(&fixture.0.join("world.wl")).unwrap();
    project.delete_document(&fixture.0.join("old.wl")).unwrap();
    assert_eq!(project.snapshot_files_limited(1, 1024).unwrap().len(), 1);
    assert!(project.snapshot_files_limited(0, 1024).is_err());
    assert!(project.snapshot_files_limited(1, 1).is_err());
    let asset = fs::File::create(fixture.0.join("oversized.bin")).unwrap();
    asset.set_len(128 * 1024 * 1024 + 1).unwrap();
    let baseline = project.content_baseline();
    assert!(project
        .snapshot_files_limited(3, 128 * 1024 * 1024)
        .is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert!(fixture.0.join("old.wl").exists());
}

#[test]
fn native_nested_snapshot_paths_roundtrip_as_portable_state_with_tombstones() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("chapters/deep")).unwrap();
    fs::create_dir_all(fixture.0.join(".world/maps")).unwrap();
    let source = fixture.0.join("chapters/deep/draft.wl");
    let deleted = fixture.0.join("chapters/deep/deleted.wl");
    fs::write(&source, "event draft\n  -> END\n").unwrap();
    fs::write(&deleted, "event deleted\n  -> END\n").unwrap();
    fs::write(
        fixture.0.join(".world/project.json"),
        br#"{"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{"future":".world/maps/future.json"}}"#,
    )
    .unwrap();
    let future = br#"{"schema_version":99,"private":"keep exact bytes"}"#;
    fs::write(fixture.0.join(".world/maps/future.json"), future).unwrap();
    let mut project = Project::open(&fixture.0).unwrap();
    project
        .set_text(&source, "unfinished nested draft".into())
        .unwrap();
    project.delete_document(&deleted).unwrap();
    let files = project.snapshot_files_limited(4, 4096).unwrap();
    let state = project.snapshot_state().unwrap();
    let wire = serde_json::to_value(&state).unwrap();
    let wire_paths: Vec<_> = wire["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|document| document["path"].as_str().unwrap())
        .collect();
    assert!(wire_paths.contains(&"chapters/deep/draft.wl"));
    assert!(wire_paths.contains(&"chapters/deep/deleted.wl"));
    assert!(wire_paths.contains(&".world/maps/future.json"));
    assert!(wire_paths.iter().all(|path| !path.contains('\\')));
    let decoded = serde_json::from_value(wire).unwrap();
    let destination = fixture.0.join("absent-rebuild-root");
    let rebuilt =
        Project::from_snapshot_with_state(&destination, Path::new("world.wl"), &files, &decoded)
            .unwrap();
    assert_eq!(rebuilt.content_baseline(), project.content_baseline());
    assert_eq!(
        rebuilt.documents[&rebuilt.root.join("chapters/deep/draft.wl")].text,
        "unfinished nested draft"
    );
    assert!(rebuilt.documents[&rebuilt.root.join("chapters/deep/deleted.wl")].is_deleted());
    let copy = rebuilt
        .authoring_document(&rebuilt.root.join(".world/maps/future.json"))
        .unwrap();
    assert_eq!(copy.bytes(), future);
    assert!(copy.is_read_only());
    assert!(!destination.exists());
}

#[cfg(windows)]
#[test]
fn windows_native_separators_accept_relative_paths_but_reject_unsafe_components() {
    let fixture = Fixture::new();
    let files = BTreeMap::from([
        (
            PathBuf::from("world.wl"),
            b"event start\n  -> END\n".to_vec(),
        ),
        (
            PathBuf::from(r"chapters\deep\note.wl"),
            b"nested draft".to_vec(),
        ),
    ]);
    let rebuilt = Project::from_snapshot(&fixture.0, Path::new("world.wl"), &files).unwrap();
    assert_eq!(
        rebuilt.documents[&rebuilt.root.join("chapters/deep/note.wl")].text,
        "nested draft"
    );
    let state = rebuilt.snapshot_state().unwrap();
    assert!(state
        .documents
        .iter()
        .any(|item| item.path.to_str() == Some("chapters/deep/note.wl")));
    for unsafe_path in [
        r"C:\outside.wl",
        r"C:outside.wl",
        r"\outside.wl",
        r"\\server\share\outside.wl",
        r"\\?\C:\outside.wl",
        r"..\outside.wl",
        r"chapters\..\outside.wl",
        r"chapters\.\note.wl",
        r"chapters\\note.wl",
        r"chapters/note.wl/",
        r"chapters/\note.wl",
        r".world\.transactions\journal.wl",
        r".world\.checkpoints\old.wl",
    ] {
        let invalid = BTreeMap::from([(PathBuf::from(unsafe_path), Vec::new())]);
        assert!(
            Project::from_snapshot(&fixture.0, Path::new("world.wl"), &invalid).is_err(),
            "{unsafe_path}"
        );
        assert!(
            Project::from_snapshot(&fixture.0, Path::new(unsafe_path), &files).is_err(),
            "{unsafe_path}"
        );
        let mut forged = state.clone();
        forged.documents[0].path = unsafe_path.into();
        assert!(
            Project::from_snapshot_with_state(&fixture.0, Path::new("world.wl"), &files, &forged)
                .is_err(),
            "{unsafe_path}"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn non_windows_literal_backslash_is_not_a_portable_snapshot_separator() {
    let fixture = Fixture::new();
    let project = Project::open(&fixture.0).unwrap();
    let files = project.snapshot_files().unwrap();
    let state = project.snapshot_state().unwrap();
    for invalid_path in [r"chapters\note.wl", r"chapters\..\outside.wl"] {
        let invalid = BTreeMap::from([(PathBuf::from(invalid_path), Vec::new())]);
        assert!(Project::from_snapshot(&fixture.0, Path::new("world.wl"), &invalid).is_err());
        assert!(Project::from_snapshot(&fixture.0, Path::new(invalid_path), &files).is_err());
        let mut forged = state.clone();
        forged.documents[0].path = invalid_path.into();
        assert!(Project::from_snapshot_with_state(
            &fixture.0,
            Path::new("world.wl"),
            &files,
            &forged
        )
        .is_err());
    }
}

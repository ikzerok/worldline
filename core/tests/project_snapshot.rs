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
    assert!(rebuilt.documents[&isolated.join("old.wl")].is_deleted());
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

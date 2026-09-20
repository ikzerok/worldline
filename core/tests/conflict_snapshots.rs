use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::project::Project;

struct TempWorkspace(PathBuf);

impl TempWorkspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-conflict-snapshots-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn workspace_with_authoring() -> (TempWorkspace, PathBuf, PathBuf) {
    let temp = TempWorkspace::new();
    let root = temp.path().join("world");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"harbor":".world/maps/harbor.json"}}"#,
    )
    .unwrap();
    let map = root.join(".world/maps/harbor.json");
    fs::write(&map, br#"{"schema_version":1,"title":"Harbor"}"#).unwrap();
    (temp, root, map)
}

#[test]
fn conflict_snapshots_expose_source_and_authoring_three_way_bytes_without_mutation() {
    let (_temp, root, _) = workspace_with_authoring();
    let mut project = Project::open(&root).unwrap();
    let map = project.root.join(".world/maps/harbor.json");
    let source = project.entry.clone();
    let source_baseline = fs::read(&source).unwrap();
    let map_baseline = fs::read(&map).unwrap();

    project
        .set_text(&source, "event local\n  -> END\n".into())
        .unwrap();
    let local_map = b"{\"schema_version\":1,\"title\":\xff}".to_vec();
    project
        .set_authoring_document(&map, local_map.clone())
        .unwrap();
    fs::write(&source, "event disk\n  -> END\n").unwrap();
    let disk_map = b"{\"schema_version\":1,\"title\":\xfe}".to_vec();
    fs::write(&map, &disk_map).unwrap();

    let before_source = project.document(&source).unwrap().to_owned();
    let before_map = project.authoring_document(&map).unwrap().bytes().to_vec();
    let before_disk_source = fs::read(&source).unwrap();
    let before_disk_map = fs::read(&map).unwrap();
    let snapshots = project.conflict_snapshots().unwrap();

    assert_eq!(snapshots.len(), 2);
    let source_snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.path == source)
        .unwrap();
    assert_eq!(source_snapshot.baseline, Some(source_baseline));
    assert_eq!(
        source_snapshot.local,
        Some(b"event local\n  -> END\n".to_vec())
    );
    assert_eq!(
        source_snapshot.disk,
        Some(b"event disk\n  -> END\n".to_vec())
    );
    let map_snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.path == map)
        .unwrap();
    assert_eq!(map_snapshot.baseline, Some(map_baseline));
    assert_eq!(map_snapshot.local, Some(local_map));
    assert_eq!(map_snapshot.disk, Some(disk_map));

    assert_eq!(project.document(&source).unwrap(), before_source);
    assert_eq!(
        project.authoring_document(&map).unwrap().bytes(),
        before_map
    );
    assert_eq!(fs::read(&source).unwrap(), before_disk_source);
    assert_eq!(fs::read(&map).unwrap(), before_disk_map);
}

#[test]
fn conflict_snapshots_preserve_missing_disk_and_local_deletion_states() {
    let (_temp, root, _) = workspace_with_authoring();
    let mut project = Project::open(&root).unwrap();
    let map = project.root.join(".world/maps/harbor.json");
    let source = project.entry.clone();
    let source_baseline = fs::read(&source).unwrap();
    let map_baseline = fs::read(&map).unwrap();

    project
        .set_text(&source, "event local\n  -> END\n".into())
        .unwrap();
    project.delete_document(&map).unwrap();
    fs::remove_file(&source).unwrap();
    let disk_map = b"{\"schema_version\":1,\"title\":\"Disk\"}".to_vec();
    fs::write(&map, &disk_map).unwrap();

    let snapshots = project.conflict_snapshots().unwrap();
    assert_eq!(snapshots.len(), 2);
    let source_snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.path == source)
        .unwrap();
    assert_eq!(source_snapshot.baseline, Some(source_baseline));
    assert_eq!(
        source_snapshot.local,
        Some(b"event local\n  -> END\n".to_vec())
    );
    assert_eq!(source_snapshot.disk, None);
    let map_snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.path == map)
        .unwrap();
    assert_eq!(map_snapshot.baseline, Some(map_baseline));
    assert_eq!(map_snapshot.local, None);
    assert_eq!(map_snapshot.disk, Some(disk_map));
}

#[test]
fn conflict_snapshots_exclude_local_changes_when_disk_still_matches_baseline() {
    let (_temp, root, _map) = workspace_with_authoring();
    let mut project = Project::open(&root).unwrap();
    let source = project.entry.clone();
    let baseline = fs::read(&source).unwrap();
    project
        .set_text(&source, "event local\n  -> END\n".into())
        .unwrap();

    assert!(project.conflict_snapshots().unwrap().is_empty());
    assert_eq!(fs::read(&source).unwrap(), baseline);
}

#[test]
fn conflict_snapshots_reject_documents_outside_the_workspace_boundary() {
    let (_temp, root, _map) = workspace_with_authoring();
    let mut project = Project::open(&root).unwrap();
    let source = project.entry.clone();
    project
        .set_text(&source, "event local\n  -> END\n".into())
        .unwrap();
    let outside = root.parent().unwrap().join("outside.wl");
    fs::write(&outside, "event outside\n  -> END\n").unwrap();
    let document = project.documents.get(&source).unwrap().clone();
    project.documents.insert(outside.clone(), document);
    let before = fs::read(&outside).unwrap();

    assert!(project.conflict_snapshots().is_err());
    assert_eq!(fs::read(outside).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn opening_a_workspace_with_a_symlink_still_rejects_the_escaped_file() {
    use std::os::unix::fs::symlink;

    let temp = TempWorkspace::new();
    let root = temp.path().join("world");
    let outside = temp.path().join("outside.wl");
    fs::create_dir_all(&root).unwrap();
    fs::write(&outside, "event outside\n  -> END\n").unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    symlink(&outside, root.join("escaped.wl")).unwrap();

    assert!(Project::open(&root).is_err());
}

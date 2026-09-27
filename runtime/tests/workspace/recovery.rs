use std::sync::{Mutex, MutexGuard, OnceLock};

use super::*;

#[test]
fn recoverable_save_replays_all_document_kinds_after_a_replacement_failure() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(root.join("old.wl"), "character old\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(root.join(".world/maps/map.json"), b"old-map").unwrap();

    let mut project = Project::open(&root).unwrap();
    let entry = project.entry.clone();
    project
        .set_text(&entry, "event changed\n  -> END\n".into())
        .unwrap();
    let new_file = project.add_file(std::path::Path::new("new.wl")).unwrap();
    project.delete_document(&root.join("old.wl")).unwrap();
    project
        .set_authoring_document(&root.join(".world/maps/map.json"), b"new-map".to_vec())
        .unwrap();

    let _failure = set_save_failure("middle");
    assert!(project.save().is_err());
    assert!(root.join(".world/.transactions").is_dir());
    drop(project);
    drop(_failure);

    let reopened = Project::open(&root).unwrap();
    assert_eq!(
        fs::read(&entry).unwrap(),
        b"event changed\n  -> END\n\ninclude \"new.wl\"\n"
    );
    assert_eq!(
        fs::read(&new_file).unwrap(),
        "// 在此文件编写事件,ID 在工程内唯一。\n".as_bytes()
    );
    assert!(!root.join("old.wl").exists());
    assert_eq!(
        fs::read(root.join(".world/maps/map.json")).unwrap(),
        b"new-map"
    );
    assert!(!reopened.is_dirty());
    assert!(reopened.recovery_conflicts().is_empty());
    assert!(!root.join(".world/.transactions").exists());
}

#[test]
fn recovery_keeps_third_party_values_and_blocks_export_until_resolved() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(root.join(".world/maps/map.json"), b"old-map").unwrap();

    let mut project = Project::open(&root).unwrap();
    let entry = project.entry.clone();
    project
        .set_text(&entry, "event local\n  -> END\n".into())
        .unwrap();
    project
        .set_authoring_document(&root.join(".world/maps/map.json"), b"new-map".to_vec())
        .unwrap();
    let _failure = set_save_failure("first");
    assert!(project.save().is_err());
    fs::write(&entry, b"third-party\n").unwrap();
    drop(project);
    drop(_failure);

    let mut reopened = Project::open(&root).unwrap();
    assert_eq!(fs::read(&entry).unwrap(), b"third-party\n");
    assert_eq!(
        fs::read(root.join(".world/maps/map.json")).unwrap(),
        b"new-map"
    );
    assert_eq!(reopened.recovery_conflicts(), &[entry]);
    assert!(reopened.export_files().is_err());
    assert!(reopened.save_as(&temp.0.join("copy")).is_err());
    assert!(root.join(".world/.transactions").is_dir());
}

#[test]
fn every_save_failure_stage_leaves_a_recoverable_transaction() {
    for phase in [
        "prepare", "temp", "first", "middle", "last", "commit", "cleanup",
    ] {
        let temp = Temp::new();
        let root = temp.0.join("world");
        fs::create_dir_all(root.join(".world/maps")).unwrap();
        fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
        fs::write(root.join("old.wl"), "character old\n").unwrap();
        fs::write(
            root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
        )
        .unwrap();
        fs::write(root.join(".world/maps/map.json"), b"old-map").unwrap();

        let mut project = Project::open(&root).unwrap();
        let entry = project.entry.clone();
        project
            .set_text(&entry, "event changed\n  -> END\n".into())
            .unwrap();
        project.add_file(std::path::Path::new("new.wl")).unwrap();
        project.delete_document(&root.join("old.wl")).unwrap();
        project
            .set_authoring_document(&root.join(".world/maps/map.json"), b"new-map".to_vec())
            .unwrap();

        let _failure = set_save_failure(phase);
        assert!(project.save().is_err(), "failure stage {phase} must fail");
        assert!(root.join(".world/.transactions").is_dir());
        drop(project);
        drop(_failure);

        let reopened = Project::open(&root).unwrap();
        assert_eq!(
            fs::read(root.join(".world/maps/map.json")).unwrap(),
            b"new-map"
        );
        assert!(!root.join("old.wl").exists());
        assert!(root.join("new.wl").exists());
        assert!(
            reopened.recovery_conflicts().is_empty(),
            "failure stage {phase}"
        );
        assert!(!root.join(".world/.transactions").exists());
    }
}

#[test]
fn malformed_transaction_path_cannot_escape_workspace() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join(".world/.transactions/bad")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(
        root.join(".world/.transactions/bad/journal.json"),
        br#"{"version":1,"status":"prepared","files":[{"path":"../escape.wl","before":null,"after":"0000000000000000","payload":[]}]}"#,
    )
    .unwrap();
    assert!(Project::open(&root).is_err());
    assert!(!temp.0.join("escape.wl").exists());
    assert!(root.join(".world/.transactions/bad/journal.json").exists());
}

#[test]
fn empty_transaction_directory_is_safe_cleanup_residue() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(root.join(".world/.transactions/leftover")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    let project = Project::open(&root).unwrap();
    assert!(project.recovery_conflicts().is_empty());
    assert!(!root.join(".world/.transactions").exists());
}

#[test]
fn staged_journal_is_promoted_after_crash_before_journal_rename() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let transaction = root.join(".world/.transactions/crashed");
    fs::create_dir_all(&transaction).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(
        transaction.join("journal.json.tmp"),
        br#"{"version":1,"status":"prepared","files":[{"path":"new.wl","before":null,"after":"cbf29ce484222325","payload":[]}]}"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    assert!(project.recovery_conflicts().is_empty());
    assert_eq!(fs::read(root.join("new.wl")).unwrap(), b"");
    assert!(!root.join(".world/.transactions").exists());
}

#[test]
fn opening_recovers_the_entry_before_reading_it() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let transaction = root.join(".world/.transactions/entry");
    fs::create_dir_all(&transaction).unwrap();
    fs::write(
        transaction.join("journal.json"),
        br#"{"version":1,"status":"prepared","files":[{"path":"world.wl","before":null,"after":"cbf29ce484222325","payload":[]}]}"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    assert_eq!(project.document(&root.join("world.wl")).unwrap(), "");
    assert!(!root.join(".world/.transactions").exists());
}

#[test]
fn recovery_distinguishes_missing_from_empty_files() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let transaction = root.join(".world/.transactions/missing");
    fs::create_dir_all(&transaction).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    fs::write(root.join("new.wl"), b"third-party").unwrap();
    fs::write(
        transaction.join("journal.json"),
        br#"{"version":1,"status":"prepared","files":[{"path":"new.wl","before":null,"after":"cbf29ce484222325","payload":[]}]}"#,
    )
    .unwrap();
    let reopened = Project::open(&root).unwrap();
    assert_eq!(fs::read(root.join("new.wl")).unwrap(), b"third-party");
    assert_eq!(
        reopened.recovery_conflicts(),
        &[reopened.root.join("new.wl")]
    );
    assert!(root.join(".world/.transactions").is_dir());
}

#[test]
fn refresh_reconciles_a_journal_recovered_in_the_same_project() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();

    let mut project = Project::open(&root).unwrap();
    let entry = project.entry.clone();
    project
        .set_text(&entry, "event changed\n  -> END\n".into())
        .unwrap();
    let added = project.add_file(std::path::Path::new("new.wl")).unwrap();

    let failure = set_save_failure("middle");
    assert!(project.save().is_err());
    drop(failure);

    let newer = "event newer\n  -> END\n";
    project.set_text(&entry, newer.into()).unwrap();
    assert!(project.refresh().unwrap().is_empty());
    assert!(project.recovery_conflicts().is_empty());
    assert!(project.is_dirty());
    assert_eq!(project.document(&entry).unwrap(), newer);
    assert_eq!(
        fs::read(&added).unwrap(),
        project.document(&added).unwrap().as_bytes()
    );
    project.save().unwrap();
    assert_eq!(fs::read_to_string(&entry).unwrap(), newer);
    assert!(!root.join(".world/.transactions").exists());
}

#[cfg(any(unix, windows))]
#[test]
fn recovery_rejects_linked_target_before_writing_outside() {
    let temp = Temp::new();
    let root = temp.0.join("world");
    let external = temp.0.join("outside");
    let linked = root.join("linked");
    fs::create_dir_all(&external).unwrap();
    fs::create_dir_all(root.join(".world/.transactions/escape")).unwrap();
    fs::write(root.join("world.wl"), "event start\n  -> END\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&external, &linked).unwrap();
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&linked)
            .arg(&external)
            .status()
            .unwrap();
        assert!(status.success(), "无法建立 junction: {status}");
    }
    fs::write(
        root.join(".world/.transactions/escape/journal.json"),
        br#"{"version":1,"status":"prepared","files":[{"path":"linked/outside.txt","before":null,"after":"cbf29ce484222325","payload":[]}]}"#,
    )
    .unwrap();

    assert!(Project::open(&root).is_err());
    assert!(!external.join("outside.txt").exists());
    #[cfg(unix)]
    let _ = fs::remove_file(&linked);
    #[cfg(windows)]
    let _ = fs::remove_dir(&linked);
}

static SAVE_FAILURE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct SaveFailureGuard {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for SaveFailureGuard {
    fn drop(&mut self) {
        std::env::remove_var("WORLDLINE_SAVE_FAIL_PHASE");
        std::env::remove_var("WORLDLINE_SAVE_FAIL_THREAD");
    }
}

fn set_save_failure(phase: &str) -> SaveFailureGuard {
    let lock = SAVE_FAILURE_LOCK.get_or_init(|| Mutex::new(()));
    let guard = lock.lock().unwrap();
    std::env::set_var("WORLDLINE_SAVE_FAIL_PHASE", phase);
    std::env::set_var(
        "WORLDLINE_SAVE_FAIL_THREAD",
        format!("{:?}", std::thread::current().id()),
    );
    SaveFailureGuard { _lock: guard }
}

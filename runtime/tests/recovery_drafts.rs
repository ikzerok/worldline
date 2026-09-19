use std::{fs, path::PathBuf};
use worldline_core::project::Project;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-rescue-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("world/.world/.transactions/interrupted")).unwrap();
        fs::write(root.join("world/world.wl"), b"event start\n  -> END\n").unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn hash(bytes: &[u8]) -> String {
    let mut value = 0xcbf29ce484222325u64;
    for byte in bytes {
        value ^= u64::from(*byte);
        value = value.wrapping_mul(0x100000001b3);
    }
    format!("{value:016x}")
}

#[test]
fn rescue_preserves_raw_drafts_and_deletion_without_resolving_conflicts() {
    let fixture = Fixture::new();
    let root = fixture.0.join("world");
    let draft = [0xff, 0, 0x80];
    fs::write(root.join("map.json"), b"external-json").unwrap();
    fs::write(root.join("deleted.wl"), b"external-source").unwrap();
    let journal_path = root.join(".world/.transactions/interrupted/journal.json");
    let journal = serde_json::to_vec(&serde_json::json!({
        "version":1,"status":"applying","files":[
            {"path":"map.json","before":null,"after":hash(&draft),"payload":draft},
            {"path":"deleted.wl","before":hash(b"old"),"after":null,"payload":null}
        ]
    }))
    .unwrap();
    fs::write(&journal_path, &journal).unwrap();
    let project = Project::open(&root).unwrap();
    let drafts = project.recovery_drafts().unwrap();
    assert_eq!(drafts.len(), 2);
    assert_eq!(drafts[0].bytes.as_deref(), Some(draft.as_slice()));
    assert_eq!(drafts[0].current_hash, Some(hash(b"external-json")));
    assert_eq!(drafts[1].bytes, None);
    assert!(project.export_files().is_err());
    let destination = fixture.0.join("rescue");
    project.export_recovery_drafts(&destination).unwrap();
    assert_eq!(
        fs::read(destination.join("drafts/interrupted/map.json")).unwrap(),
        draft
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("recovery.json")).unwrap()).unwrap();
    assert_eq!(metadata["files"][1]["operation"], "delete");
    assert!(metadata["files"][1]["payload"].is_null());
    assert_eq!(fs::read(root.join("map.json")).unwrap(), b"external-json");
    assert_eq!(
        fs::read(root.join("deleted.wl")).unwrap(),
        b"external-source"
    );
    assert_eq!(fs::read(&journal_path).unwrap(), journal);
    assert!(project.export_recovery_drafts(&destination).is_err());
    assert!(project
        .export_recovery_drafts(&root.join("rescue"))
        .is_err());
}

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::authoring::EntityDraft;
use worldline_core::project::Project;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "worldline-content-baseline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".world")).unwrap();
        fs::write(
            root.join("world.wl"),
            "entity tower kind place as \"灯塔\"\nevent start\n  正文。\n  -> END\n",
        )
        .unwrap();
        fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[],"maps":{}}"#).unwrap();
        Self(root)
    }

    fn open(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("world.wl"));
        let _ = fs::remove_file(self.0.join(".world/project.json"));
        let _ = fs::remove_file(self.0.join(".world/map.json"));
        let _ = fs::remove_dir(self.0.join(".world"));
        let _ = fs::remove_dir(&self.0);
    }
}

#[test]
fn authoring_changes_invalidate_content_baseline_without_changing_runtime_fingerprint() {
    let fixture = Fixture::new();
    let mut project = fixture.open();
    let before = project.content_baseline();
    let fingerprint = project.compile().analysis.fingerprint;
    project
        .write_entity(
            &fixture.0.join("world.wl"),
            Some("tower"),
            &EntityDraft {
                id: "tower".into(),
                entity_type: "landmark".into(),
                display: "新灯塔名".into(),
                description: "改过的作者资料".into(),
                properties: Vec::new(),
            },
        )
        .unwrap();
    assert_ne!(project.content_baseline(), before);
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
}

#[test]
fn manifest_changes_invalidate_baseline_and_restoring_content_restores_it() {
    let fixture = Fixture::new();
    let mut project = fixture.open();
    let path = fixture.0.join(".world/project.json");
    let original = fs::read(&path).unwrap();
    let before = project.content_baseline();
    let fingerprint = project.compile().analysis.fingerprint;
    let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    manifest["extensions"] = serde_json::json!({"note": "作者备注"});
    project
        .set_authoring_document(&path, serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    assert_ne!(project.content_baseline(), before);
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    assert_eq!(fs::read(&path).unwrap(), original);
    project.set_authoring_document(&path, original).unwrap();
    assert_eq!(project.content_baseline(), before);
}

#[test]
fn pending_source_deletion_is_distinct_from_an_untracked_path() {
    let fixture = Fixture::new();
    let mut project = fixture.open();
    let before = project.content_baseline();
    let entry = fixture.0.join("world.wl");
    let source = project.document(&entry).unwrap().to_string();
    let path = project.add_file(std::path::Path::new("draft.wl")).unwrap();
    project.set_text(&entry, source).unwrap();
    project.delete_document(&path).unwrap();
    assert_ne!(project.content_baseline(), before);
    assert!(!fixture.0.join("draft.wl").exists());
}

#[test]
fn registered_map_edits_and_deletion_invalidate_baseline_without_writing_files() {
    let fixture = Fixture::new();
    let manifest_path = fixture.0.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["maps"] = serde_json::json!({"island": ".world/map.json"});
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let path = fixture.0.join(".world/map.json");
    let original = br#"{"schema_version":1,"required_features":[],"layers":[]}"#.to_vec();
    fs::write(&path, &original).unwrap();
    let mut project = fixture.open();
    let before = project.content_baseline();
    project.set_authoring_document(&path, br#"{"schema_version":1,"required_features":[],"layers":[],"extensions":{"note":"changed"}}"#.to_vec()).unwrap();
    assert_ne!(project.content_baseline(), before);
    project
        .set_authoring_document(&path, original.clone())
        .unwrap();
    assert_eq!(project.content_baseline(), before);
    project.delete_authoring_document(&path).unwrap();
    assert_ne!(project.content_baseline(), before);
    project
        .set_authoring_document(&path, original.clone())
        .unwrap();
    assert_eq!(project.content_baseline(), before);
    assert_eq!(fs::read(&path).unwrap(), original);
}

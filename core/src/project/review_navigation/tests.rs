use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "event start\n  初稿。\n  -> END\n";
const MANIFEST: &[u8] = br#"{"schema_version":1,"language_version":"1.9","required_features":[]}"#;

#[cfg(not(target_arch = "wasm32"))]
struct Fixture {
    root: PathBuf,
}
#[cfg(not(target_arch = "wasm32"))]
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "review-navigation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("world.wl"), SOURCE).unwrap();
        Self { root }
    }
    fn manifest(&self, bytes: &[u8]) {
        std::fs::create_dir_all(self.root.join(".world")).unwrap();
        std::fs::write(self.root.join(".world/project.json"), bytes).unwrap();
    }
    fn open(&self) -> Project {
        Project::open_read_only(&self.root).unwrap()
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn new_source_or_manifest_requires_refresh_without_mutating_buffers() {
    let fixture = Fixture::new();
    let project = fixture.open();
    let baseline = project.content_baseline();
    project.verify_review_navigation().unwrap();
    std::fs::write(fixture.root.join("new.wl"), "event added\n  -> END\n").unwrap();
    assert!(project.verify_review_navigation().is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.documents.contains_key(&fixture.root.join("new.wl")));
    std::fs::remove_file(fixture.root.join("new.wl")).unwrap();
    fixture.manifest(MANIFEST);
    assert!(project.verify_review_navigation().is_err());
    assert!(project.authoring_documents.is_empty());
    assert_eq!(project.content_baseline(), baseline);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn modified_manifest_registered_document_and_disk_source_are_rejected() {
    let fixture = Fixture::new();
    fixture.manifest(MANIFEST);
    let project = fixture.open();
    fixture.manifest(br#"{"schema_version":1,"language_version":"1.13","required_features":[]}"#);
    assert!(project.verify_review_navigation().is_err());
    fixture.manifest(MANIFEST);
    project.verify_review_navigation().unwrap();
    std::fs::remove_file(fixture.root.join(".world/project.json")).unwrap();
    assert!(project.verify_review_navigation().is_err());
    fixture.manifest(MANIFEST);
    std::fs::write(
        fixture.root.join("world.wl"),
        "event start\n  外部稿。\n  -> END\n",
    )
    .unwrap();
    assert!(project.verify_review_navigation().is_err());
    assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dirty_local_buffers_and_unsaved_new_sources_do_not_require_saving() {
    let fixture = Fixture::new();
    let mut project = fixture.open();
    let path = project.entry.clone();
    project
        .set_text(&path, SOURCE.replace("初稿", "本地已应用稿"))
        .unwrap();
    let mut writing = project.open_source_writing_buffer(&path).unwrap();
    writing.replace_source(writing.source().replace("本地已应用稿", "未应用稿"));
    project.verify_review_navigation().unwrap();
    let result = project.compile_writing_drafts(&[writing]).unwrap();
    assert!(!result.has_errors());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOURCE);
    let added = fixture.root.join("local-new.wl");
    project.documents.insert(
        added,
        Document {
            text: "event local\n  -> END\n".into(),
            saved: None,
            deleted: false,
        },
    );
    project.verify_review_navigation().unwrap();
    assert_eq!(std::fs::read_dir(&fixture.root).unwrap().count(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn nonexistent_new_workspace_and_pending_transactions_are_not_created_or_recovered() {
    let fixture = Fixture::new();
    let root = fixture.root.join("not-yet-created");
    let project = Project::new(&root);
    project.verify_review_navigation().unwrap();
    assert!(!root.exists());
    let project = fixture.open();
    let transaction = fixture.root.join(".world/.transactions/pending");
    std::fs::create_dir_all(&transaction).unwrap();
    std::fs::write(transaction.join("marker"), "keep").unwrap();
    assert!(project.verify_review_navigation().is_err());
    assert_eq!(
        std::fs::read_to_string(transaction.join("marker")).unwrap(),
        "keep"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn registered_manuscript_external_edit_is_detected() {
    let fixture = Fixture::new();
    fixture.manifest(br#"{"schema_version":1,"required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/book.json"}}"#);
    let path = fixture.root.join(".world/book.json");
    std::fs::write(
        &path,
        r#"{"schema_version":1,"id":"book","title":"Book","entries":[]}"#,
    )
    .unwrap();
    let project = fixture.open();
    project.verify_review_navigation().unwrap();
    std::fs::write(
        &path,
        r#"{"schema_version":1,"id":"book","title":"External","entries":[]}"#,
    )
    .unwrap();
    assert!(project.verify_review_navigation().is_err());
}

#[cfg(target_arch = "wasm32")]
#[test]
fn wasm_navigation_uses_only_imported_snapshot_and_preserves_dirty_input() {
    let root = PathBuf::from(format!(
        "/review-snapshot-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let entry = root.join("world.wl");
    let manifest = root.join(".world/project.json");
    let mut mounted = BTreeMap::from([
        (entry.clone(), SOURCE.as_bytes().to_vec()),
        (manifest.clone(), MANIFEST.to_vec()),
    ]);
    crate::file_access::mount(mounted.clone());
    let files = BTreeMap::from([
        (PathBuf::from("world.wl"), SOURCE.as_bytes().to_vec()),
        (PathBuf::from(".world/project.json"), MANIFEST.to_vec()),
    ]);
    let mut project = Project::from_snapshot(&root, Path::new("world.wl"), &files).unwrap();
    project
        .set_text(&entry, SOURCE.replace("初稿", "浏览器草稿"))
        .unwrap();
    let baseline = project.content_baseline();
    project.verify_review_navigation().unwrap();
    mounted.insert(root.join("new.wl"), b"event new\n  -> END\n".to_vec());
    crate::file_access::mount(mounted.clone());
    assert!(project.verify_review_navigation().is_err());
    mounted.remove(&root.join("new.wl"));
    mounted.insert(manifest, b"{}".to_vec());
    crate::file_access::mount(mounted);
    assert!(project.verify_review_navigation().is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert!(project.document(&entry).unwrap().contains("浏览器草稿"));
    crate::file_access::mount(BTreeMap::new());
}

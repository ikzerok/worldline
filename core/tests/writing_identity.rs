#![cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{project::Project, TargetRef};

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> (Workspace, Project) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "writing-identity-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("world.wl"), "event start\n  原稿\n  -> END\n").unwrap();
    fs::write(root.join("other.wl"), "event other\n  -> END\n").unwrap();
    let project = Project::open(&root).unwrap();
    (Workspace(root), project)
}

#[test]
fn identity_tracks_every_buffer_mutation_rebase_and_undo_branch_without_read_side_effects() {
    let (_work, mut project) = fixture();
    let original = project
        .open_writing_buffer(&TargetRef::new("event", "start"))
        .unwrap();
    let mut buffer = original.clone();
    let initial = buffer.identity();
    buffer.replace_source(buffer.source().into());
    assert_eq!(buffer.identity(), initial);
    assert_eq!(buffer.generation(), 0);
    let at = buffer.source().find("原稿").unwrap();
    buffer
        .replace_range(0, at..at + "原稿".len(), "原稿", "范围稿")
        .unwrap();
    let range_identity = buffer.identity();
    assert_ne!(initial, range_identity);
    assert_eq!(buffer.identity(), buffer.identity());
    let changed_clone = buffer.clone();
    buffer = original.clone();
    assert_eq!(buffer.identity(), initial);
    buffer.replace_source(buffer.source().replace("原稿", "另分叉"));
    assert_eq!(buffer.generation(), changed_clone.generation());
    assert_ne!(buffer.identity(), range_identity);
    let before_rebase = buffer.identity();
    project
        .set_text(
            &project.root.join("other.wl"),
            "event other\n  其他稿\n  -> END\n".into(),
        )
        .unwrap();
    let generation = buffer.generation();
    buffer.rebase_unchanged_source(&project).unwrap();
    assert_eq!(generation, buffer.generation());
    assert_ne!(before_rebase, buffer.identity());
    assert_eq!(buffer.clone().identity(), buffer.identity());
}

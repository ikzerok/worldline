#![cfg(not(target_arch = "wasm32"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        Self(std::env::temp_dir().join(format!(
            "worldline-blank-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn new_project_is_one_valid_blank_document_without_sample_content() {
    let workspace = Workspace::new();
    let mut project = Project::new(&workspace.0);
    assert!(!workspace.0.exists());
    assert!(project.is_dirty());
    assert_eq!(project.documents.len(), 1);
    assert!(project.authoring_documents.is_empty());
    assert_eq!(project.entry, project.root.join("world.wl"));
    assert_eq!(
        project.document(&project.entry).unwrap(),
        "event start\n  -> END\n"
    );

    let compiled = project.compile();
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    assert_eq!(compiled.program.files.len(), 1);
    assert_eq!(compiled.program.events.len(), 1);
    assert_eq!(compiled.program.entry, "start");
    assert!(compiled.program.worlds.is_empty());
    assert!(compiled.program.characters.is_empty());
    assert!(compiled.program.storylines.is_empty());
    assert!(compiled.program.catalog.is_empty());
    assert!(compiled.analysis.world.is_none());
    let files = project.export_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[Path::new("world.wl")], b"event start\n  -> END\n");
    assert!(!workspace.0.exists());

    project.save().unwrap();
    let mut reopened = Project::open(&workspace.0).unwrap();
    assert!(!reopened.is_dirty());
    assert_eq!(reopened.documents.len(), 1);
    assert_eq!(reopened.sources(), project.sources());
    assert_eq!(
        reopened.compile().analysis.fingerprint,
        compiled.analysis.fingerprint
    );
}

#[test]
fn blank_project_can_be_edited_saved_and_reopened() {
    let workspace = Workspace::new();
    let mut project = Project::new(&workspace.0);
    let source = "event start\n  作者新稿。\n  -> END\n";
    project
        .set_text(&project.entry.clone(), source.into())
        .unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert!(!workspace.0.exists());
    project.save().unwrap();
    assert!(!project.is_dirty());
    assert_eq!(fs::read_to_string(&project.entry).unwrap(), source);

    let mut reopened = Project::open(&workspace.0).unwrap();
    assert!(!reopened.is_dirty());
    assert_eq!(reopened.documents.len(), 1);
    assert_eq!(reopened.document(&reopened.entry).unwrap(), source);
    assert_eq!(
        reopened.compile().analysis.fingerprint,
        compiled.analysis.fingerprint
    );
}

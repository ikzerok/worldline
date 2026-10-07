#![cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    catalog::TargetRef, manuscript::*, presentation_commands::Revision, project::Project,
};
mod manuscript_creation {
    use super::*;
    mod compatibility;
    mod conflicts;
}
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-chapter-create-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn request(project: &Project, revision: Revision) -> ManuscriptChapterCreateRequest {
    ManuscriptChapterCreateRequest {
        schema_version: 1,
        expected_baseline: project.content_baseline(),
        expected_revision: revision,
        book: ManuscriptBookDestination::New {
            id: "novel".into(),
            title: "我的书稿".into(),
        },
        chapter: ManuscriptChapterDraft {
            id: "chapter_one".into(),
            title: "第一章".into(),
            parent_section_id: None,
            after_sibling_id: None,
        },
        source: ManuscriptChapterSource::Existing {
            target: TargetRef::new("event", "start"),
        },
    }
}
fn new_event(request: &mut ManuscriptChapterCreateRequest, path: &str, create_file: bool) {
    request.source = ManuscriptChapterSource::NewEvent {
        id: "opening".into(),
        storyline: "main".into(),
        destination: if create_file {
            ManuscriptSourceDestination::NewActiveSource {
                relative_path: path.into(),
            }
        } else {
            ManuscriptSourceDestination::ExistingActiveSource {
                relative_path: path.into(),
            }
        },
    };
}
fn apply(
    project: &mut Project,
    revision: &mut Revision,
    request: &ManuscriptChapterCreateRequest,
) -> ManuscriptChapterCreateResult {
    let plan = project
        .preview_manuscript_chapter_create(*revision, request)
        .unwrap();
    project
        .apply_manuscript_chapter_create(revision, request, &plan.plan_digest)
        .unwrap()
}
#[test]
fn blank_book_reuses_start_without_language_content_or_disk_changes() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let mut revision = Revision::default();
    let request = request(&project, revision);
    let baseline = project.content_baseline();
    let preview = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    assert!(preview.can_apply);
    assert_eq!(preview.entry_before, "start");
    assert_eq!(preview.entry_after, "start");
    assert_eq!(
        preview.runtime_fingerprint_before,
        preview.runtime_fingerprint_after
    );
    assert_eq!(project.content_baseline(), baseline);
    assert!(!work.0.exists());
    let result = project
        .apply_manuscript_chapter_create(&mut revision, &request, &preview.plan_digest)
        .unwrap();
    assert_eq!(result.source_path, Path::new("world.wl"));
    assert_eq!(revision.content_generation, 0);
    assert_eq!(revision.presentation_generation, 1);
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(
        project.document(&project.entry).unwrap(),
        "event start\n  -> END\n"
    );
    assert_eq!(project.documents.len(), 1);
    assert_eq!(project.authoring_documents.len(), 2);
    let mut buffer = project.open_writing_buffer(&result.target).unwrap();
    let slot = project
        .project_writing_buffer(&buffer, &result.target)
        .unwrap()
        .empty_prose_slot
        .unwrap();
    project
        .insert_writing_prose(&mut buffer, &slot, "第一段中文🙂。")
        .unwrap();
    project.apply_writing_buffer(&buffer).unwrap();
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert!(reopened
        .document(&reopened.entry)
        .unwrap()
        .contains("第一段中文🙂。"));
    assert_eq!(
        reopened.manuscript_index("novel").unwrap().entries[0].target_ref,
        Some(TargetRef::new("event", "start"))
    );
}
#[test]
fn new_event_existing_file_is_one_undo_without_automatic_plot_links() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let initial = project.clone();
    let mut revision = Revision::default();
    let mut request = request(&project, revision);
    new_event(&mut request, "world.wl", false);
    let result = apply(&mut project, &mut revision, &request);
    assert_ne!(
        result.plan.runtime_fingerprint_before,
        result.plan.runtime_fingerprint_after
    );
    assert_eq!(revision.content_generation, 1);
    assert_eq!(revision.presentation_generation, 1);
    let compiled = project.compile_read_only().unwrap();
    assert_eq!(compiled.program.entry, "start");
    assert_eq!(compiled.program.events.len(), 2);
    assert!(compiled
        .program
        .events
        .iter()
        .all(|event| event.predecessors.is_empty()));
    assert!(project
        .document(&project.entry)
        .unwrap()
        .starts_with("event start\n  -> END\n"));
    project.save().unwrap();
    let redo = project.clone();
    assert!(project.restore(initial));
    assert_eq!(project.compile_read_only().unwrap().program.events.len(), 1);
    assert!(project.manuscript_indices().is_empty());
    project.save().unwrap();
    assert!(project.restore(redo));
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(
        reopened.compile_read_only().unwrap().program.events.len(),
        2
    );
    assert_eq!(reopened.manuscript_index("novel").unwrap().entries.len(), 1);
}
#[test]
fn new_active_file_is_atomic_and_preserves_explicit_archived_members() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    project.save().unwrap();
    fs::write(work.0.join("archive.wl"), "event archived\n  -> END\n").unwrap();
    fs::write(work.0.join("inactive.wl"), "event inactive\n  -> END\n").unwrap();
    fs::create_dir_all(work.0.join(".world")).unwrap();
    fs::write(work.0.join(".world/project.json"), r#"{"schema_version":1,"entry":"world.wl","required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["archive.wl"]},"extension":{"keep":true}}"#).unwrap();
    let mut project = Project::open(&work.0).unwrap();
    let before = project.content_baseline();
    let mut revision = Revision::default();
    let mut request = request(&project, revision);
    new_event(&mut request, "chapters/opening.wl", true);
    let _plan = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap();
    assert_eq!(before, project.content_baseline());
    assert!(!work.0.join("chapters").exists());
    let result = apply(&mut project, &mut revision, &request);
    assert!(result.plan.new_source);
    assert_eq!(project.sources().len(), 2);
    assert!(!project
        .document(&work.0.join("chapters/opening.wl"))
        .unwrap()
        .contains("在此文件"));
    let content = project.compile_read_only().unwrap();
    assert_eq!(content.program.entry, "start");
    assert_eq!(content.program.events.len(), 2);
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.sources().len(), 2);
    assert_eq!(reopened.documents.len(), 4);
    let manifest: serde_json::Value = serde_json::from_slice(
        reopened
            .authoring_document(&work.0.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(manifest["extension"]["keep"], true);
    assert_eq!(
        manifest["source_config"]["archived"],
        serde_json::json!(["archive.wl"])
    );
}

#![cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{manuscript::*, project::Project, TargetRef};

struct Workspace(PathBuf);
impl Workspace {
    fn new(source: &str, version: &str, localization: bool) -> (Self, Project) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-dialogue-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        fs::write(
            root.join(".world/project.json"),
            serde_json::json!({
                "schema_version":1,"language_version":version,"required_features":
                if localization { vec!["content.localization.v1"] } else { vec![] }
            })
            .to_string(),
        )
        .unwrap();
        let project = Project::open(&root).unwrap();
        assert!(
            !project.compile_read_only().unwrap().has_errors(),
            "{:?}",
            project.compile_read_only().unwrap().diagnostics
        );
        (Self(root), project)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn start() -> TargetRef {
    TargetRef::new("event", "start")
}
fn literal(text: &str) -> DialoguePart {
    DialoguePart::Literal { text: text.into() }
}
fn say(text: &str) -> DialogueDraft {
    DialogueDraft {
        kind: DialogueKind::Say,
        speaker: Some(TargetRef::new("character", "a")),
        direction: None,
        parts: vec![literal(text)],
    }
}
fn request(
    project: &Project,
    buffer: &WritingBuffer,
    operation: DialogueOperation,
) -> DialogueEditRequest {
    DialogueEditRequest {
        schema_version: 1,
        expected_baseline: project.content_baseline(),
        target: start(),
        generation: buffer.generation(),
        operation,
        enable_language_1_11: false,
    }
}
fn project_rows(project: &Project, buffer: &WritingBuffer) -> DialogueProjection {
    project.project_dialogue_buffer(buffer, &start()).unwrap()
}
const BASIC: &str =
    "character a as \"同名\"\ncharacter b as \"同名\"\nevent start\n  say a \"原句\"\n  -> END\n";

mod dialogue_authoring {
    use super::*;
    mod boundaries;
    mod continuation;
    mod expressions;
    mod guards;
    mod localization;
    mod projection;
    mod transactions;
}

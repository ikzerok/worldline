#![allow(dead_code)]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{condition_authoring::*, project::Project, TargetRef};

pub const SOURCE: &str = "world realm as \"世界\"\r\ntag open as \"通行\"\r\ntag alert as \"警戒\"\r\nstate mood on world realm with open as \"心境\"\r\nlet ready = true\r\nlet score = 4\r\nlet title = \"中文\"\r\nrule allowed() -> bool = ready\r\nevent start as \"门🙂\" after ((ready)) // 事件注释\r\n  choice once \"if enable disabled🙂\" if ((ready)) enable score >= 3 disabled \"需3点\" #wl-localization:gate // 选择注释\r\n    正文🙂。\r\n    -> END\r\n\r\nevent other\r\n  -> END\r\n";
pub struct Fixture {
    pub root: PathBuf,
    pub project: Project,
}
impl Fixture {
    pub fn new(source: &str, version: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".scratch")
            .join(format!(
                "condition-core-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".world")).unwrap();
        fs::write(root.join(".world/project.json"), format!(r#"{{"schema_version":1,"language_version":"{version}","required_features":["content.localization.v1"]}}"#)).unwrap();
        fs::write(root.join("world.wl"), source).unwrap();
        let project = Project::open(&root).unwrap();
        Self { root, project }
    }
    pub fn compiled(&self) -> worldline_core::CompileResult {
        worldline_core::compile_sources_with_options(
            &self.project.entry,
            &self.project.sources(),
            self.project.compile_options(),
        )
    }
    pub fn text(&self) -> &str {
        self.project.document(&self.root.join("world.wl")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
pub fn selector(slot: ConditionSlot) -> ConditionSelector {
    ConditionSelector {
        schema_version: 1,
        owner: ConditionOwner {
            target: TargetRef::new("event", "start"),
            choice_line: (slot != ConditionSlot::EventAfter).then_some(10),
        },
        slot,
    }
}
pub fn input(source: &str) -> ConditionDraftInput {
    ConditionDraftInput {
        schema_version: 1,
        owner: selector(ConditionSlot::EventAfter).owner,
        slot: ConditionSlot::EventAfter,
        generation: 7,
        source: source.to_owned(),
    }
}
pub fn request(
    project: &Project,
    slot: ConditionSlot,
    replacement: ConditionNode,
) -> ConditionAuthoringRequest {
    let selector = selector(slot);
    let projection = project.condition_authoring(&selector).unwrap();
    ConditionAuthoringRequest {
        schema_version: 1,
        expected_baseline: projection.baseline,
        expected_generation: projection.generation,
        selector,
        replacement,
    }
}

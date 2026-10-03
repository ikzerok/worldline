#![allow(dead_code)]
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldline_core::{project::Project, CompileResult};
use worldline_runtime::{ReplayTrace, Story};

pub const SOURCE: &str = include_str!("route_comparison.wl");
static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Fixture {
    pub root: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "wl-route-comparison-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".world")).unwrap();
        std::fs::write(root.join("world.wl"), SOURCE).unwrap();
        std::fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.10","required_features":[]}"#,
        )
        .unwrap();
        Self { root }
    }
    pub fn compile(&self) -> CompileResult {
        let mut project = Project::open(&self.root).unwrap();
        let result = project.compile();
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        result
    }
    pub fn traces(&self) -> (ReplayTrace, ReplayTrace) {
        let result = self.compile();
        (record(&result, 0, true), record(&result, 1, true))
    }
    pub fn replace(&self, source: &str) {
        std::fs::write(self.root.join("world.wl"), source).unwrap();
    }
    pub fn source(&self) -> String {
        std::fs::read_to_string(self.root.join("world.wl")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn record(result: &CompileResult, branch: usize, finish: bool) -> ReplayTrace {
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 73).unwrap();
    story.continue_story().unwrap();
    story.choose(branch).unwrap();
    story.continue_story().unwrap();
    if finish {
        story.choose(0).unwrap();
        story.continue_story().unwrap();
        assert!(story.is_ended());
    }
    story.replay_trace()
}

pub fn checkpoint_trace(result: &CompileResult) -> ReplayTrace {
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 73).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let checkpoint = story.checkpoint().unwrap();
    let mut restored = Story::from_checkpoint(&result.program, &result.analysis, &checkpoint).unwrap();
    restored.continue_story().unwrap();
    restored.replay_trace()
}

pub fn request(method: &str, params: Value) -> Value {
    serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}

//! 集成测试:最小合成输入编译、试玩走查、诊断、存读档。
//! 位于 runtime 包(core + runtime 均可用)。

use worldline_core::ast::PropertyValue;
use worldline_core::authoring::{CharacterDraft, EventDraft};
use worldline_core::project::Project;
use worldline_core::{compile_path, compile_source, EdgeKind, Severity};
use worldline_runtime::{AnchorKind, Output, Story};

#[path = "integration/analysis.rs"]
mod analysis;
#[path = "integration/authoring.rs"]
mod authoring;
#[path = "integration/diagnostics.rs"]
mod diagnostics;
#[path = "integration/playthrough.rs"]
mod playthrough;
#[path = "integration/saves.rs"]
mod saves;
#[path = "integration/story_state.rs"]
mod story_state;

fn transcript(story: &mut Story) -> String {
    let mut buf = String::new();
    for o in story.continue_story().unwrap() {
        match o {
            Output::Text {
                content, new_line, ..
            } => {
                if new_line && !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&content);
            }
            Output::Ended => buf.push_str("[END]"),
        }
    }
    buf
}

/// 按脚本走完一个故事,收集全部输出(选择处消费脚本)。
fn play_all(source: &str, script: &[usize]) -> (String, Vec<String>) {
    let result = compile_source("test.wl", source);
    assert!(
        !result.has_errors(),
        "编译存在错误:{:#?}",
        result.diagnostics
    );
    let mut story = Story::new(&result.program, &result.analysis).unwrap();
    let mut log = String::new();
    let mut labels = Vec::new();
    loop {
        log.push_str(&transcript(&mut story));
        if story.is_ended() {
            return (log, labels);
        }
        let choices: Vec<String> = story.choices().iter().map(|c| c.label.clone()).collect();
        labels.push(choices.join("|"));
        let n = script.get(labels.len() - 1).copied().unwrap_or(0);
        if n >= choices.len() {
            panic!("脚本第 {} 步越界:{:?}", labels.len(), choices);
        }
        story.choose(n).unwrap();
        log.push_str(&format!(" <<{}>> ", choices[n]));
    }
}

struct ProjectDir(std::path::PathBuf);
impl ProjectDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "worldline-project-test-{}-{id}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for ProjectDir {
    fn drop(&mut self) {
        assert!(self.0.is_absolute() && self.0.starts_with(std::env::temp_dir()));
        assert!(self
            .0
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("worldline-project-test-"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// -- 诊断 -------------------------------------------------------------------

fn codes(source: &str) -> Vec<(String, Severity)> {
    let result = compile_source("d.wl", source);
    result
        .diagnostics
        .iter()
        .map(|d| (d.code.to_string(), d.severity))
        .collect()
}

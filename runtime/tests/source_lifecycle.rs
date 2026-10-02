//! 路径整理必须实际验证旧存档、检查点和入口 trace 的既有守卫。
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;
use worldline_core::source_lifecycle::SourceLifecycleRequest;
use worldline_runtime::{ReplayBudget, ReplayCancellation, ReplayStatus, ReplayTrace, Story};

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn safe_source_move_keeps_old_save_checkpoint_and_entry_trace_loadable() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let ws = Workspace(std::env::temp_dir().join(format!(
        "wl-runtime-move-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&ws.0).unwrap();
    std::fs::write(
        ws.0.join("world.wl"),
        "include \"old.wl\"\nlet count = 0\nevent start\n  启程\n  -> chapter\n",
    )
    .unwrap();
    std::fs::write(ws.0.join("old.wl"), "event chapter\n  set count = count + 1\n  第 {count} 章\n  choice \"继续\"\n    到达\n    -> END\n").unwrap();
    let mut project = Project::open(&ws.0).unwrap();
    let before = project.compile();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    let mut original = Story::new_with_seed(&before.program, &before.analysis, 31).unwrap();
    original.continue_story().unwrap();
    let save = original.save().unwrap();
    let checkpoint = original.checkpoint().unwrap();
    let state = original.state_view();
    original.choose(0).unwrap();
    let output = serde_json::to_value(original.continue_story().unwrap()).unwrap();
    let trace = original.replay_trace();

    let request = SourceLifecycleRequest::Move {
        from: "old.wl".into(),
        to: "章节/第一 卷/新章.wl".into(),
    };
    let plan = project.preview_source_lifecycle(&request).unwrap();
    project.apply_source_lifecycle_plan(&plan).unwrap();
    project.save().unwrap();
    let mut reopened = Project::open(&ws.0).unwrap();
    let after = reopened.compile();
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(before.program.entry, after.program.entry);
    let mut restored = Story::load(&after.program, &after.analysis, &save).unwrap();
    assert_eq!(restored.state_view(), state);
    // 完整 JSON 值相等，保留全部存档字段与数组顺序；仅忽略 HashMap 对象键的序列化顺序。
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&restored.save().unwrap()).unwrap(),
        serde_json::from_str::<serde_json::Value>(&save).unwrap(),
        "restored save must preserve every field; partial state comparisons are insufficient"
    );
    let from_checkpoint =
        Story::from_checkpoint(&after.program, &after.analysis, &checkpoint).unwrap();
    assert_eq!(from_checkpoint.state_view(), state);
    restored.choose(0).unwrap();
    assert_eq!(
        serde_json::to_value(restored.continue_story().unwrap()).unwrap(),
        output
    );
    let replay = ReplayTrace::replay(
        &after.program,
        &after.analysis,
        &trace,
        ReplayBudget::new(10_000, 5_000),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(matches!(
        replay.status,
        ReplayStatus::Replayed { ended: true, .. }
    ));
    assert_eq!(replay.current_state, original.state_view());
}

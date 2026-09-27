use super::common::*;
use serde_json::Value;

#[test]
fn check_json_output() {
    let f = temp_story("ok.wl", "event start\n  你好。\n  -> END\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["check".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("\"ok\":true"), "{text}");
    assert!(text.contains("\"diagnostics\""), "{text}");
    // JSON 必须可解析
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("输出应为合法 JSON");
    assert_eq!(v["ok"], serde_json::Value::Bool(true));
}

#[test]
fn play_can_write_a_seeded_trace_that_cli_replays() {
    let source = temp_story(
        "replay-cli.wl",
        "event start\n  开场。\n  choice \"完成\"\n    结尾。\n    -> END\n",
    );
    let trace_path = std::env::temp_dir().join(format!(
        "wl-replay-cli-{}-{}.json",
        std::process::id(),
        source.file_name().unwrap().to_string_lossy()
    ));
    let mut output = Vec::new();
    let mut input = std::io::Cursor::new(b"0\n".to_vec());
    let code = wl::run(
        &[
            "play".into(),
            source.to_string_lossy().into_owned(),
            "--seed=907".into(),
            format!("--trace-output={}", trace_path.display()),
            "--json".into(),
        ],
        &mut output,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let trace = std::fs::read_to_string(&trace_path).unwrap();
    let trace_dto: Value = serde_json::from_str(&trace).unwrap();
    assert_eq!(trace_dto["origin"]["kind"], "entry");
    assert_eq!(trace_dto["complete"], true);

    let mut replay_output = Vec::new();
    let mut no_input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &[
            "replay".into(),
            source.to_string_lossy().into_owned(),
            format!("--trace-json={trace}"),
            "--max-steps=1000".into(),
            "--time-budget-ms=5000".into(),
            "--json".into(),
        ],
        &mut replay_output,
        &mut no_input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let replay = json_lines(&replay_output).remove(0);
    assert_eq!(replay["status"]["status"], "replayed");
    assert_eq!(replay["status"]["complete"], true);
}

#[test]
fn check_exit_code_on_error() {
    let f = temp_story("bad.wl", "event start\n  -> missing\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["check".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1, "存在错误时退出码应为 1");
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("A101"), "{text}");
}

#[test]
fn play_scripted_choices() {
    let f = temp_story(
        "story.wl",
        "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new("1\n".as_bytes().to_vec());
    let code = wl::run(
        &["play".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("甲线。"), "{text}");
    assert!(text.contains("故事结束"), "{text}");
}

#[test]
fn graph_outputs_mermaid() {
    let f = temp_story("g.wl", "event start\n  choice \"走\"\n    -> END\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["graph".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("flowchart TD"), "{text}");
}

#[test]
fn timeline_outputs_swimlanes_and_drift() {
    let f = temp_story(
        "tl.wl",
        "storyline a as \"甲线\"\n  event start\n    -> END\n\nstoryline b\n  event b.entry\n    ->> start\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["timeline".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("flowchart LR"), "{text}");
    assert!(text.contains("subgraph"), "{text}");
    assert!(text.contains("甲线"), "{text}");
    assert!(text.contains("漂流"), "{text}");
}

#[test]
fn timeline_rejects_broken_story() {
    let f = temp_story("tl_bad.wl", "event start\n  -> missing\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["timeline".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1);
}

#[test]
fn play_rejects_broken_story() {
    let f = temp_story("broken.wl", "event start\n  -> nowhere\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["play".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("A101"), "{text}");
}

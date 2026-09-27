use super::common::*;

// -- 机器模式(spec/agent-protocol.md §2) ------------------------------------

#[test]
fn graph_json_outputs_structured() {
    let f = temp_story(
        "g_json.wl",
        "event start\n  choice \"走\"\n    -> next\n\nevent next\n  -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["graph".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let v = &json_lines(&out)[0];
    let nodes = v["graph"]["nodes"].as_array().expect("nodes 数组");
    assert!(!nodes.is_empty());
    assert_eq!(v["graph"]["entry"], 0);
    assert_eq!(v["graph"]["edges"][0]["kind"], "choice");
}

#[test]
fn timeline_json_outputs_anchors_and_stats() {
    let f = temp_story(
        "tl_json.wl",
        "storyline a as \"甲线\"\n  event start\n    开场。\n    anchor \"听闻\" as \"听说\"\n    -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &[
            "timeline".into(),
            f.to_string_lossy().into(),
            "--json".into(),
        ],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let v = &json_lines(&out)[0];
    assert_eq!(v["stats"]["events"], 1);
    assert_eq!(v["graph"]["storyline_order"][0][0], "a");
    let anchors = v["anchors"].as_array().expect("anchors 数组");
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0]["name"], "听闻");
}

#[test]
fn graph_json_compile_failed() {
    let f = temp_story("g_bad.wl", "event start\n  -> missing\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["graph".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1);
    let v = &json_lines(&out)[0];
    assert_eq!(v["type"], "compile_failed");
    assert!(!v["diagnostics"].as_array().expect("diagnostics").is_empty());
}

#[test]
fn play_json_stepwise_choices() {
    let f = temp_story(
        "p_json.wl",
        "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new("0\n".as_bytes().to_vec());
    let code = wl::run(
        &["play".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let events = json_lines(&out);
    assert_eq!(events[0]["type"], "turn");
    assert_eq!(events[0]["outputs"][0]["content"], "开场。");
    assert_eq!(events[0]["choices"][0]["label"], "甲");
    assert_eq!(events[0]["choices"][0]["index"], 0);
    assert_eq!(events[0]["state"]["paused"], true);
    assert_eq!(events[1]["type"], "ended");
    assert_eq!(events[1]["state"]["ended"], true);
}

#[test]
fn play_json_eof_saves_and_reload_resumes_paused() {
    let f = temp_story(
        "p_save.wl",
        "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n",
    );
    let save = std::env::temp_dir()
        .join("wl_cli_tests")
        .join("p_save.json");
    // 第一段:空 stdin,暂停即 EOF → 落存档
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &[
            "play".into(),
            f.to_string_lossy().into(),
            "--json".into(),
            format!("--save={}", save.display()),
        ],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let events = json_lines(&out);
    assert_eq!(events[0]["type"], "turn");
    assert_eq!(events.last().unwrap()["type"], "eof");
    let save_text = std::fs::read_to_string(&save).unwrap();
    let sv: serde_json::Value = serde_json::from_str(&save_text).unwrap();
    assert!(sv["fingerprint"].is_u64(), "存档应含指纹");
    // 第二段:读档恢复,应再次停在同一个选择组
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &[
            "play".into(),
            f.to_string_lossy().into(),
            "--json".into(),
            format!("--load={}", save.display()),
        ],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let events = json_lines(&out);
    assert_eq!(events[0]["type"], "turn");
    assert_eq!(events[0]["choices"][0]["label"], "甲");
    assert!(
        events[0]["outputs"].as_array().unwrap().is_empty(),
        "暂停态不重复产出文本"
    );
}

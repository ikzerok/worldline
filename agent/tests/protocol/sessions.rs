use super::*;

#[test]
fn navigation_metadata_and_unknown_links_use_story_results() {
    let source = "character lin as \"林舟\"\nalias character lin as \"阿舟\"\nevent start\n  看见[[character:lin|阿舟]]。\n  -> END\n";
    let (_, responses) = exchange(&[
        req(1, "compile", json!({"source": source})),
        req(2, "analyze", json!({"story_id": "s1"})),
        req(
            3,
            "compile",
            json!({"source": source.replace("[[character:lin|", "[[character:missing|")}),
        ),
    ]);
    assert_eq!(responses[0]["result"]["ok"], true);
    let catalog = &responses[1]["result"]["catalog"];
    assert_eq!(catalog["aliases"][0]["name"], "阿舟");
    assert_eq!(catalog["text_links"][0]["target"]["id"], "lin");
    assert_eq!(catalog["text_links"][0]["line"], 4);
    assert!(responses[2].get("error").is_none());
    assert_eq!(responses[2]["result"]["ok"], false);
    assert!(responses[2]["result"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "A218"));
}

#[test]
fn initialize_and_shutdown() {
    let (code, resp) = exchange(&[
        req(1, "initialize", json!({})),
        req(2, "shutdown", json!({})),
    ]);
    assert_eq!(code, 0);
    assert_eq!(resp.len(), 2);
    assert_eq!(resp[0]["result"]["protocol"], 1);
    assert_eq!(resp[0]["result"]["server"], "wl-agent");
    assert_eq!(resp[0]["result"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(resp[1]["result"]["bye"], true);
}

#[test]
fn full_play_session() {
    let (_, resp) = exchange(&[
        req(1, "compile", json!({ "source": STORY })),
        req(2, "session.open", json!({ "story_id": "s1" })),
        req(3, "session.continue", json!({ "session_id": "c1" })),
        req(
            4,
            "session.choose",
            json!({ "session_id": "c1", "index": 0 }),
        ),
        req(5, "session.continue", json!({ "session_id": "c1" })),
        req(6, "session.continue", json!({ "session_id": "c1" })),
        req(7, "session.restart", json!({ "session_id": "c1" })),
        req(8, "session.save", json!({ "session_id": "c1" })),
        req(9, "session.close", json!({ "session_id": "c1" })),
        req(10, "shutdown", json!({})),
    ]);
    for (i, r) in resp.iter().enumerate() {
        assert!(
            r.get("error").is_none(),
            "第 {} 个响应不应是错误:{r}",
            i + 1
        );
    }
    let r = &resp[0]["result"];
    assert_eq!(r["ok"], true);
    assert_eq!(r["story_id"], "s1");
    assert!(r["fingerprint"].is_u64());
    assert_eq!(r["stats"]["events"], 1);
    assert_eq!(resp[1]["result"]["session_id"], "c1");

    let r = &resp[2]["result"];
    assert_eq!(r["outputs"][0]["type"], "text");
    assert_eq!(r["outputs"][0]["content"], "开场。");
    assert_eq!(r["choices"][0]["label"], "甲");
    assert_eq!(r["choices"][1]["index"], 1);
    assert_eq!(r["paused"], true);
    assert_eq!(r["ended"], false);
    assert_eq!(r["state"]["paused"], true);

    let r = &resp[3]["result"];
    assert_eq!(r["paused"], false);
    assert_eq!(r["ended"], false);

    let r = &resp[4]["result"];
    assert_eq!(r["outputs"][0]["content"], "甲线。");
    assert_eq!(r["outputs"][1]["type"], "ended");
    assert_eq!(r["ended"], true);
    assert_eq!(r["state"]["ended"], true);

    // 已结束的会话继续 continue 仍安全:outputs 仅含收束事件
    let r = &resp[5]["result"];
    assert_eq!(r["ended"], true);
    assert_eq!(r["outputs"][0]["type"], "ended");

    // restart 复位到开头
    let r = &resp[6]["result"];
    assert_eq!(r["state"]["ended"], false);
    assert_eq!(r["state"]["turns"], 0);
    assert_eq!(r["state"]["current_node"], "start");

    let save = resp[7]["result"]["save"].as_str().expect("save 为字符串");
    let sv: Value = serde_json::from_str(save).expect("存档应为合法 JSON");
    assert!(sv["fingerprint"].is_u64());
    assert_eq!(resp[8]["result"]["closed"], true);
    assert_eq!(resp[9]["result"]["bye"], true);
}

#[test]
fn replay_rpc_exposes_trace_checkpoint_and_pure_choice_explanations() {
    let source =
        "event start\n  choice \"blocked\" if false\n    -> END\n  choice \"finish\"\n    -> END\n";
    let compiled = worldline_core::compile_source("未命名.wl", source);
    assert!(!compiled.has_errors(), "{:#?}", compiled.diagnostics);
    let mut fixture =
        worldline_runtime::Story::new_with_seed(&compiled.program, &compiled.analysis, 42).unwrap();
    fixture.continue_story().unwrap();
    fixture.choose(0).unwrap();
    fixture.continue_story().unwrap();
    let replay_trace = json!(fixture.replay_trace());
    let (_, first) = exchange(&[
        req(1, "compile", json!({ "source": source })),
        req(2, "session.open", json!({ "story_id": "s1", "seed": 42 })),
        req(3, "session.continue", json!({ "session_id": "c1" })),
        req(4, "session.explain_choices", json!({ "session_id": "c1" })),
        req(5, "session.checkpoint", json!({ "session_id": "c1" })),
        req(
            6,
            "session.choose",
            json!({ "session_id": "c1", "index": 0 }),
        ),
        req(7, "session.continue", json!({ "session_id": "c1" })),
        req(8, "session.trace", json!({ "session_id": "c1" })),
        req(
            9,
            "trace.replay",
            json!({
                "story_id": "s1",
                "trace": replay_trace,
                "max_steps": 1000,
                "time_budget_ms": 5000
            }),
        ),
        req(10, "shutdown", json!({})),
    ]);
    assert!(first.iter().all(|response| response.get("error").is_none()));
    assert_eq!(first[1]["result"]["state"]["paused"], false);
    assert_eq!(first[3]["result"]["choices"][0]["available"], false);
    assert_eq!(
        first[3]["result"]["choices"][0]["condition"]["result"],
        false
    );
    assert_eq!(first[4]["result"]["checkpoint"]["seed"], 42);
    let trace = first[7]["result"]["trace"].clone();
    assert_eq!(trace["complete"], true);
    assert_eq!(first[8]["result"]["ok"], true, "{:?}", first[8]);
    assert_eq!(first[8]["result"]["replay"]["status"]["status"], "replayed");
    assert_eq!(first[8]["result"]["replay"]["status"]["complete"], true);
}

#[test]
fn replay_rpc_separates_invalid_dtos_from_story_failures() {
    let source = "event start\n  choice \"continue\" if true\n    -> END\n";
    let compiled = worldline_core::compile_source("未命名.wl", source);
    assert!(!compiled.has_errors(), "{:#?}", compiled.diagnostics);
    let mut fixture =
        worldline_runtime::Story::new_with_seed(&compiled.program, &compiled.analysis, 9).unwrap();
    fixture.continue_story().unwrap();
    fixture.choose(0).unwrap();
    fixture.continue_story().unwrap();
    let trace = json!(fixture.replay_trace());
    let changed_source = "event start\n  choice \"continue\" if 1 / 0 == 1\n    -> END\n";

    let (_, responses) = exchange(&[
        req(1, "compile", json!({ "source": source })),
        req(2, "compile", json!({ "source": changed_source })),
        req(
            3,
            "trace.replay",
            json!({ "story_id": "s2", "trace": trace }),
        ),
        req(4, "trace.replay", json!({ "story_id": "s2", "trace": {} })),
        req(5, "session.open", json!({ "story_id": "s1", "seed": -1 })),
        req(6, "shutdown", json!({})),
    ]);
    assert_eq!(responses[2]["result"]["ok"], true);
    assert_eq!(
        responses[2]["result"]["replay"]["status"]["status"],
        "story_failed"
    );
    assert_eq!(responses[2]["result"]["replay"]["status"]["line"], 2);
    assert_eq!(responses[3]["error"]["code"], -32602);
    assert_eq!(responses[4]["error"]["code"], -32602);
}

#[test]
fn compile_error_returns_diagnostics() {
    let (_, resp) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": "event start\n  -> missing\n" }),
        ),
        req(2, "shutdown", json!({})),
    ]);
    let r = &resp[0]["result"];
    assert_eq!(r["ok"], false);
    assert!(r.get("story_id").is_none(), "编译失败不产生 story_id");
    let diags = r["diagnostics"].as_array().expect("diagnostics 数组");
    assert_eq!(diags[0]["code"], "A101");
}

#[test]
fn save_load_roundtrip_resumes_paused() {
    let (_, first) = exchange(&[
        req(1, "compile", json!({ "source": STORY })),
        req(2, "session.open", json!({ "story_id": "s1" })),
        req(3, "session.continue", json!({ "session_id": "c1" })),
        req(4, "session.save", json!({ "session_id": "c1" })),
    ]);
    let save = first[3]["result"]["save"].as_str().unwrap().to_string();
    let (_, second) = exchange(&[
        req(1, "compile", json!({ "source": STORY })),
        req(2, "session.open", json!({ "story_id": "s1", "save": save })),
        req(3, "session.continue", json!({ "session_id": "c1" })),
        req(4, "shutdown", json!({})),
    ]);
    let r = &second[2]["result"];
    assert!(
        r["outputs"].as_array().unwrap().is_empty(),
        "暂停态不重复产出文本"
    );
    assert_eq!(r["choices"][0]["label"], "甲");
    assert_eq!(r["paused"], true);
}

#[test]
fn analyze_and_export() {
    let story = "storyline a as \"甲线\"\n  event start\n    开场。\n    anchor \"听闻\" as \"听说\"\n    -> END\n";
    let (_, resp) = exchange(&[
        req(1, "compile", json!({ "source": story })),
        req(2, "analyze", json!({ "story_id": "s1" })),
        req(
            3,
            "export",
            json!({ "story_id": "s1", "format": "timeline_mermaid" }),
        ),
        req(4, "shutdown", json!({})),
    ]);
    let a = &resp[1]["result"];
    assert!(!a["graph"]["nodes"].as_array().expect("nodes").is_empty());
    assert_eq!(a["anchors"][0]["name"], "听闻");
    assert_eq!(a["stats"]["events"], 1);
    assert_eq!(a["symbols"]["storyline_order"][0], "a");
    assert!(a["catalog"]["objects"].is_array());
    assert_eq!(a["catalog"]["tags"], json!({}));
    assert_eq!(a["catalog"]["assets"], json!({}));
    assert_eq!(a["catalog"]["marks"], json!([]));
    assert_eq!(a["catalog"]["attachments"], json!([]));
    assert!(resp[2]["result"]["text"]
        .as_str()
        .unwrap()
        .contains("flowchart LR"));
}

#[test]
fn compile_accepts_explicit_110_and_analyze_exposes_entities() {
    let source = "entity lighthouse kind place as \"雾港灯塔\"\n";
    let (_, responses) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": source, "language_version": "1.10" }),
        ),
        req(2, "analyze", json!({ "story_id": "s1" })),
        req(3, "shutdown", json!({})),
    ]);
    assert_eq!(responses[0]["result"]["ok"], true);
    assert_eq!(responses[0]["result"]["language_version"], "1.10");
    assert_eq!(responses[1]["result"]["language_version"], "1.10");
    assert_eq!(
        responses[1]["result"]["catalog"]["entities"]["lighthouse"]["entity_type"],
        "place"
    );
}

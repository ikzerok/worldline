//! wl-agent 协议集成测试:直接驱动 lib 层 run(),不启动进程。

use std::io::Cursor;

use serde_json::{json, Value};

/// 发送一批请求行,返回 (退出码, 响应列表)。
fn exchange(lines: &[Value]) -> (i32, Vec<Value>) {
    let input = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    exchange_raw(&input)
}

/// 同 exchange,但接受原始文本(测坏 JSON 等非合法请求)。
fn exchange_raw(input: &str) -> (i32, Vec<Value>) {
    let mut out = Vec::new();
    let code = worldline_agent::run(&mut Cursor::new(input.to_string()), &mut out);
    let responses = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("每行响应都应为合法 JSON"))
        .collect();
    (code, responses)
}

fn req(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

const STORY: &str = "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n";

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
fn analyze_catalog_preserves_targets_and_source_locations() {
    let source = "tag coordinate as \"坐标\"\ntag harbor as \"港口\"\nmark tag harbor with coordinate\nmark event start with harbor\nevent start\n  -> END\n";
    let (code, resp) = exchange(&[
        req(
            1,
            "compile",
            json!({"source": source, "file_name": "catalog.wl"}),
        ),
        req(2, "analyze", json!({"story_id": "s1"})),
        req(3, "session.open", json!({"story_id": "s1"})),
        req(4, "session.continue", json!({"session_id": "c1"})),
        req(5, "analyze", json!({"story_id": "s1"})),
        req(6, "shutdown", json!({})),
    ]);
    assert_eq!(code, 0);
    assert_eq!(resp.len(), 6);
    for (index, response) in resp.iter().enumerate() {
        assert_eq!(response["id"], index + 1);
        assert!(response.get("error").is_none(), "{response}");
    }
    assert_eq!(resp[0]["result"]["ok"], true);
    let analyzed = &resp[1]["result"];
    for field in ["graph", "anchors", "symbols", "stats", "world", "timeline"] {
        assert!(analyzed.get(field).is_some(), "原字段 {field} 继续保留");
    }
    let catalog = &analyzed["catalog"];
    assert_eq!(catalog["tags"].as_object().unwrap().len(), 2);
    assert!(catalog["tags"]["coordinate"].is_object());
    assert!(catalog["tags"]["harbor"].is_object());
    assert_eq!(catalog["assets"], json!({}));
    assert_eq!(catalog["attachments"], json!([]));
    assert_eq!(catalog["marks"].as_array().unwrap().len(), 2);
    let objects = catalog["objects"].as_array().expect("目录对象数组");
    for (kind, id, line) in [("tag", "harbor", 2), ("event", "start", 5)] {
        let object = objects
            .iter()
            .find(|o| o["target"] == json!({"kind": kind, "id": id}))
            .expect("带稳定 ID 的对象");
        assert_eq!(object["line"], line);
        assert!(object["file"].as_str().unwrap().ends_with("catalog.wl"));
    }
    assert_eq!(resp[3]["result"]["ended"], true);
    assert_eq!(resp[4]["result"]["catalog"], *catalog, "播放不会改变目录");
}

#[test]
fn protocol_errors() {
    let (code, resp) = exchange_raw(
        "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"nope\"}\n{{{\n{\"jsonrpc\":\"1.0\",\"id\":8,\"method\":\"initialize\"}\n",
    );
    assert_eq!(code, 0, "EOF 以 0 结束");
    assert_eq!(resp[0]["error"]["code"], -32601);
    assert_eq!(resp[0]["id"], 7);
    assert_eq!(resp[1]["error"]["code"], -32700);
    assert_eq!(resp[2]["error"]["code"], -32600);
    assert_eq!(resp[2]["id"], 8);
}

#[test]
fn domain_param_errors() {
    let (_, resp) = exchange(&[
        req(1, "compile", json!({ "source": STORY })),
        req(2, "session.open", json!({ "story_id": "sX" })),
        req(3, "compile", json!({})),
        req(4, "session.open", json!({ "story_id": "s1" })),
        req(
            5,
            "session.choose",
            json!({ "session_id": "c1", "index": 9 }),
        ),
        req(6, "export", json!({ "story_id": "s1", "format": "pdf" })),
        req(7, "shutdown", json!({})),
    ]);
    assert_eq!(resp[1]["error"]["code"], -32602, "未知 story_id");
    assert_eq!(resp[2]["error"]["code"], -32602, "缺 path/source");
    assert_eq!(resp[4]["error"]["code"], -32602, "选择越界");
    assert_eq!(resp[5]["error"]["code"], -32602, "未知导出格式");
    assert_eq!(resp[4]["id"], 5);
}

#[test]
fn notification_gets_no_response() {
    let (code, resp) = exchange_raw("{\"jsonrpc\":\"2.0\",\"method\":\"shutdown\"}\n");
    assert_eq!(code, 0, "通知 shutdown 仍然生效");
    assert!(resp.is_empty(), "通知不产生响应");
}

#[test]
fn fingerprint_mismatch_is_story_failure() {
    let (_, first) = exchange(&[
        req(1, "compile", json!({ "source": STORY })),
        req(2, "session.open", json!({ "story_id": "s1" })),
        req(3, "session.continue", json!({ "session_id": "c1" })),
        req(4, "session.save", json!({ "session_id": "c1" })),
    ]);
    let save = first[3]["result"]["save"].as_str().unwrap().to_string();
    let (_, second) = exchange(&[
        req(
            1,
            "compile",
            json!({ "source": "event start\n  改过的开场。\n  choice \"甲\"\n    甲线。\n    -> END\n" }),
        ),
        req(2, "session.open", json!({ "story_id": "s1", "save": save })),
        req(3, "shutdown", json!({})),
    ]);
    let r = &second[1]["result"];
    assert_eq!(r["ok"], false, "指纹不匹配应拒绝读档");
    assert!(r["run_error"].is_object());
}

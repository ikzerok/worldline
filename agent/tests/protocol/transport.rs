use super::*;
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

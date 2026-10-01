use serde_json::{json, Value};
const SOURCE: &str = "event start\n  choice \"档案室\" enable false disabled \"还缺银钥匙\"\n    -> END\n  choice \"离开\"\n    -> END\n";
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    assert_eq!(
        worldline_agent::run(&mut std::io::Cursor::new(input), &mut output),
        0
    );
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
#[test]
fn negotiated_rpc_rejects_disabled_without_advancing_legacy_session_stays_compatible() {
    let rows = exchange(vec![
        request(1, "initialize", json!({})),
        request(
            2,
            "compile",
            json!({"source":SOURCE,"language_version":"1.12"}),
        ),
        request(3, "session.open", json!({"story_id":"s1","seed":42})),
        request(4, "session.continue", json!({"session_id":"c1"})),
        request(
            5,
            "session.open",
            json!({"story_id":"s1","seed":42,"capabilities":["runtime.choice_presentation.v1"]}),
        ),
        request(6, "session.continue", json!({"session_id":"c2"})),
        request(7, "session.save", json!({"session_id":"c2"})),
        request(
            8,
            "session.choose",
            json!({"session_id":"c2","presentation_index":0}),
        ),
        request(9, "session.save", json!({"session_id":"c2"})),
        request(10, "session.continue", json!({"session_id":"c2"})),
        request(
            11,
            "session.choose",
            json!({"session_id":"c2","presentation_index":1}),
        ),
        request(12, "session.continue", json!({"session_id":"c2"})),
        request(
            13,
            "session.choose",
            json!({"session_id":"c1","presentation_index":0}),
        ),
    ]);
    assert!(rows[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("runtime.choice_presentation.v1")));
    assert_eq!(rows[1]["result"]["ok"], true, "{}", rows[1]);
    let old = &rows[3]["result"];
    let new = &rows[5]["result"];
    assert_eq!(old["choices"], new["choices"]);
    assert!(old.get("choice_presentation").is_none());
    assert_eq!(new["choice_presentation"][0]["enabled"], false);
    assert_eq!(new["choice_presentation"][1]["index"], 0);
    assert_eq!(rows[7]["result"]["ok"], false);
    assert!(rows[7].get("error").is_none(), "禁用选项不是协议错误");
    assert_eq!(rows[6]["result"], rows[8]["result"]);
    assert_eq!(rows[9]["result"]["state"], new["state"]);
    assert_eq!(
        rows[9]["result"]["choice_presentation"],
        new["choice_presentation"]
    );
    assert_eq!(rows[11]["result"]["ended"], true);
    assert_eq!(rows[11]["result"]["state"]["turns"], 1);
    assert_eq!(rows[12]["error"]["code"], -32602);
}
#[test]
fn capability_shape_and_ambiguous_selectors_are_protocol_errors() {
    let rows = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":SOURCE,"language_version":"1.12"}),
        ),
        request(
            2,
            "session.open",
            json!({"story_id":"s1","capabilities":"runtime.choice_presentation.v1"}),
        ),
        request(
            3,
            "session.open",
            json!({"story_id":"s1","capabilities":["unknown.future.v1"]}),
        ),
        request(4, "session.continue", json!({"session_id":"c1"})),
        request(
            5,
            "session.choose",
            json!({"session_id":"c1","index":0,"choice_id":"x"}),
        ),
    ]);
    assert_eq!(rows[1]["error"]["code"], -32602);
    assert_eq!(rows[2]["result"]["capabilities"], json!([]));
    assert_eq!(rows[4]["error"]["code"], -32602);
}

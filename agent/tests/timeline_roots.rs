use serde_json::{json, Value};
use std::io::Cursor;
const SOURCE: &str = "period year\nperiod summer within year\nperiod autumn within year\nevent opening during summer\n  开幕\nevent closing during autumn follows opening\n  闭幕\n";

fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    assert_eq!(worldline_agent::run(&mut Cursor::new(input), &mut out), 0);
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})
}

#[test]
fn rpc_root_timeline_matches_core_and_mermaid_preserves_bands() {
    let result = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":SOURCE, "language_version":"1.13"}),
        ),
        request(2, "analyze", json!({"story_id":"s1"})),
        request(
            3,
            "export",
            json!({"story_id":"s1", "format":"timeline_mermaid"}),
        ),
    ]);
    assert_eq!(result[0]["result"]["ok"], true, "{:?}", result);
    let timeline = &result[1]["result"]["timeline"];
    let core = worldline_core::compile_source_with_options(
        "未命名.wl",
        SOURCE,
        worldline_core::CompileOptions::v1_13(),
    );
    assert_eq!(
        *timeline,
        serde_json::to_value(core.analysis.timeline).unwrap()
    );
    assert_eq!(timeline["events"][1]["period"], "autumn");
    assert_eq!(timeline["events"][1]["rank"], 0);
    assert_eq!(timeline["events"][1]["root_rank"], 1);
    assert_eq!(timeline["status"], "complete");
    let mermaid = result[2]["result"]["text"].as_str().unwrap();
    assert_eq!(mermaid.matches("subgraph").count(), 3);
    assert!(mermaid.contains("先于"));
}

#[test]
fn rpc_invalid_timeline_is_story_failure_with_partial_projection() {
    let result = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":SOURCE, "language_version":"1.12"}),
        ),
        request(
            2,
            "compile",
            json!({"source":"period\n", "language_version":"1.13"}),
        ),
        request(
            3,
            "compile",
            json!({"source":SOURCE.replace("follows opening", "follows absent"), "language_version":"1.13"}),
        ),
    ]);
    for response in result {
        assert!(response.get("error").is_none(), "{response}");
        let result = &response["result"];
        assert_eq!(result["ok"], false);
        assert!(result.get("story_id").is_none());
        assert_eq!(result["timeline"]["status"], "partial");
        assert!(!result["diagnostics"].as_array().unwrap().is_empty());
    }
}

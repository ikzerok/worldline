use serde_json::{json, Value};
use std::fs;
#[test]
fn object_search_rpc_is_paged_filtered_strict_and_read_only() {
    let root = std::env::temp_dir().join(format!("wl-object-search-rpc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let source = (0..45)
        .map(|n| format!("event e{n:03} as \"同名\"\n  -> END\n"))
        .collect::<String>();
    fs::write(root.join("world.wl"), &source).unwrap();
    let message = |id, method, params| {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()
    };
    let filter = json!({"allowed_kinds":["event"],"match_source_path":true});
    let messages = [
        message(1, "project.open", json!({"path":root})),
        message(
            2,
            "world.objects.search",
            json!({"project_id":"p1","query":"world.wl","filter":filter,"options":{"offset":40,"limit":10}}),
        ),
        message(
            3,
            "world.objects.search",
            json!({"project_id":"p1","query":"同名","options":{"max_candidates":1}}),
        ),
        message(
            4,
            "world.objects.search",
            json!({"project_id":"p1","query":"","expected_baseline":"stale"}),
        ),
        message(
            5,
            "world.objects.search",
            json!({"project_id":"p1","query":"","filter":{"unknown":true}}),
        ),
        message(
            6,
            "world.objects.search",
            json!({"project_id":"p1","query":"","options":{"limit":0}}),
        ),
        message(
            7,
            "world.objects.search",
            json!({"project_id":"p1","query":"","options":{"offset":-1}}),
        ),
        message(
            8,
            "world.objects.search",
            json!({"project_id":"p1","query":"","filter":{"allowed_kinds":["unknown"]}}),
        ),
    ];
    let mut output = Vec::new();
    worldline_agent::run(&mut std::io::Cursor::new(messages.join("\n")), &mut output);
    let result: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(result[1]["result"]["ok"], true);
    assert_eq!(result[1]["result"]["page"]["total"], 45);
    assert_eq!(
        result[1]["result"]["page"]["items"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        result[1]["result"]["page"]["items"][4]["target"]["id"],
        "e044"
    );
    assert_eq!(
        result[2]["result"]["error"]["code"],
        "CANDIDATE_BUDGET_EXCEEDED"
    );
    assert_eq!(result[3]["result"]["error"]["code"], "STALE_BASELINE");
    assert_eq!(result[4]["error"]["code"], -32602);
    assert_eq!(result[5]["result"]["error"]["code"], "INVALID_LIMIT");
    assert_eq!(result[6]["error"]["code"], -32602);
    assert_eq!(result[7]["result"]["page"]["total"], 0);
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), source);
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

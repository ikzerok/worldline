use serde_json::{json, Value};
use std::{io::Cursor, path::PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "agent-catalog-scope-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(".world")).unwrap();
        std::fs::write(root.join(".world/project.json"),r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","content.relations.v1"]}"#).unwrap();
        std::fs::write(root.join("world.wl"),"entity alpha kind place\nentity beta kind place\nrelation_type linked\nrelation_def edge type linked from entity alpha to entity beta\nevent start\n  -> END\n").unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn call(id: Value, params: Value) -> (String, Value) {
    let request = json!({"jsonrpc":"2.0","id":id,"method":"catalog.scope","params":params});
    let mut out = Vec::new();
    worldline_agent::run(&mut Cursor::new(format!("{request}\n")), &mut out);
    let text = String::from_utf8(out).unwrap();
    let value = serde_json::from_str(text.trim()).unwrap();
    (text, value)
}
#[test]
fn rpc_scope_returns_complete_typed_range_and_one_page_without_writing() {
    let f = Fixture::new();
    let before = std::fs::read(f.0.join("world.wl")).unwrap();
    let (_, value) = call(
        json!(1),
        json!({"path":f.0,"query":{"schema_version":1,"filters":[{"dimension":"kind","values":["entity"]}]},"page_size":1,"focus":{"kind":"entity","id":"alpha"}}),
    );
    let result = &value["result"];
    assert_eq!(result["ok"], true, "{value}");
    assert_eq!(result["scope"]["counts"]["matching_objects"], 2);
    assert_eq!(result["page"]["total"], 2);
    assert_eq!(result["page"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(result["relations"]["edges"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read(f.0.join("world.wl")).unwrap(), before);
}
#[test]
fn protocol_and_business_failures_are_distinct_and_giant_id_is_rejected_before_echo() {
    let f = Fixture::new();
    let (_, value) = call(
        json!(1),
        json!({"path":f.0,"project_id":"p1","query":{"schema_version":1}}),
    );
    assert_eq!(value["error"]["code"], -32602);
    let (_, value) = call(json!(2), json!({"path":f.0,"query":{"schema_version":99}}));
    assert_eq!(value["result"]["ok"], false);
    assert_eq!(value["result"]["error"]["code"], "INVALID_QUERY");
    let (text, value) = call(
        json!("\"".repeat(3073)),
        json!({"path":f.0,"query":{"schema_version":1}}),
    );
    assert_eq!(value["id"], Value::Null);
    assert_eq!(value["error"]["code"], -32600);
    assert!(text.len() < 4096);
}
#[test]
fn query_budget_failure_has_no_partial_scope() {
    let f = Fixture::new();
    let (_, value) = call(
        json!(1),
        json!({"path":f.0,"query":{"schema_version":1},"max_candidates":1}),
    );
    assert_eq!(value["result"]["ok"], false);
    assert_eq!(value["result"]["scope"], Value::Null);
    assert_eq!(
        value["result"]["error"]["code"],
        "CANDIDATE_BUDGET_EXCEEDED"
    );
}

#[path = "catalog_scope/read_only.rs"]
mod read_only;

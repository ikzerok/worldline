use serde_json::{json, Value};
use std::io::Cursor;
#[path = "../../cli/tests/support/world_context_fixture.rs"]
mod fixture;
use fixture::Fixture;
fn request(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    assert_eq!(worldline_agent::run(&mut Cursor::new(input), &mut out), 0);
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
#[test]
fn path_and_project_context_match_core_and_advertise_capability() {
    let fixture = Fixture::new();
    let target = json!({"kind":"character","id":"b"});
    let responses = exchange(vec![
        request("initialize", json!({})),
        request(
            "world.context",
            json!({"path":fixture.root,"target":target}),
        ),
        request("project.open", json!({"path":fixture.root})),
        request("world.context", json!({"project_id":"p1","target":target})),
        request("world.object", json!({"project_id":"p1","target":target})),
    ]);
    assert!(responses[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("authoring.world_context.v1")));
    assert_eq!(responses[1]["result"]["ok"], true, "{:?}", responses);
    assert_eq!(
        responses[1]["result"]["context"],
        responses[3]["result"]["context"]
    );
    assert_eq!(responses[4]["result"]["object"]["target"], target);
    let project = worldline_core::project::Project::open(&fixture.root).unwrap();
    let expected = project
        .query_world_context(
            &worldline_core::TargetRef::new("character", "b"),
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        responses[1]["result"]["context"],
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
        fixture::SOURCE
    );
}
#[test]
fn strict_protocol_fields_and_business_failure_contract() {
    let fixture = Fixture::new();
    let target = json!({"kind":"character","id":"b"});
    let responses = exchange(vec![
        request(
            "world.context",
            json!({"path":fixture.root,"target":{"kind":"character","id":"missing"}}),
        ),
        request(
            "world.context",
            json!({"path":fixture.root,"target":target,"options":{"expected_snapshot":"old"}}),
        ),
        request(
            "world.context",
            json!({"path":fixture.root,"project_id":"p1","target":target}),
        ),
        request(
            "world.context",
            json!({"path":fixture.root,"target":target,"options":{"depth":3}}),
        ),
        request(
            "world.context",
            json!({"path":fixture.root,"target":target,"options":{"extra":true}}),
        ),
        request(
            "world.object",
            json!({"path":fixture.root,"target":target,"options":{}}),
        ),
    ]);
    assert_eq!(responses[0]["result"]["error"]["code"], "UNKNOWN_TARGET");
    assert_eq!(responses[1]["result"]["error"]["code"], "STALE_SNAPSHOT");
    for response in &responses[2..] {
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
}
#[test]
fn comparison_and_invalid_source_are_readable_business_results() {
    let fixture = Fixture::new();
    let responses = exchange(vec![
        request(
            "temporal.compare",
            json!({"path":fixture.root,"left":"first","right":"second"}),
        ),
        request(
            "temporal.compare",
            json!({"path":fixture.root,"left":"first","right":"second","expected_baseline":"old"}),
        ),
    ]);
    assert_eq!(responses[0]["result"]["comparison"]["relation"], "before");
    assert_eq!(responses[1]["result"]["error"]["code"], "STALE_BASELINE");
    std::fs::write(
        fixture.root.join("world.wl"),
        fixture::SOURCE.replace("character b as \"乙\"", ""),
    )
    .unwrap();
    let responses = exchange(vec![request(
        "world.context",
        json!({"path":fixture.root,"target":{"kind":"character","id":"a"}}),
    )]);
    assert_eq!(responses[0]["result"]["ok"], false);
    assert_eq!(responses[0]["result"]["context"]["complete"], false);
    assert!(responses[0]["result"]["context"]["reasons"]
        .as_array()
        .unwrap()
        .contains(&json!("invalid_source")));
}

#[test]
fn io_failures_keep_the_common_query_envelope() {
    let root = std::env::temp_dir().join(format!("world-context-missing-{}", std::process::id()));
    let responses = exchange(vec![
        request(
            "world.context",
            json!({"path":root,"target":{"kind":"character","id":"b"}}),
        ),
        request(
            "world.object",
            json!({"path":root,"target":{"kind":"character","id":"b"}}),
        ),
        request(
            "temporal.compare",
            json!({"path":root,"left":"a","right":"b"}),
        ),
    ]);
    for response in responses {
        let value = &response["result"];
        assert_eq!(value["error"]["code"], "IO_ERROR");
        assert_eq!(value["schema_version"], 1);
        for field in [
            "language_version",
            "workspace_revision",
            "diagnostics",
            "workspace_diagnostics",
            "read_only",
        ] {
            assert!(value.get(field).is_some(), "missing {field}: {value}");
        }
    }
}

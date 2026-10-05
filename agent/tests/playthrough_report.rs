use serde_json::{json, Value};
use std::io::Cursor;
#[path = "../../cli/tests/support/route_comparison_fixture.rs"]
mod fixture;
use fixture::{request, Fixture, SOURCE};
const MAX_WIRE: usize = 1024 * 1024 + 4096;
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    worldline_agent::run(&mut Cursor::new(input), &mut output);
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            if value["result"].get("report").is_some() || value.get("error").is_some() {
                assert!(line.len() < MAX_WIRE);
            }
            value
        })
        .collect()
}
fn report(fixture: &Fixture, params: Value) -> Value {
    exchange(vec![
        request("project.open", json!({"path":fixture.root})),
        request("project.playthrough_report", params),
    ])
    .remove(1)
}

#[test]
fn advertised_report_is_read_only_and_uses_the_shared_runtime_producer() {
    let fixture = Fixture::new();
    let (trace, _) = fixture.traces();
    let expected = worldline_runtime::generate_playthrough_report(
        &fixture.compile(),
        &trace,
        Default::default(),
        &worldline_runtime::ReplayCancellation::new(),
    )
    .unwrap();
    let rows = exchange(vec![
        request("initialize", json!({})),
        request("project.open", json!({"path":fixture.root})),
        request("compile", json!({"path":fixture.root})),
        request("session.open", json!({"story_id":"s1","seed":73})),
        request("session.continue", json!({"session_id":"c1"})),
        request("session.save", json!({"session_id":"c1"})),
        request("session.trace", json!({"session_id":"c1"})),
        request(
            "project.playthrough_report",
            json!({"project_id":"p1","trace":trace}),
        ),
        request("session.save", json!({"session_id":"c1"})),
        request("session.trace", json!({"session_id":"c1"})),
    ]);
    assert!(rows[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("authoring.playthrough_report.v1")));
    assert_eq!(rows[7]["result"]["ok"], true, "{}", rows[7]);
    let report = &rows[7]["result"]["report"];
    assert_eq!(report["observations"], json!(expected.observations));
    assert_eq!(report["source_snapshot"], expected.source_snapshot);
    assert_eq!(report["origin"], json!(expected.origin));
    assert!(!report
        .to_string()
        .contains(&fixture.root.display().to_string()));
    assert_eq!(rows[5]["result"], rows[8]["result"]);
    assert_eq!(rows[6]["result"], rows[9]["result"]);
    assert_eq!(fixture.source(), SOURCE);
}

#[test]
fn protocol_and_story_failures_are_distinct_and_partial_report_is_honest() {
    let fixture = Fixture::new();
    let trace = fixture::record(&fixture.compile(), 0, false);
    let params = json!({"project_id":"p1","trace":trace});
    let row = report(&fixture, params.clone());
    assert_eq!(row["result"]["ok"], true);
    assert_eq!(row["result"]["report"]["complete"], false);
    for (key, value) in [
        ("unknown", json!(true)),
        ("max_steps", json!(-1)),
        ("max_steps", json!(100001)),
        ("time_budget_ms", json!(30001)),
        ("trace", json!({})),
        ("project_id", json!("unknown")),
        ("project_id", json!("")),
    ] {
        let mut invalid = params.clone();
        invalid[key] = value;
        assert_eq!(report(&fixture, invalid)["error"]["code"], -32602);
    }
    let mut zero = params.clone();
    zero["max_steps"] = json!(0);
    let row = report(&fixture, zero);
    assert!(row.get("error").is_none());
    assert_eq!(row["result"]["ok"], false);
    assert_eq!(row["result"]["report"]["status"], "step_budget_exceeded");
    fixture.replace("event start\n  -> missing\n");
    let row = report(&fixture, params);
    assert_eq!(row["result"]["ok"], false);
    assert!(row.get("error").is_none());
    assert!(row["result"]["report"].is_null());
}

#[test]
fn escaped_ids_error_details_and_notifications_obey_wire_contract() {
    let fixture = Fixture::new();
    let (trace, _) = fixture.traces();
    let params = json!({"project_id":"p1","trace":trace});
    let mut huge = request("project.playthrough_report", params.clone());
    huge["id"] = json!("\n".repeat(MAX_WIRE / 2));
    let mut large = request("project.playthrough_report", params.clone());
    large["id"] = json!("\n".repeat(MAX_WIRE / 2 - 400));
    let mut unknown = params.clone();
    unknown["x".repeat(MAX_WIRE)] = json!(true);
    let mut notification = request("project.playthrough_report", params);
    notification.as_object_mut().unwrap().remove("id");
    let rows = exchange(vec![
        request("project.open", json!({"path":fixture.root})),
        huge,
        large,
        request("project.playthrough_report", unknown),
        notification,
    ]);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[1]["error"]["code"], -32600);
    assert!(rows[1]["id"].is_null());
    assert_eq!(rows[2]["result"]["error"]["code"], "output_limit");
    assert_eq!(rows[3]["error"]["code"], -32602);
}

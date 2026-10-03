use serde_json::{json, Value};
use std::io::{BufRead, Cursor, Read};
use worldline_runtime::{compare_routes, ReplayCancellation, RouteComparisonOptions};
#[path = "../../cli/tests/support/route_comparison_fixture.rs"]
mod fixture;
use fixture::{request, Fixture, SOURCE};
const MAX_WIRE: usize = 1024 * 1024 + 4096;

fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests.into_iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n");
    exchange_reader(&mut Cursor::new(input))
}
fn exchange_reader(reader: &mut impl BufRead) -> Vec<Value> {
    let mut output = Vec::new();
    assert_eq!(worldline_agent::run(reader, &mut output), 0);
    String::from_utf8(output).unwrap().lines().map(|line| {
        // initialize/project/session retain their existing protocol; only comparison is capped.
        let value: Value = serde_json::from_str(line).unwrap();
        if value["result"].get("comparison").is_some() || value.get("error").is_some() {
            assert!(line.len() < MAX_WIRE);
        }
        value
    }).collect()
}
fn params(left: &Value, right: &Value) -> Value {
    json!({"project_id":"p1","left_trace":left,"right_trace":right})
}
fn compare(fixture: &Fixture, params: Value) -> Value {
    exchange(vec![request("project.open", json!({"path":fixture.root})), request("project.compare_routes", params)]).remove(1)
}

#[test]
fn advertised_pair_is_exactly_the_runtime_projection_and_preserves_live_story() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let expected = compare_routes(&fixture.compile(), &left, &right, RouteComparisonOptions::default(), &ReplayCancellation::new()).unwrap();
    let rows = exchange(vec![
        request("initialize", json!({})),
        request("project.open", json!({"path":fixture.root})),
        request("compile", json!({"path":fixture.root})),
        request("session.open", json!({"story_id":"s1","seed":73})),
        request("session.continue", json!({"session_id":"c1"})),
        request("session.save", json!({"session_id":"c1"})),
        request("session.trace", json!({"session_id":"c1"})),
        request("project.compare_routes", params(&json!(left), &json!(right))),
        request("session.save", json!({"session_id":"c1"})),
        request("session.trace", json!({"session_id":"c1"})),
    ]);
    assert!(rows[0]["result"]["capabilities"].as_array().unwrap().contains(&json!("authoring.route_comparison.v1")));
    assert_eq!(rows[7]["result"]["ok"], true, "{}", rows[7]);
    assert_eq!(rows[7]["result"]["comparison"], json!(expected));
    assert_eq!(rows[5]["result"], rows[8]["result"]);
    assert_eq!(rows[6]["result"], rows[9]["result"]);
    assert_eq!(fixture.source(), SOURCE);
}

#[test]
fn partial_checkpoint_one_sided_divergence_and_story_failure_are_business_results() {
    let fixture = Fixture::new();
    let compiled = fixture.compile();
    let left = json!(fixture::record(&compiled, 0, false));
    let right = json!(fixture::record(&compiled, 1, true));
    let row = compare(&fixture, params(&left, &right));
    assert_eq!(row["result"]["ok"], true);
    assert_eq!(row["result"]["comparison"]["left"]["complete"], false);
    assert_eq!(row["result"]["comparison"]["left"]["ended"], false);
    let checkpoint = json!(fixture::checkpoint_trace(&compiled));
    let row = compare(&fixture, params(&checkpoint, &checkpoint));
    let side = &row["result"]["comparison"]["left"];
    assert_eq!(side["coverage"]["executed"]["visited_nodes"], json!({}));
    assert_eq!(side["coverage"]["executed"]["selected_choices"], json!([]));
    assert_eq!(side["state_actions"]["records"], json!([]));
    assert!(!side["coverage"]["inherited"]["visited_nodes"].as_object().unwrap().is_empty());
    fixture.replace(&SOURCE.replace("set credits = 7", "set credits = 8"));
    let row = compare(&fixture, params(&left, &right));
    assert!(row.get("error").is_none());
    assert_eq!(row["result"]["ok"], false);
    assert_eq!(row["result"]["comparison"]["left"]["status"], "replayed");
    assert_eq!(row["result"]["comparison"]["right"]["status"], "diverged");
    assert_eq!(row["result"]["comparison"]["right"]["vars"]["credits"]["Num"], 8.0);
    fixture.replace(&SOURCE.replace("set credits = 7", "set credits = 1 / 0"));
    let row = compare(&fixture, params(&left, &right));
    assert!(row.get("error").is_none());
    assert_eq!(row["result"]["comparison"]["left"]["status"], "replayed");
    assert_eq!(row["result"]["comparison"]["right"]["status"], "story_failed");
}

#[test]
fn strict_params_and_trace_limits_are_protocol_errors() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let good = params(&json!(left), &json!(right));
    let mut invalid = vec![Value::Null, json!({}), json!({"project_id":"unknown","left_trace":left,"right_trace":right})];
    for (key, value) in [("unknown", json!(true)), ("path", json!(fixture.root)), ("story_id", json!("s1")), ("max_steps", json!(-1)), ("max_steps", json!(100001)), ("time_budget_ms", json!(30001)), ("time_budget_ms", json!("1")), ("project_id", json!(""))] {
        let mut value_params = good.clone(); value_params[key] = value; invalid.push(value_params);
    }
    for (key, value) in [("runtime_version", json!("0.19.0")), ("schema_version", json!(2))] {
        let mut value_params = good.clone(); value_params["left_trace"][key] = value; invalid.push(value_params);
    }
    let mut malformed = good.clone(); malformed["right_trace"] = json!({}); invalid.push(malformed);
    let mut excessive = good.clone(); excessive["left_trace"]["ignored_extension"] = json!("x".repeat(4 * 1024 * 1024)); invalid.push(excessive);
    let mut requests = vec![request("project.open", json!({"path":fixture.root}))];
    requests.extend(invalid.into_iter().map(|p| request("project.compare_routes", p)));
    let rows = exchange(requests);
    for row in &rows[1..] { assert_eq!(row["error"]["code"], -32602, "{row}"); }
    assert_eq!(fixture.source(), SOURCE);
}

#[test]
fn zero_budget_unknown_trace_extensions_and_compile_failure_keep_separate_domains() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let mut p = params(&json!(left), &json!(right));
    p["left_trace"]["optional_future_field"] = json!(true);
    let row = compare(&fixture, p.clone());
    assert_eq!(row["result"]["ok"], true);
    p["max_steps"] = json!(0);
    let row = compare(&fixture, p);
    assert!(row.get("error").is_none());
    assert_eq!(row["result"]["ok"], false);
    for side in ["left", "right"] {
        assert_eq!(row["result"]["comparison"][side]["status"], "step_budget_exceeded");
        assert_eq!(row["result"]["comparison"][side]["executed_steps"], 0);
    }
    fixture.replace("event start\n  -> unknown\n");
    let row = compare(&fixture, params(&json!(left), &json!(right)));
    assert!(row.get("error").is_none());
    assert_eq!(row["result"]["ok"], false);
    assert!(row["result"]["comparison"].is_null());
    assert!(!row["result"]["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn escaped_request_id_and_error_details_cannot_exceed_total_wire_limit() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let good = params(&json!(left), &json!(right));
    let mut huge_id = request("project.compare_routes", good.clone());
    huge_id["id"] = json!("\n".repeat(MAX_WIRE / 2));
    let mut huge_error = good.clone();
    huge_error["x".repeat(MAX_WIRE)] = json!(true);
    let mut notification = request("project.compare_routes", good);
    notification.as_object_mut().unwrap().remove("id");
    let rows = exchange(vec![request("project.open", json!({"path":fixture.root})), huge_id, request("project.compare_routes", huge_error), notification]);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1]["error"]["code"], -32600);
    assert!(rows[1]["id"].is_null());
    assert_eq!(rows[1]["error"]["data"], "request_id_exceeds_response_budget");
    assert_eq!(rows[2]["error"]["code"], -32602);
    assert_eq!(rows[2]["id"], 1);
}

struct MutatingReader {
    inner: Cursor<Vec<u8>>,
    line: usize,
    path: std::path::PathBuf,
}
impl Read for MutatingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> { self.inner.read(buffer) }
}
impl BufRead for MutatingReader {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> { self.inner.fill_buf() }
    fn consume(&mut self, n: usize) { self.inner.consume(n); }
    fn read_line(&mut self, buffer: &mut String) -> std::io::Result<usize> {
        if self.line == 1 { std::fs::write(&self.path, SOURCE.replace("set credits = 7", "set credits = 9"))?; }
        self.line += 1;
        self.inner.read_line(buffer)
    }
}
#[test]
fn comparison_uses_applied_project_buffer_without_refreshing_external_changes() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let requests = [request("project.open", json!({"path":fixture.root})), request("project.compare_routes", params(&json!(left), &json!(right)))];
    let input = requests.iter().map(|v| format!("{v}\n")).collect::<String>();
    let rows = exchange_reader(&mut MutatingReader { inner: Cursor::new(input.into_bytes()), line: 0, path: fixture.root.join("world.wl") });
    assert_eq!(rows[1]["result"]["ok"], true, "{}", rows[1]);
    assert_eq!(rows[1]["result"]["comparison"]["right"]["vars"]["credits"]["Num"], 7.0);
    assert_eq!(fixture.source(), SOURCE.replace("set credits = 7", "set credits = 9"));
}

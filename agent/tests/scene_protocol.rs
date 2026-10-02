use serde_json::{json, Value};
use std::io::Cursor;
#[path = "../../core/tests/support/scene_protocol_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn request(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}
fn exchange(values: &[Value]) -> Vec<Value> {
    let input = values
        .iter()
        .map(Value::to_string)
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
fn path_preview_matches_core_then_apply_saves_and_replay_is_stale() {
    let fixture = Fixture::new("agent-path");
    let before = fixture.bytes();
    let batch = fixture.batch();
    let (baseline, digest, plan) = fixture.preview(&batch);
    let preview = exchange(&[request(
        "scene.preview",
        json!({"path":fixture.root,"batch":batch}),
    )]);
    assert_eq!(preview[0]["result"]["plan"], plan);
    assert_eq!(preview[0]["result"]["plan_digest"], digest);
    assert_eq!(fixture.bytes(), before);
    let params =
        json!({"path":fixture.root,"batch":batch,"baseline":baseline,"plan_digest":digest});
    let response = exchange(&[
        request("scene.apply", params.clone()),
        request("scene.apply", params),
    ]);
    assert_eq!(response[0]["result"]["ok"], true, "{response:?}");
    assert_eq!(response[1]["result"]["error"]["code"], "SCENE_STALE");
    assert!(fixture.project().map_index().maps["atlas"]
        .scene
        .as_ref()
        .unwrap()
        .nodes
        .contains_key("rect_1"));
    assert_ne!(fixture.bytes(), before);
}

#[test]
fn project_session_keeps_its_scene_revision_and_exports_current_scene() {
    let fixture = Fixture::new("agent-session");
    let batch = fixture.batch();
    let (baseline, digest, plan) = fixture.preview(&batch);
    let response = exchange(&[
        request("project.open", json!({"path":fixture.root})),
        request("scene.preview", json!({"project_id":"p1","batch":batch})),
        request(
            "scene.apply",
            json!({"project_id":"p1","batch":batch,"baseline":baseline,"plan_digest":digest}),
        ),
        request("scene.export", json!({"project_id":"p1","map_id":"atlas"})),
    ]);
    assert_eq!(response[1]["result"]["plan"], plan);
    assert_eq!(response[2]["result"]["ok"], true, "{response:?}");
    assert_eq!(
        response[2]["result"]["revision"]["presentation_generation"],
        1
    );
    let svg = response[3]["result"]["svg"].as_str().unwrap();
    assert!(svg.contains("<rect"));
    assert!(worldline_core::svg_import::preview_scene(svg).is_ok());
}

#[test]
fn malformed_addressing_and_dto_are_protocol_errors_without_writes() {
    let fixture = Fixture::new("agent-params");
    let before = fixture.bytes();
    for params in [
        json!({}),
        json!({"path":fixture.root,"project_id":"p1","batch":fixture.batch()}),
        json!({"path":fixture.root,"batch":{}}),
        json!({"source":"x","path":fixture.root,"batch":fixture.batch()}),
        json!({"project_id":"missing","batch":fixture.batch()}),
    ] {
        let response = exchange(&[request("scene.preview", params)]);
        assert_eq!(response[0]["error"]["code"], -32602, "{response:?}");
    }
    assert_eq!(fixture.bytes(), before);
}

#[test]
fn tampered_digest_revision_and_readonly_feature_never_apply() {
    let fixture = Fixture::new("agent-failure");
    let before = fixture.bytes();
    let mut batch = fixture.batch();
    let (baseline, _, _) = fixture.preview(&batch);
    let result = exchange(&[request(
        "scene.apply",
        json!({"path":fixture.root,"batch":batch,"baseline":baseline,"plan_digest":"forged"}),
    )]);
    assert_eq!(result[0]["result"]["error"]["code"], "SCENE_STALE");
    batch.expected_revision.presentation_generation = 99;
    let result = exchange(&[request(
        "scene.preview",
        json!({"path":fixture.root,"batch":batch}),
    )]);
    assert_eq!(result[0]["result"]["error"]["code"], "SCENE_STALE");
    assert_eq!(fixture.bytes(), before);
    let mut map: Value = serde_json::from_slice(&before).unwrap();
    map["required_features"] = json!(["unknown.scene.v99"]);
    let readonly = serde_json::to_vec(&map).unwrap();
    std::fs::write(&fixture.map, &readonly).unwrap();
    let result = exchange(&[request(
        "scene.preview",
        json!({"path":fixture.root,"batch":fixture.batch()}),
    )]);
    assert_eq!(result[0]["result"]["ok"], false);
    assert_eq!(fixture.bytes(), readonly);
}

#[test]
fn svg_profile_errors_are_typed_business_failures_and_curves_remain_native() {
    let response = exchange(&[
        request("initialize", json!({})),
        request(
            "scene.svg.preview",
            json!({"source":"<svg width='100' height='100'><path d='M0 0 C10 20 30 40 50 60'/></svg>"}),
        ),
        request("scene.svg.preview", json!({"source":"<svg><svg/></svg>"})),
        request(
            "scene.svg.preview",
            json!({"source":"<svg><image href='https://example.invalid/x'/></svg>"}),
        ),
    ]);
    assert!(response[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("authoring.vector_scene.v1")));
    assert_eq!(response[1]["result"]["ok"], true);
    assert!(response[1]["result"]["preview"]
        .to_string()
        .contains("cubic"));
    for result in &response[2..] {
        assert_eq!(result["result"]["ok"], false);
        assert!(result.get("error").is_none());
    }
}

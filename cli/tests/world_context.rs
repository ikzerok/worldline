use serde_json::{json, Value};
use std::io::Cursor;
#[path = "support/world_context_fixture.rs"]
mod fixture;
use fixture::Fixture;
fn invoke(fixture: &Fixture, mode: &str, extra: &[&str]) -> Result<(i32, Value), String> {
    let mut args: Vec<String> = mode.split(' ').map(String::from).collect();
    args.extend([fixture.root.display().to_string(), "--json".into()]);
    args.extend(extra.iter().map(|value| value.to_string()));
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut Cursor::new(""))?;
    Ok((code, serde_json::from_slice(&out).unwrap()))
}
#[test]
fn context_and_exact_object_match_core_without_writes() {
    let fixture = Fixture::new();
    let mut project = worldline_core::project::Project::open(&fixture.root).unwrap();
    let result = project.compile();
    let baseline = project.content_baseline();
    let mut expected = result
        .query_world_context(
            &worldline_core::TargetRef::new("character", "b"),
            Default::default(),
        )
        .unwrap();
    expected.content_baseline = Some(baseline.clone());
    let (code, response) = invoke(&fixture, "world-context", &["--target", "character:b"]).unwrap();
    assert_eq!(code, 0, "{response}");
    assert_eq!(response["context"], serde_json::to_value(expected).unwrap());
    assert_eq!(response["workspace_revision"], baseline);
    let (code, response) = invoke(&fixture, "world-object", &["--target", "character:b"]).unwrap();
    assert_eq!(code, 0);
    assert_eq!(
        response["object"]["target"],
        json!({"kind":"character","id":"b"})
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
        fixture::SOURCE
    );
}
#[test]
fn context_business_errors_and_usage_errors_are_distinct() {
    let fixture = Fixture::new();
    for extra in [
        vec!["--target", "character:missing"],
        vec![
            "--target",
            "character:b",
            "--options-json",
            r#"{"expected_snapshot":"old"}"#,
        ],
    ] {
        let (code, value) = invoke(&fixture, "world-context", &extra).unwrap();
        assert_eq!(code, 1);
        assert_eq!(value["ok"], false);
        assert!(value["context"].is_null());
    }
    for options in [
        r#"{"depth":3}"#,
        r#"{"depth":1,"depth":2}"#,
        r#"{"unknown":true}"#,
    ] {
        assert!(invoke(
            &fixture,
            "world-context",
            &["--target", "character:b", "--options-json", options]
        )
        .is_err());
    }
    let (_, limited) = invoke(
        &fixture,
        "world-context",
        &[
            "--target",
            "character:b",
            "--options-json",
            r#"{"max_records":0}"#,
        ],
    )
    .unwrap();
    assert_eq!(limited["ok"], true);
    assert_eq!(limited["context"]["complete"], false);
    assert_eq!(limited["context"]["total"], 3);
}
#[test]
fn temporal_comparison_has_real_evidence_and_stale_baseline_rejection() {
    let fixture = Fixture::new();
    let (code, response) = invoke(
        &fixture,
        "timeline compare",
        &["--left", "first", "--right", "second"],
    )
    .unwrap();
    assert_eq!(code, 0, "{response}");
    assert_eq!(response["comparison"]["relation"], "before");
    assert_eq!(
        response["comparison"]["evidence"].as_array().unwrap().len(),
        1
    );
    let (code, response) = invoke(
        &fixture,
        "timeline compare",
        &[
            "--left",
            "first",
            "--right",
            "second",
            "--expected-baseline",
            "old",
        ],
    )
    .unwrap();
    assert_eq!(code, 1);
    assert_eq!(response["error"]["code"], "STALE_BASELINE");
    assert!(response["comparison"].is_null());
}

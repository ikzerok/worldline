use serde_json::{json, Value};
use std::io::Cursor;
use worldline_runtime::{compare_routes, ReplayCancellation, RouteComparisonOptions};
#[path = "support/route_comparison_fixture.rs"]
mod fixture;
use fixture::{record, Fixture, SOURCE};

fn invoke(fixture: &Fixture, left: &Value, right: &Value, extra: &[&str]) -> (i32, Value) {
    let mut args = vec![
        "route-compare".into(),
        fixture.root.display().to_string(),
        "--left-trace-json".into(),
        left.to_string(),
        "--right-trace-json".into(),
        right.to_string(),
        "--json".into(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut Cursor::new("")).unwrap();
    assert!(output.len() <= 1024 * 1024 + 4096);
    (code, serde_json::from_slice(&output).unwrap())
}

#[test]
fn same_nodes_different_inputs_and_results_are_the_shared_runtime_projection() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let expected = compare_routes(
        &fixture.compile(),
        &left,
        &right,
        RouteComparisonOptions::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    let (code, value) = invoke(&fixture, &json!(left), &json!(right), &[]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["ok"], true);
    let pair = &value["comparison"];
    assert_eq!(*pair, json!(expected));
    assert_eq!(pair["schema_version"], 1);
    assert_eq!(
        pair["left"]["coverage"]["total"]["visited_nodes"],
        pair["right"]["coverage"]["total"]["visited_nodes"]
    );
    assert_eq!(pair["alignment"]["first_difference"]["index"], 0);
    assert_eq!(pair["left"]["states"]["bell_fate"], json!(["restored"]));
    assert_eq!(pair["right"]["states"]["bell_fate"], json!(["traded"]));
    assert_eq!(pair["right"]["vars"]["credits"]["Num"], 7.0);
    assert_eq!(pair["state_differences"][0]["id"], "bell_fate");
    assert_eq!(pair["variable_differences"][0]["id"], "credits");
    for side in ["left", "right"] {
        let evidence = &pair[side]["variable_writes"];
        assert_eq!(evidence["captured"], true);
        assert_eq!(evidence["total_writes"], u64::from(side == "right"));
        assert_eq!(evidence["omitted"], false);
        if side == "left" {
            assert_eq!(evidence["records"], json!([]));
            continue;
        }
        let write = &evidence["records"][0];
        assert_eq!(write["variable"], "credits");
        assert_eq!(write["operation"], "set");
        assert_eq!(write["before"]["Num"], 0.0);
        assert_eq!(write["after"], pair[side]["vars"]["credits"]);
        assert_eq!(write["source"]["kind"], "variable_write");
    }
    assert_eq!(fixture.source(), SOURCE);
}

#[test]
fn partial_checkpoint_and_zero_budget_keep_their_actual_scope() {
    let fixture = Fixture::new();
    let compiled = fixture.compile();
    let partial = json!(record(&compiled, 0, false));
    let complete = json!(record(&compiled, 1, true));
    let (code, result) = invoke(&fixture, &partial, &complete, &[]);
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["comparison"]["left"]["status"], "replayed");
    assert_eq!(result["comparison"]["left"]["complete"], false);
    assert_eq!(result["comparison"]["left"]["ended"], false);
    let checkpoint = json!(fixture::checkpoint_trace(&compiled));
    let (_, result) = invoke(&fixture, &checkpoint, &checkpoint, &[]);
    let side = &result["comparison"]["left"];
    assert_eq!(side["origin"]["kind"], "checkpoint");
    assert_eq!(side["coverage"]["executed"]["visited_nodes"], json!({}));
    assert_eq!(side["coverage"]["executed"]["selected_choices"], json!([]));
    assert!(
        side["coverage"]["inherited"]["visited_nodes"]
            .as_object()
            .unwrap()
            .len()
            >= 2
    );
    assert_eq!(side["state_actions"]["total_actions"], 0);
    assert_eq!(side["variable_writes"]["captured"], true);
    assert_eq!(side["variable_writes"]["total_writes"], 0);
    let (code, result) = invoke(&fixture, &partial, &complete, &["--max-steps=0"]);
    assert_eq!(code, 1);
    for side in ["left", "right"] {
        assert_eq!(result["comparison"][side]["status"], "step_budget_exceeded");
        assert_eq!(result["comparison"][side]["executed_steps"], 0);
    }
}

#[test]
fn one_sided_divergence_does_not_replace_actual_result_with_recorded_values() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    fixture.replace(&SOURCE.replace("set credits = 7", "set credits = 8"));
    let before = fixture.source();
    let (code, result) = invoke(&fixture, &json!(left), &json!(right), &[]);
    assert_eq!(code, 1);
    assert_eq!(result["ok"], false);
    assert_eq!(result["comparison"]["left"]["status"], "replayed");
    assert_eq!(result["comparison"]["left"]["complete"], true);
    assert_eq!(result["comparison"]["right"]["status"], "diverged");
    assert_eq!(result["comparison"]["right"]["vars"]["credits"]["Num"], 8.0);
    assert_eq!(fixture.source(), before);
}

#[test]
fn comment_shift_uses_current_verified_action_and_choice_locations() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let original = json!(left);
    let fingerprint = fixture.compile().analysis.fingerprint;
    fixture.replace(&format!("// 重新排版\n// 当前源码位置\n{SOURCE}"));
    assert_eq!(fixture.compile().analysis.fingerprint, fingerprint);
    let before = fixture.source();
    let (code, result) = invoke(&fixture, &original, &json!(right), &[]);
    assert_eq!(code, 0, "{result}");
    let pair = &result["comparison"];
    assert_eq!(
        pair["left"]["state_actions"]["records"][0]["source"]["line"],
        12
    );
    assert_eq!(
        pair["right"]["state_actions"]["records"][0]["source"]["line"],
        15
    );
    assert_eq!(
        pair["alignment"]["first_difference"]["left"]["source"]["line"],
        11
    );
    assert_eq!(json!(left), original);
    assert_eq!(fixture.source(), before);
}

#[test]
fn invalid_and_excessive_inputs_are_usage_failures_but_bad_source_is_business_failure() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let left = json!(left);
    let right = json!(right);
    for args in [
        vec!["--unknown"],
        vec!["--max-steps=-1"],
        vec!["--max-steps=100001"],
        vec!["--time-budget-ms=30001"],
        vec!["--max-steps=1", "--max-steps=2"],
    ] {
        let (code, value) = invoke(&fixture, &left, &right, &args);
        assert_eq!(code, 2, "{value}");
        assert_eq!(value["ok"], false);
    }
    for invalid in [
        json!({}),
        {
            let mut v = left.clone();
            v["runtime_version"] = json!("0.19.0");
            v
        },
        {
            let mut v = left.clone();
            v["schema_version"] = json!(99);
            v
        },
    ] {
        let (code, value) = invoke(&fixture, &invalid, &right, &[]);
        assert_eq!(code, 2, "{value}");
        assert!(value["comparison"].is_null());
    }
    let mut excessive = left.clone();
    excessive["unknown_extension"] = json!("x".repeat(4 * 1024 * 1024));
    assert_eq!(invoke(&fixture, &excessive, &right, &[]).0, 2);
    fixture.replace("event start\n  -> missing\n");
    let (code, value) = invoke(&fixture, &left, &right, &[]);
    assert_eq!(code, 1);
    assert_eq!(value["ok"], false);
    assert!(!value["diagnostics"].as_array().unwrap().is_empty());
    assert!(value["comparison"].is_null());
}

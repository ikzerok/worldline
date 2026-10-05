use serde_json::{json, Value};
use std::io::Cursor;
use worldline_runtime::{
    generate_playthrough_report, PlaythroughReportOptions, ReplayCancellation,
};
#[path = "support/route_comparison_fixture.rs"]
mod fixture;
use fixture::{Fixture, SOURCE};

fn invoke(fixture: &Fixture, trace: &Value, extra: &[&str], json_mode: bool) -> (i32, String) {
    let mut args = vec![
        "playthrough-report".into(),
        fixture.root.display().to_string(),
        "--trace-json".into(),
        trace.to_string(),
    ];
    args.extend(extra.iter().map(|value| value.to_string()));
    if json_mode {
        args.push("--json".into());
    }
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut Cursor::new("")).unwrap();
    assert!(output.len() < 1024 * 1024 + 4096);
    (code, String::from_utf8(output).unwrap())
}
fn query(fixture: &Fixture, trace: &Value, extra: &[&str]) -> (i32, Value) {
    let (code, output) = invoke(fixture, trace, extra, true);
    (code, serde_json::from_str(&output).unwrap())
}

#[test]
fn cli_forwards_runtime_observations_and_outputs_markdown_without_modification() {
    let fixture = Fixture::new();
    let (trace, _) = fixture.traces();
    let expected = generate_playthrough_report(
        &fixture.compile(),
        &trace,
        PlaythroughReportOptions::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    let trace = json!(trace);
    let (code, value) = query(&fixture, &trace, &[]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["ok"], true);
    let report = &value["report"];
    assert_eq!(report["status"], "replayed");
    assert_eq!(report["complete"], true);
    assert_eq!(report["observations"], json!(expected.observations));
    assert_eq!(report["source_snapshot"], expected.source_snapshot);
    assert_eq!(report["source_fingerprint"], expected.source_fingerprint);
    assert_eq!(report["origin"], json!(expected.origin));
    assert!(report["markdown"]
        .as_str()
        .unwrap()
        .contains("工坊窗外传来风声"));
    assert!(!report
        .to_string()
        .contains(&fixture.root.display().to_string()));
    let (code, markdown) = invoke(&fixture, &trace, &[], false);
    assert_eq!(code, 0);
    assert!(markdown.contains("工坊窗外传来风声"));
    assert!(markdown.starts_with('#'));
    assert_eq!(fixture.source(), SOURCE);
}

#[test]
fn verified_partial_checkpoint_and_zero_budget_are_not_claimed_as_complete() {
    let fixture = Fixture::new();
    let compiled = fixture.compile();
    let partial = json!(fixture::record(&compiled, 0, false));
    let (code, value) = query(&fixture, &partial, &[]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["report"]["complete"], false);
    let checkpoint = json!(fixture::checkpoint_trace(&compiled));
    let (_, value) = query(&fixture, &checkpoint, &[]);
    assert_eq!(value["report"]["origin"]["kind"], "checkpoint");
    let (code, value) = query(&fixture, &partial, &["--max-steps=0"]);
    assert_eq!(code, 1, "{value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["report"]["status"], "step_budget_exceeded");
    assert_eq!(value["report"]["executed_steps"], 0);
}

#[test]
fn parameters_trace_failures_and_compile_diagnostics_keep_exit_domains() {
    let fixture = Fixture::new();
    let (trace, _) = fixture.traces();
    let trace = json!(trace);
    for args in [
        vec!["--unknown"],
        vec!["--max-steps=-1"],
        vec!["--max-steps=100001"],
        vec!["--time-budget-ms=30001"],
        vec!["--max-steps=1", "--max-steps=2"],
        vec!["--json=true"],
        vec!["--out=/tmp/unrequested.md"],
    ] {
        let (code, value) = query(&fixture, &trace, &args);
        assert_eq!(code, 2, "{value}");
        assert_eq!(value["ok"], false);
    }
    for invalid in [
        json!({}),
        {
            let mut value = trace.clone();
            value["runtime_version"] = json!("0.19.0");
            value
        },
        {
            let mut value = trace.clone();
            value["ignored_extension"] = json!("x".repeat(4 * 1024 * 1024));
            value
        },
    ] {
        assert_eq!(query(&fixture, &invalid, &[]).0, 2);
    }
    fixture.replace("event start\n  -> missing\n");
    let (code, value) = query(&fixture, &trace, &[]);
    assert_eq!(code, 1);
    assert!(value["report"].is_null());
    assert!(!value["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn forged_observation_is_not_exported_and_large_diagnostics_are_bounded() {
    let fixture = Fixture::new();
    let (trace, _) = fixture.traces();
    let mut forged = json!(trace);
    forged["initial_observation"]["outputs"][0]["text"] = json!("FORGED_AUTHOR_TEXT");
    let (code, value) = query(&fixture, &forged, &[]);
    assert_eq!(code, 1, "{value}");
    assert_eq!(value["report"]["status"], "diverged");
    assert!(!value["report"].to_string().contains("FORGED_AUTHOR_TEXT"));
    fixture.replace(&format!(
        "event start\n{}  -> END\n",
        "  set missing = 1\n".repeat(6000)
    ));
    let (code, value) = query(&fixture, &json!(trace), &[]);
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "output_limit");
    assert!(value.to_string().len() < 256);
}

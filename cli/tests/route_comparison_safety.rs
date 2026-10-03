use serde_json::{json, Value};
use std::io::Cursor;
#[path = "support/route_comparison_fixture.rs"]
mod fixture;
use fixture::{Fixture, SOURCE};

fn run(fixture: &Fixture, left: String, right: String) -> (i32, Value) {
    let args = vec![
        "route-compare".into(),
        fixture.root.display().to_string(),
        "--left-trace-json".into(),
        left,
        "--right-trace-json".into(),
        right,
        "--json".into(),
    ];
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut Cursor::new("")).unwrap();
    assert!(out.len() <= 1024 * 1024 + 4096);
    (code, serde_json::from_slice(&out).unwrap())
}

#[test]
fn pending_transaction_is_rejected_without_recovery_or_byte_changes() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let directory = fixture.root.join(".world/.transactions/interrupted");
    std::fs::create_dir_all(&directory).unwrap();
    let journal = directory.join("journal.json");
    let bytes = b"author-owned interrupted transaction bytes";
    std::fs::write(&journal, bytes).unwrap();
    let (code, response) = run(&fixture, json!(left).to_string(), json!(right).to_string());
    assert_eq!(code, 2, "{response}");
    assert_eq!(response["error"]["code"], "IO_ERROR");
    assert!(response["comparison"].is_null());
    assert_eq!(std::fs::read(journal).unwrap(), bytes);
    assert_eq!(fixture.source(), SOURCE);
}

#[test]
fn workspace_escape_include_is_a_compile_error_with_no_external_fallback() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    fixture.replace(&format!("include \"../unapproved.wl\"\n{SOURCE}"));
    let before = fixture.source();
    let (code, response) = run(&fixture, json!(left).to_string(), json!(right).to_string());
    assert_eq!(code, 1, "{response}");
    assert!(response["comparison"].is_null());
    assert!(response["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "A109"));
    assert_eq!(fixture.source(), before);
}

#[test]
fn old_checkpoint_oversized_step_count_and_duplicate_json_keys_are_rejected() {
    let fixture = Fixture::new();
    let compiled = fixture.compile();
    let (left, right) = fixture.traces();
    let right = json!(right).to_string();
    let mut checkpoint = json!(fixture::checkpoint_trace(&compiled));
    checkpoint["origin"]["checkpoint"]["runtime_version"] = json!("0.19.0");
    let (code, response) = run(&fixture, checkpoint.to_string(), right.clone());
    assert_eq!(code, 2, "{response}");
    assert_eq!(response["error"]["code"], "invalid_trace");
    let mut excessive = json!(left);
    let mut step = excessive["steps"][0].clone();
    step["observation"] = Value::Null;
    excessive["steps"] = json!(vec![step; 4097]);
    excessive["complete"] = json!(false);
    let (code, response) = run(&fixture, excessive.to_string(), right.clone());
    assert_eq!(code, 2, "{response}");
    assert_eq!(response["error"]["code"], "input_limit");
    let (code, response) = run(
        &fixture,
        "{\"schema_version\":1,\"schema_version\":2}".into(),
        right,
    );
    assert_eq!(code, 2, "{response}");
}

#[test]
fn missing_workspace_is_an_io_failure_and_unknown_trace_extensions_remain_compatible() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let mut left = json!(left);
    left["future_optional_field"] = json!({"description":"保留现有trace宽容读取"});
    assert_eq!(
        run(&fixture, left.to_string(), json!(right).to_string()).0,
        0
    );
    std::fs::remove_file(fixture.root.join("world.wl")).unwrap();
    let (code, response) = run(&fixture, left.to_string(), json!(right).to_string());
    assert_eq!(code, 2, "{response}");
    assert_eq!(response["error"]["code"], "IO_ERROR");
}

#[test]
fn human_output_exposes_incomparable_seed_and_actual_result_differences() {
    let fixture = Fixture::new();
    let compiled = fixture.compile();
    let left = fixture::record(&compiled, 0, true);
    let mut story =
        worldline_runtime::Story::new_with_seed(&compiled.program, &compiled.analysis, 99).unwrap();
    story.continue_story().unwrap();
    story.choose(1).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let args = vec![
        "route-compare".into(),
        fixture.root.display().to_string(),
        "--left-trace-json".into(),
        json!(left).to_string(),
        "--right-trace-json".into(),
        json!(story.replay_trace()).to_string(),
    ];
    let mut out = Vec::new();
    assert_eq!(wl::run(&args, &mut out, &mut Cursor::new("")).unwrap(), 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("可对齐：false"), "{text}");
    assert!(text.contains("随机种子不同"));
    assert!(text.contains("bell_fate"));
    assert!(text.contains("credits"));
    assert!(text.contains("seed：99"));
}

#[test]
fn oversized_compile_and_workspace_diagnostics_return_small_budget_failures() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let source = format!(
        "event start\n{}  -> END\n",
        "  set missing = 1\n".repeat(6000)
    );
    fixture.replace(&source);
    let (code, response) = run(&fixture, json!(left).to_string(), json!(right).to_string());
    assert_eq!(code, 1, "{response}");
    assert_eq!(response["error"]["code"], "output_limit");
    assert!(response.to_string().len() < 256);
    assert_eq!(fixture.source(), source);
    fixture.replace(SOURCE);
    let manifest = json!({"schema_version":1,"language_version":"1.10","required_features":["unsupported".repeat(110_000)]}).to_string();
    std::fs::write(fixture.root.join(".world/project.json"), &manifest).unwrap();
    let (code, response) = run(&fixture, json!(left).to_string(), json!(right).to_string());
    assert_eq!(code, 1, "{response}");
    assert_eq!(response["error"]["code"], "output_limit");
    assert!(response.to_string().len() < 256);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(".world/project.json")).unwrap(),
        manifest
    );
}

#[test]
fn oversized_unknown_cli_flag_does_not_echo_the_supplied_name() {
    let fixture = Fixture::new();
    let (left, right) = fixture.traces();
    let args = vec![
        "route-compare".into(),
        fixture.root.display().to_string(),
        "--left-trace-json".into(),
        json!(left).to_string(),
        "--right-trace-json".into(),
        json!(right).to_string(),
        format!("--{}", "x".repeat(2 * 1024 * 1024)),
        "--json".into(),
    ];
    let mut out = Vec::new();
    assert_eq!(wl::run(&args, &mut out, &mut Cursor::new("")).unwrap(), 2);
    assert!(out.len() < 256);
    let value: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["error"]["message"], "含未知route-compare参数");
}

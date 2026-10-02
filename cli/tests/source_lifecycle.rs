use serde_json::{json, Value};
use std::io::Cursor;
use std::process::Command;
#[path = "support/source_lifecycle_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn args(fixture: &Fixture, operation: &str, request: &Value, digest: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "source-lifecycle".into(),
        operation.into(),
        fixture.root.display().to_string(),
        "--request-json".into(),
        request.to_string(),
        "--json".into(),
    ];
    if let Some(digest) = digest {
        args.extend(["--plan-digest".into(), digest.into()]);
    }
    args
}

fn run(args: &[String]) -> Result<(i32, Value), String> {
    let mut output = Vec::new();
    let code = wl::run(args, &mut output, &mut Cursor::new(""))?;
    Ok((code, serde_json::from_slice(&output).unwrap()))
}

#[test]
fn cli_source_lifecycle_create_matches_core_and_saves_explicit_membership() {
    for explicit in [false, true] {
        let fixture = Fixture::new("cli-create", explicit);
        let request = json!({"operation":"create","path":"章节/新 章.wl"});
        let original = fixture.bytes();
        let plan = fixture.plan(&request);
        let (code, preview) = run(&args(&fixture, "preview", &request, None)).unwrap();
        assert_eq!(code, 0, "{preview}");
        assert_eq!(preview["plan"], serde_json::to_value(&plan).unwrap());
        assert_eq!(preview["applied"], false);
        assert_eq!(preview["saved"], false);
        assert_eq!(fixture.bytes(), original);
        let (code, applied) =
            run(&args(&fixture, "apply", &request, Some(&plan.plan_digest))).unwrap();
        assert_eq!(code, 0, "{applied}");
        assert_eq!(applied["plan"], preview["plan"]);
        assert_eq!(applied["applied"], true);
        assert_eq!(applied["saved"], true);
        assert_eq!(applied["baseline"], fixture.project().content_baseline());
        assert!(fixture.root.join("章节/新 章.wl").is_file());
        assert!(std::fs::read_to_string(fixture.root.join("world.wl"))
            .unwrap()
            .contains("章节/新 章.wl"));
        assert!(!fixture.project().compile().has_errors());
        if explicit {
            let manifest: Value = serde_json::from_slice(
                &std::fs::read(fixture.root.join(".world/project.json")).unwrap(),
            )
            .unwrap();
            assert!(manifest["source_config"]["active"]
                .as_array()
                .unwrap()
                .contains(&json!("章节/新 章.wl")));
            assert_eq!(
                manifest["source_config"]["archived"],
                json!(["archived.wl"])
            );
            assert_eq!(manifest["extension"], json!({"keep":true}));
        }
    }
}

#[test]
fn cli_source_lifecycle_move_roundtrips_and_rejects_stale_or_tampered_digest() {
    let fixture = Fixture::new("cli-move", true);
    let request = json!({"operation":"move","from":"old.wl","to":"章节/中文 空间/新章.wl"});
    let plan = fixture.plan(&request);
    let original = fixture.bytes();
    let (_, failure) = run(&args(&fixture, "apply", &request, Some("wrong"))).unwrap();
    assert_eq!(failure["ok"], false);
    assert_eq!(failure["error"]["code"], "SOURCE_LIFECYCLE_REJECTED");
    assert_eq!(failure["error"]["stage"], "apply");
    assert_eq!(fixture.bytes(), original);
    let apply = args(&fixture, "apply", &request, Some(&plan.plan_digest));
    let (code, applied) = run(&apply).unwrap();
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["plan"], serde_json::to_value(&plan).unwrap());
    assert!(!fixture.root.join("old.wl").exists());
    assert_eq!(
        std::fs::read(fixture.root.join("章节/中文 空间/新章.wl")).unwrap(),
        original[std::path::Path::new("old.wl")]
    );
    assert!(!fixture.project().compile().has_errors());
    let saved = fixture.bytes();
    assert_eq!(run(&apply).unwrap().0, 1);
    assert_eq!(fixture.bytes(), saved);
}

#[test]
fn cli_source_lifecycle_include_uses_core_plan_without_rewriting_existing_source() {
    let fixture = Fixture::new("cli-include", false);
    let request = json!({"operation":"include","path":"archived.wl"});
    let original = std::fs::read(fixture.root.join("archived.wl")).unwrap();
    let plan = fixture.plan(&request);
    let (code, preview) = run(&args(&fixture, "preview", &request, None)).unwrap();
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["plan"], serde_json::to_value(&plan).unwrap());
    let (code, applied) = run(&args(&fixture, "apply", &request, Some(&plan.plan_digest))).unwrap();
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["plan"], preview["plan"]);
    assert_eq!(
        std::fs::read(fixture.root.join("archived.wl")).unwrap(),
        original
    );
    assert!(std::fs::read_to_string(fixture.root.join("world.wl"))
        .unwrap()
        .contains("archived.wl"));
}

#[test]
fn cli_source_lifecycle_business_rejections_preserve_exact_bytes() {
    let fixture = Fixture::new("cli-rejections", true);
    let original = fixture.bytes();
    for request in [
        json!({"operation":"include","path":"archived.wl"}),
        json!({"operation":"create","path":"old.wl"}),
        json!({"operation":"create","path":"../escape.wl"}),
        json!({"operation":"move","from":"world.wl","to":"renamed.wl"}),
        json!({"operation":"move","from":"old.wl","to":"archived.wl"}),
    ] {
        let (code, failure) = run(&args(&fixture, "preview", &request, None)).unwrap();
        assert_eq!(code, 1, "{failure}");
        assert_eq!(failure["error"]["code"], "SOURCE_LIFECYCLE_REJECTED");
        assert_eq!(failure["applied"], false);
        assert_eq!(failure["saved"], false);
        assert_eq!(fixture.bytes(), original);
    }
}

#[test]
fn cli_source_lifecycle_rejects_invalid_protocol_shape_before_writes() {
    let fixture = Fixture::new("cli-shape", false);
    let request = json!({"operation":"create","path":"new.wl"});
    let original = fixture.bytes();
    for extra in [
        vec!["--json"],
        vec!["--plan-digest", "x"],
        vec!["--request-json", "{}"],
        vec!["--unknown"],
        vec!["another-workspace"],
    ] {
        let mut arguments = args(&fixture, "preview", &request, None);
        arguments.extend(extra.into_iter().map(str::to_owned));
        assert!(run(&arguments).is_err());
    }
    for invalid in [
        r#"{"operation":"create","path":"new.wl","unknown":true}"#,
        r#"{"operation":"create","path":"a.wl","path":"b.wl"}"#,
        r#"{"operation":"move","from":"old.wl"}"#,
        r#"{"operation":"create","path":23}"#,
        r#"{"operation":"delete","path":"old.wl"}"#,
        "[]",
    ] {
        let mut arguments = args(&fixture, "preview", &request, None);
        arguments[4] = invalid.into();
        assert!(run(&arguments).is_err(), "{invalid}");
    }
    assert!(run(&args(&fixture, "apply", &request, None)).is_err());
    assert!(run(&args(&fixture, "apply", &request, Some(" "))).is_err());
    assert_eq!(fixture.bytes(), original);
}

#[test]
fn cli_source_lifecycle_external_change_and_read_only_workspace_are_rejected() {
    let fixture = Fixture::new("cli-external", true);
    let request = json!({"operation":"create","path":"new.wl"});
    let plan = fixture.plan(&request);
    std::fs::write(fixture.root.join("old.wl"), "character changed\n").unwrap();
    let changed = fixture.bytes();
    let (code, failure) = run(&args(&fixture, "apply", &request, Some(&plan.plan_digest))).unwrap();
    assert_eq!(code, 1, "{failure}");
    assert_eq!(fixture.bytes(), changed);
    let path = fixture.root.join(".world/project.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["required_features"] = json!(["workspace.source_sets.v1", "future.unknown.v99"]);
    std::fs::write(path, manifest.to_string()).unwrap();
    let readonly = fixture.bytes();
    let (code, failure) = run(&args(&fixture, "preview", &request, None)).unwrap();
    assert_eq!(code, 1, "{failure}");
    assert_eq!(fixture.bytes(), readonly);
}

#[test]
fn cli_source_lifecycle_save_failure_retains_journal_and_reports_partial_disk_state() {
    let fixture = Fixture::new("cli-save-failure", false);
    let original = fixture.bytes();
    let request = json!({"operation":"create","path":"章节/新章.wl"});
    let plan = fixture.plan(&request);
    let mut candidate = fixture.project();
    candidate
        .apply_source_lifecycle(&fixture.request(&request), &plan.plan_digest)
        .unwrap();
    let expected_baseline = candidate.content_baseline();
    let output = Command::new(env!("CARGO_BIN_EXE_wl"))
        .args(args(&fixture, "apply", &request, Some(&plan.plan_digest)))
        .env("WORLDLINE_SAVE_FAIL_PHASE", "middle")
        .env_remove("WORLDLINE_SAVE_FAIL_THREAD")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failure["ok"], false, "{failure}");
    assert_eq!(failure["applied"], true);
    assert_eq!(failure["saved"], false);
    assert_eq!(failure["error"]["stage"], "save");
    assert_eq!(failure["baseline"], expected_baseline);
    assert_eq!(failure["plan"], serde_json::to_value(plan).unwrap());
    assert_eq!(fixture.journal_count(), 1);
    assert_ne!(fixture.bytes(), original);
    assert!(!fixture.root.join("章节/新章.wl").exists());
    assert_eq!(fixture.project().content_baseline(), expected_baseline);
    assert!(fixture.root.join("章节/新章.wl").is_file());
    assert_eq!(fixture.journal_count(), 0);
}

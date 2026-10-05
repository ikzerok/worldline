//! CLI 使用正式 move_entity DTO、真实进程预览与保存；坏请求不接管文件。
#[path = "../../core/tests/support/entity_source_move_fixture.rs"]
mod fixture;
use fixture::*;
use serde_json::{json, Value};
use std::io::Cursor;
use std::process::Command;
use worldline_runtime::Story;

fn request(to: &str) -> Value {
    json!({"operation":"move_entity","id":ID,"to":to})
}

fn arguments(
    fixture: &Fixture,
    operation: &str,
    request: &Value,
    digest: Option<&str>,
) -> Vec<String> {
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

fn process(args: &[String]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_wl"))
        .args(args)
        .output()
        .unwrap();
    let response = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "命令未返回 JSON：{error}，stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap(), response)
}

#[test]
fn real_cli_preview_apply_save_reopen_preserves_ordinary_story_save() {
    let fixture = Fixture::full();
    let original = fixture.bytes();
    let plan = fixture.plan();
    let before = fixture.project().compile();
    let save = Story::new_with_seed(&before.program, &before.analysis, 37)
        .unwrap()
        .save()
        .unwrap();
    let args = arguments(&fixture, "preview", &request(TARGET), None);
    let (code, preview) = process(&args);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["ok"], true);
    assert_eq!(preview["plan"], serde_json::to_value(&plan).unwrap());
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["saved"], false);
    assert_eq!(fixture.bytes(), original);
    let args = arguments(&fixture, "apply", &request(TARGET), Some(&plan.plan_digest));
    let (code, applied) = process(&args);
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["plan"], preview["plan"]);
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["saved"], true);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(SOURCE)).unwrap(),
        format!("{PREFIX}{SUFFIX}")
    );
    assert!(std::fs::read_to_string(fixture.root.join(TARGET))
        .unwrap()
        .ends_with(DECLARATION));
    let after = fixture.project().compile();
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    assert_eq!(after.analysis.fingerprint, before.analysis.fingerprint);
    assert_eq!(after.analysis.catalog.states["lamp"].target.id, ID);
    assert!(Story::load(&after.program, &after.analysis, &save).is_ok());
    let mut changed_save: Value = serde_json::from_str(&save).unwrap();
    assert_eq!(
        changed_save["fingerprint"].as_u64(),
        Some(before.analysis.fingerprint)
    );
    changed_save["fingerprint"] = json!(before.analysis.fingerprint ^ 1);
    assert!(
        Story::load(&after.program, &after.analysis, &changed_save.to_string()).is_err(),
        "移源不放宽普通 Story Save 的真实 fingerprint 守卫"
    );
    let mut changed_features: Value = serde_json::from_str(&save).unwrap();
    changed_features["required_features"] = json!(["future.save.v99"]);
    assert!(
        Story::load(
            &after.program,
            &after.analysis,
            &changed_features.to_string()
        )
        .is_err(),
        "移源不接受普通 Story Save 的未知必需能力"
    );
    let saved = fixture.bytes();
    assert_eq!(process(&args).0, 1, "已应用摘要不得再次提交");
    assert_eq!(fixture.bytes(), saved);
}

#[test]
fn cli_missing_or_wrong_kind_inactive_paths_and_stale_digest_are_business_errors() {
    let fixture = Fixture::simple();
    let original = fixture.bytes();
    for edit in [
        request("archive.wl"),
        request("inactive.wl"),
        request("missing.wl"),
        json!({"operation":"move_entity","id":"start","to":TARGET}),
        json!({"operation":"move_entity","id":"missing","to":TARGET}),
    ] {
        let (code, result) = run(&arguments(&fixture, "preview", &edit, None)).unwrap();
        assert_eq!(code, 1, "{result}");
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "SOURCE_LIFECYCLE_REJECTED");
        assert_eq!(result["applied"], false);
        assert_eq!(result["saved"], false);
        assert_eq!(fixture.bytes(), original);
    }
    let plan = fixture.plan();
    fixture.write(TARGET, "// 目标被外部更新\n");
    let changed = fixture.bytes();
    let (code, failure) = run(&arguments(
        &fixture,
        "apply",
        &request(TARGET),
        Some(&plan.plan_digest),
    ))
    .unwrap();
    assert_eq!(code, 1, "{failure}");
    assert_eq!(failure["error"]["code"], "SOURCE_LIFECYCLE_REJECTED");
    assert_eq!(fixture.bytes(), changed);
}

#[test]
fn cli_bad_move_entity_shapes_are_rejected_without_writing() {
    let fixture = Fixture::simple();
    let original = fixture.bytes();
    for invalid in [
        r#"{"operation":"move_entity","to":"target.wl"}"#,
        r#"{"operation":"move_entity","id":23,"to":"target.wl"}"#,
        r#"{"operation":"move_entity","id":"north_lighthouse"}"#,
        r#"{"operation":"move_entity","id":"north_lighthouse","to":false}"#,
        r#"{"operation":"move_entity","id":"north_lighthouse","to":"target.wl","from":"old.wl"}"#,
        r#"{"operation":"move_entity","id":"a","id":"b","to":"target.wl"}"#,
        r#"{"operation":"move_entity","id":"a","to":"a.wl","to":"b.wl"}"#,
    ] {
        let mut args = arguments(&fixture, "preview", &request(TARGET), None);
        args[4] = invalid.into();
        assert!(run(&args).is_err(), "坏请求被接受：{invalid}");
        assert_eq!(fixture.bytes(), original);
    }
    assert!(run(&arguments(&fixture, "apply", &request(TARGET), None)).is_err());
    assert_eq!(fixture.bytes(), original);
}

#[test]
fn real_cli_preview_digest_is_stable_across_one_hundred_independent_processes() {
    let fixture = Fixture::full();
    let expected = serde_json::to_value(fixture.plan()).unwrap();
    let original = fixture.bytes();
    let args = arguments(&fixture, "preview", &request(TARGET), None);
    for _ in 0..100 {
        let (code, preview) = process(&args);
        assert_eq!(code, 0, "{preview}");
        assert_eq!(preview["plan"], expected);
    }
    assert_eq!(fixture.bytes(), original);
}

#[test]
fn cli_same_source_apply_reports_no_change_without_saving() {
    let fixture = Fixture::simple();
    let original = fixture.bytes();
    let (_, preview) = process(&arguments(&fixture, "preview", &request(SOURCE), None));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["plan"]["changes"], json!([]));
    let digest = preview["plan"]["plan_digest"].as_str().unwrap();
    let (code, applied) = process(&arguments(
        &fixture,
        "apply",
        &request(SOURCE),
        Some(digest),
    ));
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["applied"], false);
    assert_eq!(applied["saved"], false);
    assert_eq!(applied["plan"], preview["plan"]);
    assert_eq!(fixture.bytes(), original);
}

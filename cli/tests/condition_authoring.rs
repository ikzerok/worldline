use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{
    condition_authoring::{parse_condition_authoring_request, parse_condition_selector},
    project::Project,
};

const SOURCE: &str = "let courage = 0\r\nlet ready = true\r\nevent start\r\n  choice once \"出发🙂\" if ((ready)) // 原注释\r\n    -> END\r\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".scratch")
            .join(format!(
                "condition-cli-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("world.wl"), SOURCE).unwrap();
        fs::write(root.join("notes.md"), b"untouched\0\xff").unwrap();
        Self(root)
    }
    fn run(&self, operation: &str, args: Vec<String>) -> (i32, Value) {
        run(&self.arguments(operation, args))
    }
    fn arguments(&self, operation: &str, args: Vec<String>) -> Vec<String> {
        let mut all = vec![
            "condition-authoring".into(),
            operation.into(),
            self.0.to_string_lossy().into_owned(),
            "--json".into(),
        ];
        all.extend(args);
        all
    }
    fn project(&self) -> Project {
        Project::open_read_only(&self.0).unwrap()
    }
    fn request(&self) -> Value {
        let projection = self
            .project()
            .condition_authoring(&parse_condition_selector(&selector().to_string()).unwrap())
            .unwrap();
        json!({"schema_version":1,"expected_baseline":projection.baseline,
            "expected_generation":projection.generation,"selector":selector(),
            "replacement":{"kind":"compare","variable":"courage","op":"ge",
                "value":{"kind":"num","value":1},"literal_left":false}})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn selector() -> Value {
    json!({"schema_version":1,"owner":{"target":{"kind":"event","id":"start"},
        "choice_line":4},"slot":"choice_display"})
}
fn run(args: &[String]) -> (i32, Value) {
    let mut output = Vec::new();
    let code = wl::run(args, &mut output, &mut std::io::Cursor::new("")).unwrap();
    (code, serde_json::from_slice(&output).unwrap())
}
fn request_args(request: &Value) -> Vec<String> {
    vec!["--request-json".into(), request.to_string()]
}
fn apply_args(request: &Value, digest: &str, save: bool) -> Vec<String> {
    let mut args = request_args(request);
    args.extend(["--plan-digest".into(), digest.into()]);
    if save {
        args.push("--save".into());
    }
    args
}

#[test]
fn query_preview_apply_and_explicit_save_are_core_parity_without_language_upgrade() {
    let fixture = Fixture::new();
    let project = fixture.project();
    let core_projection = project
        .condition_authoring(&parse_condition_selector(&selector().to_string()).unwrap())
        .unwrap();
    let (code, query) = fixture.run(
        "query",
        vec!["--selector-json".into(), selector().to_string()],
    );
    assert_eq!(code, 0, "{query}");
    assert_eq!(query["projection"], json!(core_projection));
    assert_eq!(query["applied"], false);
    assert_eq!(query["saved"], false);
    let request = fixture.request();
    let core_request = parse_condition_authoring_request(&request.to_string()).unwrap();
    let plan = project.preview_condition_authoring(&core_request).unwrap();
    let (code, preview) = fixture.run("preview", request_args(&request));
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["plan"], json!(plan));
    assert_eq!(preview["applied"], false);
    assert_eq!(preview["saved"], false);
    assert_eq!(
        fs::read(fixture.0.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
    let (code, ephemeral) = fixture.run("apply", apply_args(&request, &plan.plan_digest, false));
    assert_eq!(code, 0, "{ephemeral}");
    assert_eq!(ephemeral["applied"], true);
    assert_eq!(ephemeral["saved"], false);
    assert!(ephemeral["notice"].as_str().unwrap().contains("进程退出"));
    assert_eq!(
        fs::read(fixture.0.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
    let (code, saved) = fixture.run("apply", apply_args(&request, &plan.plan_digest, true));
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["applied"], true);
    assert_eq!(saved["saved"], true);
    let patch = plan.patch.unwrap();
    let mut expected = SOURCE.to_owned();
    expected.replace_range(patch.byte_range, &patch.after);
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        expected
    );
    assert_eq!(
        fs::read(fixture.0.join("notes.md")).unwrap(),
        b"untouched\0\xff"
    );
    assert!(!fixture.0.join(".world/project.json").exists());
    assert_eq!(
        fixture
            .project()
            .compile_read_only()
            .unwrap()
            .options
            .language_version
            .as_str(),
        "1.9"
    );
    let (code, stale) = fixture.run("apply", apply_args(&request, &plan.plan_digest, true));
    assert_eq!(code, 1, "{stale}");
    assert_eq!(stale["applied"], false);
    assert_eq!(stale["error"]["code"], "CONDITION_AUTHORING_REJECTED");
}

#[test]
fn unchanged_condition_retains_redundant_parentheses_and_raw_crlf_bytes() {
    let fixture = Fixture::new();
    let mut request = fixture.request();
    request["replacement"] = json!({"kind":"global_bool","variable":"ready"});
    let (_, preview) = fixture.run("preview", request_args(&request));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(preview["plan"]["changed"], false);
    assert_eq!(preview["plan"]["patch"], Value::Null);
    assert_eq!(preview["plan"]["runtime_changed"], false);
    let digest = preview["plan"]["plan_digest"].as_str().unwrap();
    let (code, applied) = fixture.run("apply", apply_args(&request, digest, true));
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["applied"], false);
    assert_eq!(applied["saved"], false);
    assert_eq!(
        fs::read(fixture.0.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
}

#[test]
fn strict_nested_shapes_duplicate_keys_and_cli_options_are_call_errors() {
    let fixture = Fixture::new();
    let original = fixture.request();
    let mut invalid = Vec::new();
    for pointer in [
        "/unknown",
        "/selector/extra",
        "/selector/owner/extra",
        "/selector/owner/target/display",
        "/replacement/extra",
        "/replacement/value/extra",
    ] {
        let mut value = original.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        value.pointer_mut(parent).unwrap()[key] = json!(true);
        invalid.push(value.to_string());
    }
    for node in [
        json!({"kind":"bool","value":"true"}),
        json!({"kind":"bool","value":true,"extra":1}),
        json!({"kind":"not","child":{"kind":"bool","value":true,"extra":1}}),
        json!({"kind":"rule","name":"guessed"}),
    ] {
        let mut value = original.clone();
        value["replacement"] = node;
        invalid.push(value.to_string());
    }
    invalid.push(original.to_string().replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    ));
    for request in invalid {
        let (code, failed) = fixture.run("preview", vec!["--request-json".into(), request]);
        assert_eq!(code, 2, "{failed}");
        assert_eq!(failed["error"]["code"], "INVALID_PARAMS");
        assert_eq!(failed["applied"], false);
    }
    for extra in [
        vec!["--save"],
        vec!["--json"],
        vec!["--plan-digest", "x"],
        vec!["--request-json", "{}"],
        vec!["--selector-json", "{}"],
        vec!["--unknown"],
        vec!["other-project"],
    ] {
        let mut args = request_args(&original);
        args.extend(extra.into_iter().map(str::to_owned));
        assert_eq!(fixture.run("preview", args).0, 2);
    }
    assert_eq!(fixture.run("apply", request_args(&original)).0, 2);
    assert_eq!(
        fs::read(fixture.0.join("world.wl")).unwrap(),
        SOURCE.as_bytes()
    );
}

#[test]
fn missing_target_advanced_condition_and_stale_generation_are_business_rejections() {
    let fixture = Fixture::new();
    let mut missing = selector();
    missing["owner"]["target"]["id"] = json!("missing");
    let (code, failed) = fixture.run("query", vec!["--selector-json".into(), missing.to_string()]);
    assert_eq!(code, 1, "{failed}");
    assert_eq!(failed["error"]["code"], "CONDITION_AUTHORING_REJECTED");
    let mut request = fixture.request();
    request["expected_generation"] = json!(999999);
    assert_eq!(fixture.run("preview", request_args(&request)).0, 1);
    fs::write(
        fixture.0.join("world.wl"),
        SOURCE.replace("((ready))", "rnd(0, 2) == 1"),
    )
    .unwrap();
    let (code, queried) = fixture.run(
        "query",
        vec!["--selector-json".into(), selector().to_string()],
    );
    assert_eq!(code, 0, "{queried}");
    assert_eq!(queried["projection"]["support"], "advanced");
    assert_eq!(queried["projection"]["tree"], Value::Null);
    let advanced = fs::read(fixture.0.join("world.wl")).unwrap();
    let (code, refused) = fixture.run("preview", request_args(&fixture.request()));
    assert_eq!(code, 1, "{refused}");
    assert_eq!(refused["applied"], false);
    assert_eq!(fs::read(fixture.0.join("world.wl")).unwrap(), advanced);
}

#[test]
fn external_change_and_unknown_capability_do_not_write_or_upgrade() {
    let fixture = Fixture::new();
    let request = fixture.request();
    let (_, preview) = fixture.run("preview", request_args(&request));
    let changed = format!("{SOURCE}// 外部稿\r\n");
    fs::write(fixture.0.join("world.wl"), &changed).unwrap();
    let (code, rejected) = fixture.run(
        "apply",
        apply_args(
            &request,
            preview["plan"]["plan_digest"].as_str().unwrap(),
            true,
        ),
    );
    assert_eq!(code, 1, "{rejected}");
    assert_eq!(rejected["applied"], false);
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        changed
    );
    let current = fixture.request();
    fs::create_dir_all(fixture.0.join(".world")).unwrap();
    let manifest = r#"{"schema_version":1,"language_version":"1.9","required_features":["future.condition.v9"]}"#;
    fs::write(fixture.0.join(".world/project.json"), manifest).unwrap();
    let (code, refused) = fixture.run("preview", request_args(&current));
    assert_eq!(code, 1, "{refused}");
    assert_eq!(refused["applied"], false);
    assert_eq!(
        fs::read_to_string(fixture.0.join(".world/project.json")).unwrap(),
        manifest
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        changed
    );
}

#[test]
fn command_discovery_and_help_include_explicit_selector_and_save() {
    for args in [vec![], vec!["unknown-command"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_wl"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("wl condition-authoring query|preview|apply"));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_wl"))
        .args(["condition-authoring", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--selector-json"));
    assert!(help.contains("--request-json"));
    assert!(help.contains("仅 --save 持久化"));
}

#[path = "condition_authoring/compatibility.rs"]
mod compatibility;

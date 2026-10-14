use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{catalog::TargetRef, project::Project};

const SOURCE: &str = "schema city for entity entity_type place\n  field people population number required\n  field public_id public boolean required\n  field note_id note text required\nentity harbor kind place as \"港城🙂\"\n  property unknown = \"保留原样\" // 原注释\nevent start\n  -> END\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wl-schema-instance-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".world")).unwrap();
        fs::write(root.join("world.wl"), SOURCE).unwrap();
        fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.13","required_features":[]}"#,
        )
        .unwrap();
        Self(root)
    }
    fn run(&self, operation: &str, args: Vec<String>) -> (i32, Value) {
        let mut all = vec![
            "schema-authoring".into(),
            operation.into(),
            self.0.to_string_lossy().into_owned(),
            "--json".into(),
        ];
        all.extend(args);
        let mut output = Vec::new();
        let code = wl::run(&all, &mut output, &mut std::io::Cursor::new(Vec::new())).unwrap();
        (code, serde_json::from_slice(&output).unwrap())
    }
    fn request(&self) -> Value {
        json!({"schema_version":1,"expected_baseline":Project::open(&self.0).unwrap().content_baseline(),
            "target":{"kind":"entity","id":"harbor"},"binding":{"op":"bind","schema_id":"city"},
            "fields":[{"op":"set","field_id":"people","key":"population","value":0},
            {"op":"set","field_id":"public_id","key":"public","value":false},
            {"op":"set","field_id":"note_id","key":"note","value":""}]})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn query_preview_apply_and_explicit_save_share_core_and_preserve_source() {
    let fixture = Fixture::new();
    let project = Project::open_read_only(&fixture.0).unwrap();
    let core = project
        .schema_authoring(&TargetRef::new("entity", "harbor"))
        .unwrap();
    let (code, result) = fixture.run(
        "query",
        vec![
            "--target-json".into(),
            json!({"kind":"entity","id":"harbor"}).to_string(),
        ],
    );
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["projection"], json!(core));
    let request = fixture.request();
    let core_request =
        worldline_core::schemas::parse_schema_authoring_request(&request.to_string()).unwrap();
    let core_plan = project.preview_schema_authoring(&core_request).unwrap();
    let (code, preview) = fixture.run(
        "preview",
        vec!["--request-json".into(), request.to_string()],
    );
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["plan"], json!(core_plan));
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        SOURCE
    );
    let args = vec![
        "--request-json".into(),
        request.to_string(),
        "--plan-digest".into(),
        core_plan.plan_digest.clone(),
    ];
    let (code, ephemeral) = fixture.run("apply", args.clone());
    assert_eq!(code, 0, "{ephemeral}");
    assert_eq!(ephemeral["applied"], true);
    assert_eq!(ephemeral["saved"], false);
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        SOURCE
    );
    let mut save = args.clone();
    save.push("--save".into());
    let (code, saved) = fixture.run("apply", save);
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["saved"], true);
    let reopened = Project::open(&fixture.0).unwrap();
    assert_eq!(
        reopened.clone().compile().analysis.fingerprint,
        project.clone().compile().analysis.fingerprint
    );
    let after = reopened
        .schema_authoring(&TargetRef::new("entity", "harbor"))
        .unwrap();
    assert!(after.fields.iter().all(|field| field.present));
    assert!(after.diagnostics.is_empty(), "{:?}", after.diagnostics);
    assert!(fs::read_to_string(fixture.0.join("world.wl"))
        .unwrap()
        .contains("  property unknown = \"保留原样\" // 原注释"));
    let (code, stale) = fixture.run("apply", args);
    assert_eq!(code, 1, "{stale}");
    assert_eq!(stale["applied"], false);
}

#[test]
fn strict_cli_shapes_and_duplicate_json_never_write() {
    let fixture = Fixture::new();
    let original = fixture.request();
    let mut invalid = Vec::new();
    let mut extra = original.clone();
    extra["extra"] = json!(true);
    invalid.push(extra.to_string());
    let mut target = original.clone();
    target["target"]["display"] = json!("名称");
    invalid.push(target.to_string());
    let mut binding = original.clone();
    binding["binding"]["guess"] = json!(true);
    invalid.push(binding.to_string());
    let mut value = original.clone();
    value["fields"][0]["value"] = Value::Null;
    invalid.push(value.to_string());
    let mut reference = original.clone();
    reference["fields"][0]["value"] = json!({"kind":"entity","id":"harbor","name":"oops"});
    invalid.push(reference.to_string());
    invalid.push(original.to_string().replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    ));
    for request in invalid {
        let (code, result) = fixture.run("preview", vec!["--request-json".into(), request]);
        assert_eq!(code, 2, "{result}");
        assert_eq!(result["error"]["code"], "INVALID_PARAMS");
    }
    for args in [
        vec![
            "--request-json".into(),
            original.to_string(),
            "--save".into(),
        ],
        vec![
            "--request-json".into(),
            original.to_string(),
            "--json".into(),
        ],
    ] {
        assert_eq!(fixture.run("preview", args).0, 2);
    }
    assert_eq!(
        fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
        SOURCE
    );
}

#[test]
fn query_missing_target_is_business_failure_and_unknown_target_key_is_protocol_failure() {
    let fixture = Fixture::new();
    let (code, missing) = fixture.run(
        "query",
        vec![
            "--target-json".into(),
            r#"{"kind":"entity","id":"missing"}"#.into(),
        ],
    );
    assert_eq!(code, 1, "{missing}");
    assert_eq!(missing["error"]["code"], "SCHEMA_AUTHORING_REJECTED");
    let (code, malformed) = fixture.run(
        "query",
        vec![
            "--target-json".into(),
            r#"{"kind":"entity","id":"harbor","guess":true}"#.into(),
        ],
    );
    assert_eq!(code, 2, "{malformed}");
}

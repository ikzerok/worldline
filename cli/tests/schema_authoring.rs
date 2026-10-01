use serde_json::{json, Value};
const SOURCE: &str = "schema city for entity entity_type place closed\n  field people population number required\nentity harbor kind place\n  property population = 0\nbind entity harbor to city\nevent start\n  -> END\n";
#[test]
fn schema_cli_uses_core_index_and_reviewed_impact_before_applying_invalid_draft() {
    let root = std::env::temp_dir().join(format!("wl-schema-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
    )
    .unwrap();
    std::fs::write(root.join("world.wl"), SOURCE).unwrap();
    let project = worldline_core::project::Project::open(&root).unwrap();
    let changed = SOURCE.replace("population number required", "population text required");
    let request = json!({"schema_version":1,"path":"world.wl","expected_baseline":project.content_baseline(),"source":changed});
    let run = |command: &str, digest: Option<&str>| {
        let mut args = vec![
            command.into(),
            root.to_string_lossy().into_owned(),
            "--json".into(),
        ];
        if command != "schema-index" {
            args.extend(["--request-json".into(), request.to_string()]);
        }
        if let Some(digest) = digest {
            args.extend(["--plan-digest".into(), digest.into()]);
        }
        let mut output = Vec::new();
        let code = wl::run(&args, &mut output, &mut std::io::Cursor::new(Vec::new())).unwrap();
        (code, serde_json::from_slice::<Value>(&output).unwrap())
    };
    let (code, index) = run("schema-index", None);
    assert_eq!(code, 0);
    assert_eq!(index["index"], json!(project.schema_index()));
    let (code, preview) = run("schema-preview", None);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(
        preview["preview"]["field_changes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        preview["preview"]["instance_impacts"][0]["target"]["id"],
        "harbor"
    );
    assert!(preview["preview"]["after_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "SCH005"));
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        SOURCE
    );
    let (code, rejected) = run("schema-apply", Some("wrong"));
    assert_eq!(code, 1);
    assert_eq!(rejected["error"]["code"], "SCHEMA_EDIT_REJECTED");
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        SOURCE
    );
    let digest = preview["preview"]["plan_digest"].as_str().unwrap();
    assert_eq!(run("schema-apply", Some(digest)).0, 0);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        changed
    );
    assert_eq!(run("schema-apply", Some(digest)).0, 1);
    std::fs::remove_dir_all(root).unwrap();
}

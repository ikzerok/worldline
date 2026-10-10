use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::project::Project;

static NEXT: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str =
    "character actor\nevent start\n  say actor \"原文🙂\" direction \"作者备注\"\n  -> END\n";
fn fixture(version: &str, source: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "v034-cli-dialogue-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(root.join(".world/project.json"), json!({"schema_version":1,"language_version":version,"entry":"world.wl","required_features":[]}).to_string()).unwrap();
    root
}
fn invoke(root: &std::path::Path, operation: &str, rest: &[String]) -> (i32, Value) {
    let mut args = vec![
        "dialogue".into(),
        operation.into(),
        root.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    args.extend_from_slice(rest);
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&output).unwrap())
}
fn query(root: &std::path::Path) -> Value {
    let (code, result) = invoke(
        root,
        "query",
        &[
            "--target-json".into(),
            json!({"kind":"event","id":"start"}).to_string(),
        ],
    );
    assert_eq!(code, 0, "{result}");
    result["projection"].clone()
}
fn update(projection: &Value, text: &str) -> Value {
    let statement = &projection["statements"][0];
    let mut draft = statement["draft"].clone();
    draft["parts"] = json!([{"kind":"literal","text":text}]);
    json!({"schema_version":1,"expected_baseline":projection["baseline"],"target":projection["target"],
        "generation":projection["generation"],"operation":{"kind":"update","statement_id":statement["id"],"draft":draft}})
}

#[test]
fn preview_memory_apply_explicit_save_and_stale_plan_are_distinct() {
    let root = fixture("1.11", SOURCE);
    let projection = query(&root);
    let request = update(&projection, "引号\"与\\反斜杠\n字面{值}🙂");
    let args = vec!["--request-json".into(), request.to_string()];
    let (code, preview) = invoke(&root, "preview", &args);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["plan"]["can_apply"], true, "{preview}");
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    let project = Project::open_read_only(&root).unwrap();
    let buffer = project
        .open_writing_buffer(&worldline_core::TargetRef::new("event", "start"))
        .unwrap();
    let core = project
        .preview_dialogue_edit(&buffer, &serde_json::from_value(request).unwrap())
        .unwrap();
    assert_eq!(preview["plan"], json!(core));
    let mut apply = args.clone();
    apply.extend([
        "--plan-digest".into(),
        preview["plan"]["plan_digest"].as_str().unwrap().into(),
    ]);
    let (code, memory) = invoke(&root, "apply", &apply);
    assert_eq!(code, 0, "{memory}");
    assert_eq!(memory["applied"], true);
    assert_eq!(memory["saved"], false);
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    apply.push("--save".into());
    let (code, saved) = invoke(&root, "apply", &apply);
    assert_eq!(code, 0, "{saved}");
    assert_eq!(saved["saved"], true);
    assert_ne!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    assert!(!Project::open(&root)
        .unwrap()
        .compile_read_only()
        .unwrap()
        .has_errors());
    assert_eq!(invoke(&root, "apply", &apply).0, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_op_and_invalid_shapes_never_write() {
    let root = fixture("1.11", SOURCE);
    let projection = query(&root);
    let request = update(&projection, "原文🙂");
    let mut args = vec!["--request-json".into(), request.to_string()];
    let (_, preview) = invoke(&root, "preview", &args);
    assert_eq!(preview["plan"]["no_change"], true, "{preview}");
    args.extend([
        "--plan-digest".into(),
        preview["plan"]["plan_digest"].as_str().unwrap().into(),
        "--save".into(),
    ]);
    let (code, applied) = invoke(&root, "apply", &args);
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["applied"], false);
    assert_eq!(applied["saved"], false);
    assert_eq!(invoke(&root, "preview", &args).0, 2);
    assert_eq!(
        invoke(
            &root,
            "query",
            &[
                "--target-json".into(),
                "{\"kind\":\"event\",\"id\":\"start\",\"extra\":true}".into()
            ]
        )
        .0,
        2
    );
    assert_eq!(
        invoke(
            &root,
            "preview",
            &[
                "--request-json".into(),
                request
                    .to_string()
                    .replacen('{', "{\"schema_version\":1,", 1)
            ]
        )
        .0,
        2
    );
    assert_eq!(
        invoke(
            &root,
            "query",
            &[
                "--target-json".into(),
                json!({"kind":"event","id":"start"}).to_string(),
                "--json".into()
            ]
        )
        .0,
        2
    );
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn migration_is_previewed_and_saved_with_the_dialogue_in_one_request() {
    let source = "character actor\nevent start\n  原旁白\n  -> END\n";
    let root = fixture("1.9", source);
    let projection = query(&root);
    let request = json!({"schema_version":1,"expected_baseline":projection["baseline"],"target":projection["target"],"generation":0,
        "enable_language_1_11":true,"operation":{"kind":"convert","statement_id":projection["statements"][0]["id"],
        "to":"say","speaker":{"kind":"character","id":"actor"}}});
    let mut args = vec!["--request-json".into(), request.to_string()];
    let (code, preview) = invoke(&root, "preview", &args);
    assert_eq!(code, 0, "{preview}");
    assert!(!preview["plan"]["migration"].is_null());
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), source);
    assert_eq!(
        Project::open(&root)
            .unwrap()
            .compile_read_only()
            .unwrap()
            .options
            .language_version
            .as_str(),
        "1.9"
    );
    args.extend([
        "--plan-digest".into(),
        preview["plan"]["plan_digest"].as_str().unwrap().into(),
        "--save".into(),
    ]);
    let (code, applied) = invoke(&root, "apply", &args);
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["saved"], true);
    assert_eq!(
        Project::open(&root)
            .unwrap()
            .compile_read_only()
            .unwrap()
            .options
            .language_version
            .as_str(),
        "1.11"
    );
    assert!(fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("say actor"));
    fs::remove_dir_all(root).unwrap();
}

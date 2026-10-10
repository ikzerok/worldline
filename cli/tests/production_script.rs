use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{production_script::*, project::Project};
static NEXT: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "character actor\ncharacter other\nfragment shared()\n  say other \"别人的片段\" direction \"SECRET_FRAGMENT\"\n  return\nevent start\n  say actor \"=1+2,中文🙂\" direction \"SECRET_DIRECTION\"\n  call shared()\n  -> END\n";
fn fixture() -> (PathBuf, PathBuf) {
    let parent = std::env::temp_dir().join(format!(
        "v034-cli-production-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let root = parent.join("project");
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), SOURCE).unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.11","entry":"world.wl","required_features":[]}"#).unwrap();
    (parent, root)
}
fn request() -> Value {
    json!({"schema_version":1,"scope":{"kind":"current_target","target":{"kind":"event","id":"start"}},"speaker":{"kind":"character","id":"actor"}})
}
fn run(root: &Path, operation: &str, request: &Value, extra: &[String]) -> (i32, Value) {
    let mut args = vec![
        "production-script".into(),
        operation.into(),
        root.to_string_lossy().into_owned(),
        "--request-json".into(),
        request.to_string(),
        "--json".into(),
    ];
    args.extend_from_slice(extra);
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}
#[test]
fn selected_scope_all_formats_exact_core_bytes_and_explicit_direction() {
    let (parent, root) = fixture();
    let (code, page) = run(&root, "query", &request(), &[]);
    assert_eq!(code, 0, "{page}");
    assert_eq!(page["page"]["total"], 1);
    assert!(!page.to_string().contains("SECRET"));
    assert!(!page.to_string().contains("别人的片段"));
    let project = Project::open_read_only(&root).unwrap();
    let snapshot = project
        .production_script_snapshot(&[], &[], &serde_json::from_value(request()).unwrap())
        .unwrap();
    for (format, extension) in [("json", "json"), ("markdown", "md"), ("csv", "csv")] {
        let options = json!({"schema_version":1,"format":format});
        let destination = parent.join(format!("script.{extension}"));
        let (code, export) = run(
            &root,
            "export",
            &request(),
            &[
                "--options-json".into(),
                options.to_string(),
                "--output".into(),
                destination.to_string_lossy().into_owned(),
            ],
        );
        assert_eq!(code, 0, "{export}");
        assert_eq!(export["delivered"], true);
        let expected = snapshot
            .export(&serde_json::from_value::<ProductionExportOptions>(options).unwrap())
            .unwrap();
        assert_eq!(fs::read(&destination).unwrap(), expected.bytes());
        assert_eq!(
            export["artifact"]["text"].as_str().unwrap().as_bytes(),
            expected.bytes()
        );
        assert_eq!(export["artifact"]["byte_count"], expected.bytes().len());
        assert!(!String::from_utf8_lossy(expected.bytes()).contains("SECRET"));
        let (_, repeated) = run(
            &root,
            "export",
            &request(),
            &[
                "--options-json".into(),
                json!({"schema_version":1,"format":format}).to_string(),
                "--output".into(),
                destination.to_string_lossy().into_owned(),
            ],
        );
        assert_eq!(repeated["ok"], false);
        assert_eq!(fs::read(destination).unwrap(), expected.bytes());
    }
    let (_, private) = run(
        &root,
        "export",
        &request(),
        &[
            "--options-json".into(),
            json!({"schema_version":1,"format":"json","include_direction":true}).to_string(),
        ],
    );
    assert!(private["artifact"]["text"]
        .as_str()
        .unwrap()
        .contains("SECRET_DIRECTION"));
    assert!(!private["artifact"]["text"]
        .as_str()
        .unwrap()
        .contains("SECRET_FRAGMENT"));
    assert_eq!(fs::read_to_string(root.join("world.wl")).unwrap(), SOURCE);
    fs::remove_dir_all(parent).unwrap();
}
#[test]
fn query_budget_protocol_and_workspace_output_fail_without_files() {
    let (parent, root) = fixture();
    let original = request();
    let mut invalid = original.clone();
    invalid["scope"]["target"]["extra"] = json!(1);
    assert_eq!(run(&root, "query", &invalid, &[]).0, 2);
    assert_eq!(
        run(&root, "query", &original, &["--offset".into(), "-1".into()]).0,
        2
    );
    assert_eq!(
        run(&root, "query", &original, &["--limit".into(), "0".into()]).0,
        1
    );
    let mut over = original.clone();
    over["speaker"] = Value::Null;
    over["limits"] = json!({"rows":1});
    let (code, result) = run(&root, "query", &over, &[]);
    assert_eq!(code, 1, "{result}");
    assert_eq!(result["error"]["code"], "BUDGET_EXCEEDED");
    let options = json!({"schema_version":1,"format":"json"});
    let destination = root.join("private.json");
    assert_eq!(
        run(
            &root,
            "export",
            &original,
            &[
                "--options-json".into(),
                options.to_string(),
                "--output".into(),
                destination.to_string_lossy().into_owned()
            ]
        )
        .0,
        1
    );
    assert!(!destination.exists());
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    fs::remove_dir_all(parent).unwrap();
}

use super::common::*;
use serde_json::{json, Value};
#[test]
fn source_edit_cli_requires_reviewed_digest_and_preserves_invalid_draft() {
    let root = temp_workspace(
        "source-edit",
        r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
        "event start\n  原稿\n  -> END\n",
    );
    let p = worldline_core::project::Project::open(&root).unwrap();
    let original = std::fs::read(root.join("world.wl")).unwrap();
    let request = json!({"schema_version":1,"path":"world.wl","expected_baseline":p.content_baseline(),"source":"event start\n  if (\n    未完稿\n"});
    let run = |operation: &str, digest: Option<&str>| {
        let mut args = vec![
            "source-edit".into(),
            operation.into(),
            root.to_string_lossy().into_owned(),
            "--request-json".into(),
            request.to_string(),
            "--json".into(),
        ];
        if let Some(d) = digest {
            args.extend(["--plan-digest".into(), d.into()]);
        }
        let mut out = Vec::new();
        let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
        (code, serde_json::from_slice::<Value>(&out).unwrap())
    };
    let (code, preview) = run("preview", None);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), original);
    assert_eq!(run("apply", Some("wrong")).0, 1);
    let digest = preview["preview"]["plan_digest"].as_str().unwrap();
    assert_eq!(run("apply", Some(digest)).0, 0);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        request["source"].as_str().unwrap()
    );
    assert_eq!(run("apply", Some(digest)).0, 1);
}

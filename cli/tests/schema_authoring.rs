use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{project::Project, source_edit::SourceEditRequest};
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
    let core_request = serde_json::from_value(request.clone()).unwrap();
    let core_preview = project.preview_schema_edit(&core_request).unwrap();
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
    assert_eq!(preview["preview"], json!(core_preview));
    assert_eq!(preview["preview"]["complete"], true);
    assert_eq!(preview["preview"]["incomplete_reasons"], json!([]));
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
    let (code, applied) = run("schema-apply", Some(digest));
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["preview"], json!(core_preview));
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        changed
    );
    assert_eq!(run("schema-apply", Some(digest)).0, 1);
    std::fs::remove_dir_all(root).unwrap();
}

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-schema-cli-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join(".world")).unwrap();
        fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
        )
        .unwrap();
        for (path, source) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        Self(root)
    }

    fn project(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn edit_request(project: &Project, path: &str, source: &str) -> SourceEditRequest {
    SourceEditRequest {
        schema_version: 1,
        path: path.into(),
        expected_baseline: project.content_baseline(),
        source: source.into(),
    }
}

fn schema_command(root: &Path, request: &SourceEditRequest, digest: Option<&str>) -> (i32, Value) {
    let mut args = vec![
        if digest.is_some() {
            "schema-apply".into()
        } else {
            "schema-preview".into()
        },
        root.to_string_lossy().into_owned(),
        "--request-json".into(),
        json!(request).to_string(),
        "--json".into(),
    ];
    if let Some(digest) = digest {
        args.extend(["--plan-digest".into(), digest.into()]);
    }
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut std::io::Cursor::new("")).unwrap();
    (code, serde_json::from_slice(&output).unwrap())
}

#[test]
fn schema_cli_distinguishes_known_zero_instances_from_missing_or_escaping_sources() {
    let source =
        "schema city for entity\n  field people population number\nevent start\n  -> END\n";
    for (prefix, diagnostic) in [
        ("", None),
        ("include \"missing.wl\"\n", Some("A105")),
        ("include \"../outside.wl\"\n", Some("A109")),
    ] {
        let fixture = Fixture::new("zero", &[("world.wl", source)]);
        let project = fixture.project();
        let candidate = format!("{prefix}{}", source.replace("number", "text"));
        let request = edit_request(&project, "world.wl", &candidate);
        let core = project.preview_schema_edit(&request).unwrap();
        let (code, response) = schema_command(&fixture.0, &request, None);
        assert_eq!(code, 0, "{response}");
        let preview = &response["preview"];
        assert_eq!(preview, &json!(core));
        assert_eq!(preview["instance_impacts"], json!([]));
        assert_eq!(preview["field_changes"].as_array().unwrap().len(), 1);
        assert_eq!(preview["complete"], diagnostic.is_none());
        if let Some(diagnostic) = diagnostic {
            assert_eq!(preview["incomplete_reasons"], json!(["source_loading"]));
            assert!(preview["after_diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == diagnostic));
        } else {
            assert_eq!(preview["incomplete_reasons"], json!([]));
        }
        assert_eq!(
            fs::read_to_string(fixture.0.join("world.wl")).unwrap(),
            source
        );
        assert_eq!(response["baseline"], request.expected_baseline);
    }
}

#[test]
fn schema_cli_keeps_known_multi_include_impacts_and_saves_incomplete_draft_without_migration() {
    let schema = "schema city for entity entity_type place closed\n  field people population number required\n";
    let world = "include \"schema.wl\"\ninclude \"places/harbor.wl\"\ninclude \"missing.wl\"\ninclude \"places/cove.wl\"\nevent start\n  -> END\n";
    let harbor =
        "entity harbor kind place\n  property population = 0\nbind entity harbor to city\n";
    let cove = "entity cove kind place\n  property population = 12\nbind entity cove to city\n";
    let files = [
        ("world.wl", world),
        ("schema.wl", schema),
        ("places/harbor.wl", harbor),
        ("places/cove.wl", cove),
    ];
    let fixture = Fixture::new("multi-include", &files);
    let project = fixture.project();
    let candidate = schema.replace("population number", "inhabitants number");
    let request = edit_request(&project, "schema.wl", &candidate);
    let core = project.preview_schema_edit(&request).unwrap();
    let (code, response) = schema_command(&fixture.0, &request, None);
    assert_eq!(code, 0, "{response}");
    let preview = &response["preview"];
    assert_eq!(preview, &json!(core));
    assert_eq!(preview["complete"], false);
    assert_eq!(preview["incomplete_reasons"], json!(["source_loading"]));
    let targets: Vec<_> = preview["instance_impacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|impact| impact["target"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(targets, ["cove", "harbor"]);
    for impact in preview["instance_impacts"].as_array().unwrap() {
        for code in ["SCH004", "SCH008"] {
            assert!(impact["after_diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == code));
        }
    }
    for (path, source) in files {
        assert_eq!(fs::read_to_string(fixture.0.join(path)).unwrap(), source);
    }
    let (code, applied) = schema_command(&fixture.0, &request, Some(&core.plan_digest));
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["preview"], json!(core));
    assert_eq!(
        fs::read_to_string(fixture.0.join("schema.wl")).unwrap(),
        candidate
    );
    for (path, source) in [
        ("world.wl", world),
        ("places/harbor.wl", harbor),
        ("places/cove.wl", cove),
    ] {
        assert_eq!(fs::read_to_string(fixture.0.join(path)).unwrap(), source);
    }
    let mut reopened = fixture.project();
    assert!(
        reopened.compile().has_errors(),
        "保存错误草稿不得解除运行门禁"
    );
    assert_eq!(applied["baseline"], reopened.content_baseline());
}

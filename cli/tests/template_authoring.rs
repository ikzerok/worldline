use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{
    presentation_commands::Revision,
    project::Project,
    project_templates::{protocol::*, ProjectTemplateDraft},
};

struct Work(PathBuf);
impl Work {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "wl-template-cli-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("world.wl"),
            "character author\nevent start\n  -> END\n",
        )
        .unwrap();
        Self(root)
    }
    fn project(&self) -> Project {
        Project::open_read_only(&self.0).unwrap()
    }
    fn args(&self, operation: &str, request: &Value) -> Vec<String> {
        vec![
            "template".into(),
            operation.into(),
            self.0.to_string_lossy().into_owned(),
            "--request-json".into(),
            request.to_string(),
            "--json".into(),
        ]
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn invoke(args: Vec<String>) -> (i32, Value) {
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&output).unwrap())
}
fn request(project: &Project) -> TemplateMutationRequest {
    TemplateMutationRequest {
        schema_version: 1,
        expected_revision: Revision::default(),
        expected_baseline: project.content_baseline(),
        intent: TemplateMutationIntent::Upsert {
            draft: ProjectTemplateDraft {
                source_bytes: serde_json::to_vec(&json!({
                    "schema_version":1,"id":"project:profile","title":"人物笔记",
                    "applies_to":{"kind":"character"},"fields":[],"extension":[0,false,""]
                }))
                .unwrap(),
                ..Default::default()
            },
        },
    }
}

#[test]
fn cli_template_draft_and_preview_equal_core_without_writes() {
    let work = Work::new();
    let project = work.project();
    let draft: TemplateDraftRequest = serde_json::from_value(json!({"schema_version":1,
        "action":{"kind":"open","source":{"kind":"new"}}}))
    .unwrap();
    let expected = project.template_draft_request(&draft).unwrap();
    let (code, value) = invoke(work.args("draft", &json!(draft)));
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["projection"], json!(expected.projection));
    let request = request(&project);
    let expected = project
        .preview_template_request(Revision::default(), &request)
        .unwrap();
    let (code, value) = invoke(work.args("preview", &json!(request)));
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["plan"], json!(expected));
    assert_eq!(value["saved"], false);
    assert!(!work.0.join(".world/project.json").exists());
}

#[test]
fn cli_template_apply_requires_digest_and_only_save_flag_persists() {
    let work = Work::new();
    let project = work.project();
    let request = request(&project);
    let plan = project
        .preview_template_request(Revision::default(), &request)
        .unwrap();
    let mut args = work.args("apply", &json!(request));
    assert_eq!(invoke(args.clone()).0, 2);
    args.extend(["--plan-digest".into(), plan.plan_digest]);
    let (code, value) = invoke(args.clone());
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["applied"], true);
    assert_eq!(value["saved"], false);
    assert!(!work.0.join(".world/project.json").exists());
    args.push("--save".into());
    let (code, value) = invoke(args);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["saved"], true);
    let index = work.project().template_index();
    assert_eq!(
        index.projects["project:profile"]
            .source_document
            .as_ref()
            .unwrap()["extension"],
        json!([0, false, ""])
    );
}

#[test]
fn cli_template_strict_parameters_stale_and_wire_errors_are_separate() {
    let work = Work::new();
    let mut request = request(&work.project());
    request.expected_baseline = "old".into();
    let (code, value) = invoke(work.args("preview", &json!(request)));
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "STALE_BASELINE");
    let mut args = work.args("preview", &json!(request));
    args.push("--save".into());
    assert_eq!(invoke(args).0, 2);
    let mut args = work.args("draft",&json!({"schema_version":1,"action":{"kind":"open","source":{"kind":"new","unknown":true}}}));
    assert_eq!(invoke(args.clone()).0, 2);
    args[4] = "{\"schema_version\":1,\"schema_version\":1}".into();
    assert_eq!(invoke(args).0, 2);
    assert!(!work.0.join(".world/project.json").exists());
}

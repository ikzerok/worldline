use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{draft_rehearsal::DraftRehearsalRequest, project::Project};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-draft-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut project = Project::new(&root);
        project
            .set_text(
                &project.entry.clone(),
                "event start\n  已保存正文\n  -> END\n".into(),
            )
            .unwrap();
        project.save().unwrap();
        Self(root)
    }
    fn request(&self) -> Value {
        let project = Project::open_read_only(&self.0).unwrap();
        let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
        buffer.replace_source("event start\n  未应用草稿ONLY 👋\n  -> END\n".into());
        let input =
            DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], Vec::new(), false)
                .unwrap();
        json!({"input":input,"seed":7})
    }
    fn args(&self, request: &Value) -> Vec<String> {
        vec![
            "draft-rehearsal".into(),
            self.0.to_string_lossy().into_owned(),
            "--request-json".into(),
            request.to_string(),
            "--json".into(),
        ]
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn invoke(args: Vec<String>) -> (i32, Value) {
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}

#[test]
fn cli_draft_rehearsal_executes_current_draft_without_applying_or_saving() {
    let work = Workspace::new();
    let before = fs::read(work.0.join("world.wl")).unwrap();
    let request = work.request();
    let (code, response) = invoke(work.args(&request));
    assert_eq!(code, 0, "{response}");
    assert_eq!(response["ok"], true);
    assert_eq!(response["applied"], false);
    assert_eq!(response["saved"], false);
    assert_eq!(response["result"]["outputs_complete"], true);
    assert!(response["result"]["outputs"]
        .to_string()
        .contains("未应用草稿ONLY"));
    assert!(!response["result"]["outputs"]
        .to_string()
        .contains("已保存正文"));
    assert_eq!(
        response["result"]["scope"]["sources"][0]["kind"],
        "writing_draft"
    );
    assert_eq!(fs::read(work.0.join("world.wl")).unwrap(), before);
    assert_eq!(
        Project::open_read_only(&work.0).unwrap().content_baseline(),
        request["input"]["content_baseline"].as_str().unwrap()
    );
}

#[test]
fn cli_draft_stale_and_compile_failures_are_domain_results() {
    let work = Workspace::new();
    let mut request = work.request();
    request["input"]["content_baseline"] = json!("stale");
    let (code, response) = invoke(work.args(&request));
    assert_eq!(code, 1);
    assert_eq!(response["result"]["ok"], false);
    assert!(response["result"]["error"].is_string());
    let mut invalid = work.request();
    invalid["input"]["drafts"][0]["source"] = json!("event\n");
    assert_eq!(invoke(work.args(&invalid)).0, 1);
    let mut composing = work.request();
    composing["input"]["composing"] = json!(true);
    assert_eq!(invoke(work.args(&composing)).0, 1);
}

#[test]
fn cli_draft_rejects_unknown_duplicate_unsafe_and_save_parameters() {
    let work = Workspace::new();
    let request = work.request();
    let mut args = work.args(&request);
    args.push("--save".into());
    assert_eq!(invoke(args).0, 2);
    let mut duplicate = work.args(&request);
    duplicate.extend(["--request-json".into(), request.to_string()]);
    assert_eq!(invoke(duplicate).0, 2);
    let mut unknown = request.clone();
    unknown["input"]["unknown"] = json!(true);
    assert_eq!(invoke(work.args(&unknown)).0, 2);
    let mut unsafe_seed = request.clone();
    unsafe_seed["seed"] = json!(9_007_199_254_740_992u64);
    assert_eq!(invoke(work.args(&unsafe_seed)).0, 2);
    let mut duplicate_json = work.args(&request);
    duplicate_json[3] = format!("{{\"input\":{},\"seed\":1,\"seed\":2}}", request["input"]);
    assert_eq!(invoke(duplicate_json).0, 2);
}

#[test]
fn cli_draft_file_input_is_explicit_and_extra_choices_are_not_ignored() {
    let work = Workspace::new();
    let request = work.request();
    let path = work.0.join("request.json");
    fs::write(&path, request.to_string()).unwrap();
    let args = vec![
        "draft-rehearsal".into(),
        work.0.to_string_lossy().into_owned(),
        "--request".into(),
        path.to_string_lossy().into_owned(),
        "--json".into(),
    ];
    assert_eq!(invoke(args.clone()).0, 0);
    let mut both = args;
    both.extend(["--request-json".into(), request.to_string()]);
    assert_eq!(invoke(both).0, 2);
    let mut extra = request;
    extra["choice_ids"] = json!(["not-a-real-current-choice"]);
    let (code, response) = invoke(work.args(&extra));
    assert_eq!(code, 1, "{response}");
    assert_eq!(response["result"]["choices_consumed"], 0);
}

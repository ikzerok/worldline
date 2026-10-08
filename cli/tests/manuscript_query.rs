use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    manuscript::{ManuscriptDraft, ManuscriptQueryDraft, ManuscriptQueryRequest},
    project::Project,
};
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-query-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut project = Project::new(&root);
        project.create_authoring_document(&root.join(".world/project.json"), serde_json::to_vec(&json!({"schema_version":1,"required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/manuscripts/book.json"}})).unwrap()).unwrap();
        let entries: Vec<_> = (0..125).map(|n| json!({"id":format!("chapter{n}"),"kind":"chapter","title":"同名章","summary":format!("摘要{n}"),"target_ref":{"kind":"event","id":"start"}})).collect();
        project
            .create_authoring_document(
                &root.join(".world/manuscripts/book.json"),
                serde_json::to_vec(
                    &json!({"schema_version":1,"id":"book","title":"书稿","entries":entries}),
                )
                .unwrap(),
            )
            .unwrap();
        project.save().unwrap();
        Self(root)
    }
    fn args(&self, query: &Value) -> Vec<String> {
        vec![
            "manuscript-query".into(),
            self.0.to_string_lossy().into_owned(),
            "--query-json".into(),
            query.to_string(),
            "--json".into(),
        ]
    }
    fn project(&self) -> Project {
        Project::open_read_only(&self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn invoke(args: Vec<String>) -> (i32, Value) {
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&output).unwrap())
}
fn query() -> ManuscriptQueryRequest {
    ManuscriptQueryRequest {
        manuscript_id: "book".into(),
        ..Default::default()
    }
}

#[test]
fn cli_manuscript_query_pages_match_core_and_unchanged_disk() {
    let work = Workspace::new();
    let project = work.project();
    let before = fs::read(work.0.join(".world/manuscripts/book.json")).unwrap();
    let mut request = query();
    request.text = "摘要124".into();
    let expected = project
        .manuscript_query_snapshot(&[], &[])
        .unwrap()
        .query(&request)
        .unwrap();
    let (code, result) = invoke(work.args(&json!(request)));
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["page"], json!(expected));
    assert_eq!(result["page"]["matching_chapters"], 1);
    assert_eq!(result["saved"], false);
    assert_eq!(
        fs::read(work.0.join(".world/manuscripts/book.json")).unwrap(),
        before
    );
}

#[test]
fn cli_manuscript_query_draft_is_explicit_and_stale_is_business_failure() {
    let work = Workspace::new();
    let project = work.project();
    let mut draft = ManuscriptDraft::from_index(
        &project
            .manuscript_query_snapshot(&[], &[])
            .unwrap()
            .indices()["book"],
    );
    draft.entries[124].summary = Some("未应用编排结尾".into());
    let input = ManuscriptQueryDraft {
        expected_baseline: project.content_baseline(),
        draft,
    };
    let mut request = query();
    request.text = "未应用编排结尾".into();
    let mut args = work.args(&json!(request));
    args.extend(["--drafts-json".into(), json!([input.clone()]).to_string()]);
    let (code, result) = invoke(args.clone());
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["page"]["rows"][0]["entry"]["id"], "chapter124");
    assert_eq!(result["page"]["source"], "draft");
    let mut stale = input;
    stale.expected_baseline = "stale".into();
    *args.last_mut().unwrap() = json!([stale]).to_string();
    let (code, result) = invoke(args);
    assert_eq!(code, 1);
    assert_eq!(result["error"]["code"], "STALE_DRAFT");
}

#[test]
fn cli_manuscript_query_rejects_duplicate_unknown_budget_and_save_options() {
    let work = Workspace::new();
    let args = work.args(&json!(query()));
    let mut duplicate = args.clone();
    duplicate[3] = r#"{"schema_version":1,"manuscript_id":"book","text":"x","text":"y"}"#.into();
    assert_eq!(invoke(duplicate).0, 2);
    let mut unknown = args.clone();
    unknown.push("--save".into());
    assert_eq!(invoke(unknown).0, 2);
    let mut repeated = args.clone();
    repeated.extend(["--query-json".into(), json!(query()).to_string()]);
    assert_eq!(invoke(repeated).0, 2);
    let mut giant = query();
    giant.text = "x".repeat(4097);
    assert_eq!(invoke(work.args(&json!(giant))).0, 2);
    let mut huge = args;
    huge.extend(["--drafts-json".into(), "x".repeat(4 * 1024 * 1024)]);
    assert_eq!(invoke(huge).0, 2);
}

#[test]
fn cli_manuscript_query_preserves_honest_partial_page() {
    let work = Workspace::new();
    let path = work.0.join(".world/manuscripts/book.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["entries"][0]["target_ref"]["id"] = json!("missing");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let (code, result) = invoke(work.args(&json!(query())));
    assert_eq!(code, 1);
    assert_eq!(result["error"]["code"], "INCOMPLETE_SNAPSHOT");
    assert_eq!(result["page"]["recognized_chapters"], 125);
}

#[test]
fn cli_manuscript_query_limits_are_strict_shared_input_validation() {
    let work = Workspace::new();
    for limit in [0, 101, usize::MAX] {
        let mut request = query();
        request.limit = limit;
        let (code, response) = invoke(work.args(&json!(request)));
        assert_eq!(code, 2);
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("INVALID_LIMIT"));
    }
    for limit in [1, 100] {
        let mut request = query();
        request.limit = limit;
        let (code, response) = invoke(work.args(&json!(request)));
        assert_eq!(code, 0);
        assert_eq!(response["page"]["limit"], limit);
    }
}

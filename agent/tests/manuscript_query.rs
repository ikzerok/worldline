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
            "wl-query-rpc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut project = Project::new(&root);
        project
            .set_text(
                &project.entry.clone(),
                "character lin as \"同名\"\nevent start\n  正文。\n  -> END\n".into(),
            )
            .unwrap();
        project.create_authoring_document(&root.join(".world/project.json"), serde_json::to_vec(&json!({"schema_version":1,"required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/manuscripts/book.json"}})).unwrap()).unwrap();
        let entries: Vec<_> = (0..125).map(|n| json!({"id":format!("chapter{n}"),"kind":"chapter","title":"同名章","summary":format!("摘要{n}"),"target_ref":{"kind":"event","id":"start"},"pov":{"kind":"character","id":"lin"},"status":if n == 124 {"review"} else {"draft"}})).collect();
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
    fn project(&self) -> Project {
        Project::open(&self.0).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn message(id: u32, method: &str, params: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()
}
fn exchange(messages: Vec<String>) -> Vec<Value> {
    let mut output = Vec::new();
    worldline_agent::run(&mut std::io::Cursor::new(messages.join("\n")), &mut output);
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn query() -> ManuscriptQueryRequest {
    ManuscriptQueryRequest {
        manuscript_id: "book".into(),
        ..Default::default()
    }
}
fn open(work: &Workspace) -> String {
    message(1, "project.open", json!({"path":work.0}))
}

#[test]
fn rpc_manuscript_query_matches_core_paging_and_never_saves() {
    let work = Workspace::new();
    let project = work.project();
    let before = fs::read(work.0.join(".world/manuscripts/book.json")).unwrap();
    let mut request = query();
    request.limit = 23;
    let expected = project
        .manuscript_query_snapshot(&[], &[])
        .unwrap()
        .query(&request)
        .unwrap();
    let mut next = request.clone();
    next.cursor = expected.next_cursor.clone();
    let result = exchange(vec![
        open(&work),
        message(
            2,
            "manuscript.query",
            json!({"project_id":"p1","query":request}),
        ),
        message(
            3,
            "manuscript.query",
            json!({"project_id":"p1","query":next}),
        ),
    ]);
    assert_eq!(result[1]["result"]["page"], json!(expected));
    assert_eq!(result[1]["result"]["ok"], true);
    assert_eq!(result[2]["result"]["page"]["offset"], 23);
    assert_eq!(
        result[2]["result"]["page"]["rows"][0]["entry"]["id"],
        "chapter23"
    );
    assert_eq!(result[1]["result"]["saved"], false);
    assert_eq!(result[1]["result"]["applied"], false);
    assert_eq!(
        fs::read(work.0.join(".world/manuscripts/book.json")).unwrap(),
        before
    );
    assert_eq!(
        work.project().content_baseline(),
        project.content_baseline()
    );
}

#[test]
fn rpc_manuscript_query_explicit_draft_and_full_scope_filter_use_same_core_dto() {
    let work = Workspace::new();
    let project = work.project();
    let snapshot = project.manuscript_query_snapshot(&[], &[]).unwrap();
    let mut draft = ManuscriptDraft::from_index(&snapshot.indices()["book"]);
    draft.entries[124].summary = Some("未应用结尾".into());
    let input = ManuscriptQueryDraft {
        expected_baseline: project.content_baseline(),
        draft,
    };
    let mut request = query();
    request.text = "未应用结尾".into();
    request.status = "review".into();
    let expected = project
        .manuscript_query_snapshot(&[], std::slice::from_ref(&input))
        .unwrap()
        .query(&request)
        .unwrap();
    let result = exchange(vec![
        open(&work),
        message(
            2,
            "manuscript.query",
            json!({"project_id":"p1","query":request,"drafts":[input]}),
        ),
    ]);
    assert_eq!(result[1]["result"]["page"], json!(expected));
    assert_eq!(result[1]["result"]["page"]["matching_chapters"], 1);
    assert_eq!(result[1]["result"]["page"]["source"], "draft");
    assert!(
        !String::from_utf8(fs::read(work.0.join(".world/manuscripts/book.json")).unwrap())
            .unwrap()
            .contains("未应用结尾")
    );
}

#[test]
fn rpc_manuscript_query_keeps_protocol_errors_and_business_failures_distinct() {
    let work = Workspace::new();
    let mut giant = query();
    giant.text = "x".repeat(4097);
    let mut stale = query();
    stale.cursor = Some("mq1:stale:1:bad".into());
    let duplicate = r#"{"jsonrpc":"2.0","id":9,"method":"manuscript.query","params":{"project_id":"p1","query":{"schema_version":1,"manuscript_id":"book","text":"x","text":"y"}}}"#.to_owned();
    let result = exchange(vec![
        open(&work),
        message(2, "manuscript.query", json!({"project_id":"p1"})),
        message(
            3,
            "manuscript.query",
            json!({"project_id":"p1","query":query(),"save":true}),
        ),
        message(
            4,
            "manuscript.query",
            json!({"project_id":"p1","query":giant}),
        ),
        message(
            5,
            "manuscript.query",
            json!({"project_id":"p1","query":stale}),
        ),
        duplicate,
        message(
            6,
            "manuscript.query",
            json!({"project_id":"p1","query":query(),"drafts":"x".repeat(4*1024*1024)}),
        ),
    ]);
    for row in [&result[1], &result[2], &result[3], &result[6]] {
        assert_eq!(row["error"]["code"], -32602);
    }
    assert_eq!(result[4]["result"]["ok"], false);
    assert_eq!(result[4]["result"]["error"]["code"], "STALE_CURSOR");
    assert_eq!(result[5]["error"]["code"], -32700);
}

#[test]
fn rpc_manuscript_query_incomplete_page_is_not_empty_success() {
    let work = Workspace::new();
    let path = work.0.join(".world/manuscripts/book.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["entries"][0]["target_ref"]["id"] = json!("missing");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let result = exchange(vec![
        open(&work),
        message(
            2,
            "manuscript.query",
            json!({"project_id":"p1","query":query()}),
        ),
    ]);
    assert_eq!(result[1]["result"]["ok"], false);
    assert_eq!(result[1]["result"]["error"]["code"], "INCOMPLETE_SNAPSHOT");
    assert_eq!(result[1]["result"]["page"]["recognized_chapters"], 125);
    assert_eq!(
        result[1]["result"]["page"]["rows"][0]["entry"]["source"]["status"],
        "missing"
    );
}

#[test]
fn rpc_manuscript_query_limits_are_strict_shared_input_validation() {
    let work = Workspace::new();
    let mut messages = vec![open(&work)];
    for (index, limit) in [0, 101, usize::MAX, 1, 100].into_iter().enumerate() {
        let mut request = query();
        request.limit = limit;
        messages.push(message(
            index as u32 + 2,
            "manuscript.query",
            json!({"project_id":"p1","query":request}),
        ));
    }
    let responses = exchange(messages);
    for response in &responses[1..4] {
        assert_eq!(response["error"]["code"], -32602);
    }
    assert_eq!(responses[4]["result"]["page"]["limit"], 1);
    assert_eq!(responses[5]["result"]["page"]["limit"], 100);
}

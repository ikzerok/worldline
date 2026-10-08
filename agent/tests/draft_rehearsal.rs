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
            "wl-draft-rpc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut project = Project::new(&root);
        project
            .set_text(
                &project.entry.clone(),
                "event start\n  正式原稿\n  -> END\n".into(),
            )
            .unwrap();
        project.save().unwrap();
        Self(root)
    }
    fn request(&self) -> Value {
        let project = Project::open_read_only(&self.0).unwrap();
        let mut draft = project.open_source_writing_buffer(&project.entry).unwrap();
        draft.replace_source("event start\n  隔离草稿OUTPUT 👋\n  -> END\n".into());
        json!({"input":DraftRehearsalRequest::from_writing_buffers(&project, &[draft], Vec::new(), false).unwrap(),"seed":11})
    }
    fn open(&self) -> String {
        message(json!(1), "project.open", json!({"path":self.0}))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn message(id: Value, method: &str, params: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()
}
fn exchange(messages: Vec<String>) -> Vec<Value> {
    let mut out = Vec::new();
    worldline_agent::run(&mut std::io::Cursor::new(messages.join("\n")), &mut out);
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn rpc_draft_isolated_output_preserves_the_live_project_and_disk() {
    let work = Workspace::new();
    let request = work.request();
    let before = fs::read(work.0.join("world.wl")).unwrap();
    let responses = exchange(vec![
        work.open(),
        message(
            json!(2),
            "project.draft_rehearsal",
            json!({"project_id":"p1","request":request}),
        ),
        message(json!(3), "project.analyze", json!({"project_id":"p1"})),
    ]);
    let result = &responses[1]["result"];
    assert_eq!(result["ok"], true, "{responses:?}");
    assert_eq!(result["applied"], false);
    assert_eq!(result["saved"], false);
    assert!(result["result"]["outputs"]
        .to_string()
        .contains("隔离草稿OUTPUT"));
    assert_eq!(result["result"]["outputs_complete"], true);
    assert_eq!(
        responses[0]["result"]["baseline"],
        responses[2]["result"]["baseline"]
    );
    assert_eq!(fs::read(work.0.join("world.wl")).unwrap(), before);
}

#[test]
fn rpc_draft_stale_is_business_failure_and_bad_dto_is_protocol_failure() {
    let work = Workspace::new();
    let mut stale = work.request();
    stale["input"]["content_baseline"] = json!("stale");
    let mut invalid = work.request();
    invalid["seed"] = json!(9_007_199_254_740_992u64);
    let responses = exchange(vec![
        work.open(),
        message(
            json!(2),
            "project.draft_rehearsal",
            json!({"project_id":"p1","request":stale}),
        ),
        message(
            json!(3),
            "project.draft_rehearsal",
            json!({"project_id":"p1","request":invalid}),
        ),
        message(
            json!(4),
            "project.draft_rehearsal",
            json!({"project_id":"p1","request":work.request(),"save":true}),
        ),
    ]);
    assert_eq!(responses[1]["result"]["ok"], false);
    assert!(responses[1].get("error").is_none());
    assert_eq!(responses[2]["error"]["code"], -32602);
    assert_eq!(responses[3]["error"]["code"], -32602);
}

#[test]
fn rpc_draft_notifications_and_bounded_request_ids_follow_protocol() {
    let work = Workspace::new();
    let notification = json!({"jsonrpc":"2.0","method":"project.draft_rehearsal","params":{"project_id":"p1","request":work.request()}}).to_string();
    let giant = message(
        json!("x".repeat(3000)),
        "project.draft_rehearsal",
        json!({"project_id":"p1","request":work.request()}),
    );
    let responses = exchange(vec![work.open(), notification, giant]);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[1]["error"]["code"], -32600);
    assert!(responses[1]["id"].is_null());
    assert!(responses[1].to_string().len() < 4096);
}

#[test]
fn rpc_draft_rejects_duplicate_keys_and_advertises_capability() {
    let work = Workspace::new();
    let request = work.request();
    let malformed = format!("{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"project.draft_rehearsal\",\"params\":{{\"project_id\":\"p1\",\"request\":{{\"input\":{},\"seed\":1,\"seed\":2}}}}}}", request["input"]);
    let responses = exchange(vec![
        message(json!(0), "initialize", json!({})),
        work.open(),
        malformed,
    ]);
    assert!(responses[0]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "authoring.draft_rehearsal.v1"));
    assert_eq!(responses[2]["error"]["code"], -32700);
}

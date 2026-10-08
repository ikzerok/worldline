use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Cursor,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use worldline_core::{
    manuscript::{generate_manuscript_delivery, ManuscriptDeliveryRequest, ManuscriptQueryRequest},
    project::Project,
};
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rpc-manuscript-delivery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut project = Project::new(&root);
        let entry = project.entry.clone();
        project.documents.retain(|path, _| path == &entry);
        project
            .set_text(
                &entry,
                "event start\n  if true\n    甲😀\n  else\n    乙\n  -> END\n".into(),
            )
            .unwrap();
        project.create_authoring_document(&root.join(".world/project.json"), serde_json::to_vec(&json!({"schema_version":1,"required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/manuscripts/book.json"}})).unwrap()).unwrap();
        project.create_authoring_document(&root.join(".world/manuscripts/book.json"), serde_json::to_vec(&json!({"schema_version":1,"id":"book","title":"书","entries":[{"id":"first","kind":"chapter","title":"同名","target_ref":{"kind":"event","id":"start"}},{"id":"again","kind":"chapter","title":"同名","target_ref":{"kind":"event","id":"start"}}]})).unwrap()).unwrap();
        project.save().unwrap();
        Self(root)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn request() -> ManuscriptDeliveryRequest {
    ManuscriptDeliveryRequest::new(ManuscriptQueryRequest {
        manuscript_id: "book".into(),
        ..Default::default()
    })
}
fn exchange(messages: Vec<Value>) -> Vec<Value> {
    let lines = messages
        .into_iter()
        .map(|message| message.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    worldline_agent::run(&mut Cursor::new(lines), &mut out);
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn message(id: u32, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

#[test]
fn rpc_uses_same_complete_scope_and_private_markdown_without_side_effects() {
    let workspace = Workspace::new();
    // 与project.open RPC采用同一初始化/刷新代次；不可删除快照身份等价断言。
    let mut project = Project::open(&workspace.0).unwrap();
    let _ = project.compile();
    let before: BTreeMap<_, _> = project
        .documents
        .iter()
        .map(|(path, doc)| (path.clone(), doc.text.clone()))
        .collect();
    let report = generate_manuscript_delivery(
        project
            .manuscript_delivery_snapshot(&[], &[], &request())
            .unwrap(),
        &mut |_| true,
    )
    .unwrap();
    let responses = exchange(vec![
        message(1, "project.open", json!({"path":workspace.0})),
        message(
            2,
            "manuscript.delivery",
            json!({"project_id":"p1","request":request()}),
        ),
    ]);
    let response = &responses[1];
    assert_eq!(response["result"]["ok"], true, "{response}");
    assert_eq!(response["result"]["report"], json!(report));
    assert_eq!(response["result"]["saved"], false);
    assert_eq!(response["result"]["delivered"], false);
    assert_eq!(
        response["result"]["report"]["scope"]["selected_occurrences"],
        2
    );
    for (path, text) in before {
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
}

#[test]
fn rpc_strict_dto_and_business_staleness_have_different_error_channels() {
    let workspace = Workspace::new();
    let mut invalid = json!(request());
    invalid["unknown"] = json!(true);
    let mut stale = request();
    stale.expected_snapshot_key = Some("old".into());
    let responses = exchange(vec![
        message(1, "project.open", json!({"path":workspace.0})),
        message(
            2,
            "manuscript.delivery",
            json!({"project_id":"p1","request":invalid}),
        ),
        message(
            3,
            "manuscript.delivery",
            json!({"project_id":"p1","request":stale}),
        ),
    ]);
    assert_eq!(responses[1]["error"]["code"], -32602);
    assert_eq!(responses[2]["result"]["error"]["code"], "STALE_SNAPSHOT");
    assert!(responses[2].get("error").is_none());
    assert!(responses[2]["result"]["report"].is_null());
}

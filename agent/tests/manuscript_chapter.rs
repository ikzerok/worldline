use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{
    manuscript::ManuscriptChapterCreateRequest, presentation_commands::Revision, project::Project,
};
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "wl-chapter-rpc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        Project::new(&root).save().unwrap();
        Self(root)
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
fn request(project: &Project) -> Value {
    json!({"schema_version":1,"expected_baseline":project.content_baseline(),"expected_revision":Revision::default(),
        "book":{"kind":"new","id":"book","title":"书稿"},"chapter":{"id":"one","title":"第一章"},
        "source":{"kind":"new_event","id":"first","storyline":"main","destination":{"kind":"new_active_source","relative_path":"chapters/first.wl"}}})
}
#[test]
fn rpc_matches_core_previews_memory_apply_and_explicit_save() {
    let work = Workspace::new();
    let mut project = Project::open(&work.0).unwrap();
    let request: ManuscriptChapterCreateRequest =
        serde_json::from_value(request(&project)).unwrap();
    let plan = project
        .preview_manuscript_chapter_create(Revision::default(), &request)
        .unwrap();
    let expected = project
        .apply_manuscript_chapter_create(&mut Revision::default(), &request, &plan.plan_digest)
        .unwrap();
    let open = message(1, "project.open", json!({"path":work.0}));
    let apply = message(
        3,
        "manuscript.chapter.apply",
        json!({"project_id":"p1","request":request,"plan_digest":plan.plan_digest}),
    );
    let result = exchange(vec![
        open.clone(),
        message(
            2,
            "manuscript.chapter.preview",
            json!({"project_id":"p1","request":request}),
        ),
        apply.clone(),
    ]);
    assert_eq!(result[0]["result"]["revision"], json!(Revision::default()));
    assert_eq!(result[1]["result"]["plan"], json!(plan));
    assert_eq!(result[2]["result"]["result"], json!(expected));
    assert_eq!(result[2]["result"]["saved"], false);
    assert!(!work.0.join("chapters").exists());
    assert!(!work.0.join(".world/project.json").exists());
    let result = exchange(vec![
        open,
        apply.clone(),
        apply,
        message(
            4,
            "project.save",
            json!({"project_id":"p1","expected_baseline":expected.new_baseline}),
        ),
    ]);
    assert_eq!(result[2]["result"]["ok"], false);
    assert_eq!(result[2]["result"]["error"]["code"], "STALE_BASELINE");
    assert!(result[2].get("error").is_none());
    assert_eq!(result[3]["result"]["saved"], true);
    assert_eq!(
        Project::open(&work.0)
            .unwrap()
            .manuscript_index("book")
            .unwrap()
            .entries
            .len(),
        1
    );
}
#[test]
fn rpc_strict_shapes_duplicate_keys_and_business_failures_are_distinct() {
    let work = Workspace::new();
    let project = Project::open(&work.0).unwrap();
    let request = request(&project);
    let mut unknown = request.clone();
    unknown["expected_revision"]["unknown"] = json!(true);
    let duplicate = message(
        8,
        "manuscript.chapter.preview",
        json!({"project_id":"p1","request":request}),
    )
    .replace(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
    );
    let messages = vec![
        message(1, "project.open", json!({"path":work.0})),
        message(2, "initialize", json!({})),
        message(
            3,
            "manuscript.chapter.preview",
            json!({"project_id":"p1","request":request,"unknown":true}),
        ),
        message(
            4,
            "manuscript.chapter.preview",
            json!({"project_id":"p1","request":unknown}),
        ),
        message(
            5,
            "manuscript.chapter.apply",
            json!({"project_id":"p1","request":request,"plan_digest":"wrong"}),
        ),
        message(
            6,
            "manuscript.chapter.apply",
            json!({"project_id":"p1","request":request}),
        ),
        message(
            7,
            "manuscript.chapter.preview",
            json!({"project_id":"missing","request":request}),
        ),
        duplicate,
    ];
    let result = exchange(messages);
    assert!(result[1]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("authoring.manuscript_chapter.v1")));
    for index in [2, 3, 5, 6] {
        assert_eq!(result[index]["error"]["code"], -32602, "{}", result[index]);
    }
    assert_eq!(result[4]["result"]["error"]["code"], "STALE_BASELINE");
    assert_eq!(result[7]["error"]["code"], -32700);
    assert!(!work.0.join("chapters").exists());
}

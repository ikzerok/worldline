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
            "wl-template-rpc-{}-{}-{}",
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
            "character author as \"作者\"\nevent start\n  -> END\n",
        )
        .unwrap();
        Self(root)
    }
    fn project(&self) -> Project {
        Project::open_read_only(&self.0).unwrap()
    }
    fn open(&self) -> String {
        msg(1, "project.open", json!({"path":self.0}))
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn msg(id: u32, method: &str, params: Value) -> String {
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
fn mutation(project: &Project) -> TemplateMutationRequest {
    TemplateMutationRequest { schema_version: 1, expected_revision: Revision::default(),
        expected_baseline: project.content_baseline(), intent: TemplateMutationIntent::Upsert {
            draft: ProjectTemplateDraft { source_bytes: serde_json::to_vec(&json!({
                "schema_version":1,"id":"project:profile","title":"人物笔记",
                "applies_to":{"kind":"character"},
                "fields":[{"id":"note","key":"note","label":"备注","type":"text","required":false,"default":"提示"}],
                "extension":{"keep":true}
            })).unwrap(), ..Default::default() }
        } }
}

#[test]
fn rpc_template_draft_projection_and_edit_use_core_without_project_writes() {
    let work = Work::new();
    let project = work.project();
    let request: TemplateDraftRequest = serde_json::from_value(json!({
        "schema_version":1,"action":{"kind":"open","source":{"kind":"new"}}
    }))
    .unwrap();
    let expected = project.template_draft_request(&request).unwrap();
    let draft = expected.projection.draft.clone();
    let edit: TemplateDraftRequest = serde_json::from_value(json!({"schema_version":1,
        "expected_baseline":project.content_baseline(),"action":{"kind":"edit","draft":draft,
        "edit":{"kind":"add_field","parent_id":null,"index":0,"field_type":"text"}}}))
    .unwrap();
    let expected_edit = project.template_draft_request(&edit).unwrap();
    let rows = exchange(vec![
        work.open(),
        msg(2, "initialize", json!({})),
        msg(
            3,
            "template.draft",
            json!({"project_id":"p1","request":request}),
        ),
        msg(
            4,
            "template.draft",
            json!({"project_id":"p1","request":edit}),
        ),
    ]);
    assert!(rows[1]["result"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == TEMPLATE_AUTHORING_CAPABILITY));
    assert_eq!(rows[2]["result"]["projection"], json!(expected.projection));
    assert_eq!(
        rows[3]["result"]["projection"],
        json!(expected_edit.projection)
    );
    assert_eq!(rows[3]["result"]["applied"], false);
    assert!(!work.0.join(".world/project.json").exists());
    assert_eq!(
        work.project().content_baseline(),
        project.content_baseline()
    );
}

#[test]
fn rpc_template_preview_apply_and_explicit_save_match_core_and_keep_instances() {
    let work = Work::new();
    let mut project = work.project();
    let request = mutation(&project);
    let mut revision = Revision::default();
    let plan = project
        .preview_template_request(revision, &request)
        .unwrap();
    let result = project
        .apply_template_request(&mut revision, &request, &plan.plan_digest)
        .unwrap();
    let apply = msg(
        3,
        "template.apply",
        json!({"project_id":"p1","request":request,"plan_digest":plan.plan_digest}),
    );
    let rows = exchange(vec![
        work.open(),
        msg(
            2,
            "template.preview",
            json!({"project_id":"p1","request":request}),
        ),
        apply.clone(),
    ]);
    assert_eq!(rows[1]["result"]["plan"], json!(plan));
    assert_eq!(rows[2]["result"]["ok"], true, "{}", rows[2]);
    assert_eq!(rows[2]["result"]["saved"], false);
    assert!(!work.0.join(".world/project.json").exists());
    let rows = exchange(vec![
        work.open(),
        apply,
        msg(
            4,
            "project.save",
            json!({
        "project_id":"p1","expected_baseline":result.baseline}),
        ),
    ]);
    assert_eq!(rows[2]["result"]["saved"], true, "{}", rows[2]);
    let reopened = work.project();
    let index = reopened.template_index();
    assert_eq!(
        index.projects["project:profile"]
            .source_document
            .as_ref()
            .unwrap()["extension"]["keep"],
        true
    );
    assert!(!fs::read_to_string(work.0.join("world.wl"))
        .unwrap()
        .contains("property note"));
}

#[test]
fn rpc_template_strict_wire_and_tampered_digest_never_write() {
    let work = Work::new();
    let project = work.project();
    let request = mutation(&project);
    let duplicate = msg(
        9,
        "template.preview",
        json!({"project_id":"p1","request":request}),
    )
    .replace(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
    );
    let rows = exchange(vec![
        work.open(),
        msg(
            2,
            "template.draft",
            json!({"project_id":"p1","request":{"schema_version":1,
            "action":{"kind":"open","source":{"kind":"new","future":true}}}}),
        ),
        msg(
            3,
            "template.preview",
            json!({"project_id":"p1","request":request,"future":true}),
        ),
        msg(
            4,
            "template.apply",
            json!({"project_id":"p1","request":request,"plan_digest":"forged"}),
        ),
        msg(
            5,
            "template.draft",
            json!({"project_id":"p1","request":{"schema_version":1,
            "expected_baseline":"x".repeat(MAX_TEMPLATE_REQUEST_BYTES),"action":{"kind":"open","source":{"kind":"new"}}}}),
        ),
        duplicate,
    ]);
    assert_eq!(rows[1]["error"]["code"], -32602);
    assert_eq!(rows[2]["error"]["code"], -32602);
    assert_eq!(rows[3]["result"]["error"]["code"], "STALE_PLAN");
    assert_eq!(rows[4]["error"]["code"], -32602);
    assert_eq!(rows[5]["error"]["code"], -32700);
    assert!(!work.0.join(".world/project.json").exists());
}

#[test]
fn rpc_template_incomplete_source_is_not_complete_empty_impact() {
    let work = Work::new();
    fs::write(
        work.0.join("world.wl"),
        "include \"missing.wl\"\ncharacter author\nevent start\n  -> END\n",
    )
    .unwrap();
    let request = mutation(&work.project());
    let rows = exchange(vec![
        work.open(),
        msg(
            2,
            "template.preview",
            json!({"project_id":"p1","request":request}),
        ),
    ]);
    assert_eq!(rows[1]["result"]["ok"], false, "{}", rows[1]);
    assert_eq!(rows[1]["result"]["plan"]["complete"], false);
    assert_eq!(rows[1]["result"]["plan"]["can_apply"], false);
    assert!(rows[1]["result"]["plan"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "A105"));
    assert!(!work.0.join(".world/project.json").exists());
}

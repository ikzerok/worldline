use serde_json::{json, Value};
use std::{
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
            "cli-manuscript-delivery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut project = Project::new(&root);
        let entry = project.entry.clone();
        project.documents.retain(|path, _| path == &entry);
        project.set_text(&entry, "event start\n  if true\n    中文😀甲\n  else\n    中文乙\n  -> END\nevent second\n  不在筛选中\n  -> END\n".into()).unwrap();
        project.create_authoring_document(&root.join(".world/project.json"), serde_json::to_vec(&json!({"schema_version":1,"required_features":["presentation.manuscripts.v1"],"manuscripts":{"book":".world/manuscripts/book.json"}})).unwrap()).unwrap();
        project.create_authoring_document(&root.join(".world/manuscripts/book.json"), serde_json::to_vec(&json!({"schema_version":1,"id":"book","title":"书","entries":[{"id":"first","kind":"chapter","title":"同名","status":"review","target_ref":{"kind":"event","id":"start"}},{"id":"second","kind":"chapter","title":"同名","status":"draft","target_ref":{"kind":"event","id":"second"}}]})).unwrap()).unwrap();
        project.save().unwrap();
        Self(root)
    }
    fn run(&self, request: Value, extra: &[String]) -> (i32, Value) {
        let mut args = vec![
            "manuscript-delivery".into(),
            self.0.display().to_string(),
            "--request-json".into(),
            request.to_string(),
            "--json".into(),
        ];
        args.extend_from_slice(extra);
        let mut out = Vec::new();
        let code = wl::run(&args, &mut out, &mut Cursor::new("")).unwrap();
        (code, serde_json::from_slice(&out).unwrap())
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
        status: "review".into(),
        ..Default::default()
    })
}

#[test]
fn cli_material_matches_core_and_does_not_save_or_change_source() {
    let workspace = Workspace::new();
    let project = Project::open_read_only(&workspace.0).unwrap();
    let before = std::fs::read(workspace.0.join("world.wl")).unwrap();
    let expected = generate_manuscript_delivery(
        project
            .manuscript_delivery_snapshot(&[], &[], &request())
            .unwrap(),
        &mut |_| true,
    )
    .unwrap();
    let (code, value) = workspace.run(json!(request()), &[]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["report"], json!(expected));
    assert_eq!(value["applied"], false);
    assert_eq!(value["saved"], false);
    assert_eq!(value["delivered"], false);
    assert_eq!(value["report"]["scope"]["selected_occurrences"], 1);
    assert!(!value["report"]["markdown"]
        .as_str()
        .unwrap()
        .contains("不在筛选中"));
    assert_eq!(std::fs::read(workspace.0.join("world.wl")).unwrap(), before);
}

#[test]
fn cli_export_is_exact_new_markdown_and_rejects_existing_target() {
    let workspace = Workspace::new();
    let target = workspace.0.with_extension("md");
    let extra = ["--output".into(), target.display().to_string()];
    let (code, value) = workspace.run(json!(request()), &extra);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["delivered"], true);
    let bytes = std::fs::read(&target).unwrap();
    assert_eq!(
        bytes,
        value["report"]["markdown"].as_str().unwrap().as_bytes()
    );
    let (code, value) = workspace.run(json!(request()), &extra);
    assert_eq!(code, 1);
    assert_eq!(value["delivered"], false);
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    std::fs::remove_file(target).unwrap();
}

#[test]
fn cli_parameter_errors_and_business_failures_remain_distinct() {
    let workspace = Workspace::new();
    let mut value = json!(request());
    value["future"] = json!(true);
    assert_eq!(workspace.run(value, &[]).0, 2);
    let mut input = request();
    input.chapter_ids = Some(vec!["second".into()]);
    let (code, value) = workspace.run(json!(input), &[]);
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "SELECTION_OUTSIDE_SCOPE");
    std::fs::write(workspace.0.join("world.wl"), "event start\n  if (\n").unwrap();
    let (code, value) = workspace.run(json!(request()), &[]);
    assert_eq!(code, 1);
    assert_eq!(value["ok"], false);
    assert!(value["report"]["markdown"].is_null());
}

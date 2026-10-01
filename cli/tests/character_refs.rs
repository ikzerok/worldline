use serde_json::{json, Value};
use worldline_core::{project::Project, source_edit::SourceEditRequest, TargetRef};
const SOURCE: &str = "schema vessel for entity\n  field captain_id captain ref character required\ncharacter lin as \"林舟\"\nentity boat kind ship\n  property captain = ref(\"character\", \"lin\")\n  property plain = \"lin\"\nbind entity boat to vessel\nevent start with lin\n  [[character:lin|林舟]]\n  -> END\n";
fn invoke(args: Vec<String>) -> (i32, Value) {
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}
#[test]
fn character_refs_cli_reads_manifest_and_previews_core_rename_candidate_without_writes() {
    let root = std::env::temp_dir().join(format!("wl-character-refs-cli-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1"]}"#).unwrap();
    std::fs::write(root.join("world.wl"), SOURCE).unwrap();
    let mut project = Project::open(&root).unwrap();
    let core = project.compile();
    let path = root.to_string_lossy().into_owned();
    let (code, catalog) = invoke(vec!["catalog".into(), path.clone(), "--json".into()]);
    assert_eq!(code, 0, "{catalog}");
    assert_eq!(catalog["catalog"], json!(core.analysis.catalog));
    assert_eq!(
        catalog["catalog"]["entities"]["boat"]["properties"]["captain"],
        json!({"kind":"character","id":"lin"})
    );
    let (code, index) = invoke(vec!["schema-index".into(), path.clone(), "--json".into()]);
    assert_eq!(code, 0);
    assert_eq!(index["index"], json!(project.schema_index()));
    let baseline = project.content_baseline();
    let plan = project
        .plan_rename_target(&TargetRef::new("character", "lin"), "navigator")
        .unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let request = SourceEditRequest {
        schema_version: 1,
        path: "world.wl".into(),
        expected_baseline: baseline.clone(),
        source: project.document(&root.join("world.wl")).unwrap().into(),
    };
    assert!(request.source.contains("property plain = \"lin\""));
    let (code, preview) = invoke(vec![
        "source-edit".into(),
        "preview".into(),
        path,
        "--request-json".into(),
        json!(request).to_string(),
        "--json".into(),
    ]);
    assert_eq!(code, 0, "{preview}");
    assert!(preview["preview"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        preview["preview"]["plan_digest"],
        project.content_baseline()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        SOURCE
    );
    std::fs::remove_dir_all(root).unwrap();
}

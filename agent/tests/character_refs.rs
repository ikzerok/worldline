use serde_json::{json, Value};
use worldline_core::{project::Project, source_edit::SourceEditRequest, TargetRef};
const SOURCE: &str = "schema vessel for entity\n  field captain_id captain ref character required\ncharacter lin as \"林舟\"\nentity boat kind ship\n  property captain = ref(\"character\", \"lin\")\n  property plain = \"lin\"\nbind entity boat to vessel\nevent start with lin\n  [[character:lin|林舟]]\n  -> END\n";
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();
    assert_eq!(
        worldline_agent::run(&mut std::io::Cursor::new(input), &mut out),
        0
    );
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
#[test]
fn character_refs_rpc_reads_manifest_catalog_schema_and_checks_core_candidate_baseline() {
    let root = std::env::temp_dir().join(format!("wl-character-refs-rpc-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1"]}"#).unwrap();
    std::fs::write(root.join("world.wl"), SOURCE).unwrap();
    let mut project = Project::open(&root).unwrap();
    let compiled = project.compile();
    let baseline = project.content_baseline();
    let index = project.schema_index();
    let plan = project
        .plan_rename_target(&TargetRef::new("character", "lin"), "navigator")
        .unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let dto = SourceEditRequest {
        schema_version: 1,
        path: "world.wl".into(),
        expected_baseline: baseline,
        source: project.document(&root.join("world.wl")).unwrap().into(),
    };
    let rows = exchange(vec![
        request(1, "compile", json!({"path":root})),
        request(2, "analyze", json!({"story_id":"s1"})),
        request(3, "schema.index", json!({"path":root})),
        request(4, "source.edit.preview", json!({"path":root,"request":dto})),
        request(
            5,
            "source.edit.apply",
            json!({"path":root,"request":dto,"plan_digest":"wrong"}),
        ),
    ]);
    assert_eq!(rows[0]["result"]["ok"], true, "{}", rows[0]);
    assert_eq!(
        rows[1]["result"]["catalog"],
        json!(compiled.analysis.catalog)
    );
    assert_eq!(rows[2]["result"]["index"], json!(index));
    assert_eq!(rows[3]["result"]["ok"], true, "{}", rows[3]);
    assert_eq!(
        rows[3]["result"]["preview"]["plan_digest"],
        project.content_baseline()
    );
    assert_eq!(rows[4]["result"]["ok"], false);
    assert!(rows.iter().all(|row| row.get("error").is_none()));
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        SOURCE
    );
    std::fs::remove_dir_all(root).unwrap();
}

use serde_json::{json, Value};
const SOURCE: &str = "schema city for entity entity_type place closed\n  field people population number required\nentity harbor kind place\n  property population = 0\nbind entity harbor to city\nevent start\n  -> END\n";
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    worldline_agent::run(&mut std::io::Cursor::new(input), &mut output);
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
#[test]
fn schema_rpc_index_preview_and_apply_share_core_and_preserve_rejected_edits() {
    let root = std::env::temp_dir().join(format!("wl-schema-rpc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
    )
    .unwrap();
    std::fs::write(root.join("world.wl"), SOURCE).unwrap();
    let project = worldline_core::project::Project::open(&root).unwrap();
    let changed = SOURCE.replace("population number required", "population text required");
    let dto = json!({"schema_version":1,"path":"world.wl","expected_baseline":project.content_baseline(),"source":changed});
    let core_request: worldline_core::source_edit::SourceEditRequest =
        serde_json::from_value(dto.clone()).unwrap();
    let core_preview = project.preview_schema_edit(&core_request).unwrap();
    let rows = exchange(vec![
        request(1, "schema.index", json!({"path":root})),
        request(2, "schema.edit.preview", json!({"path":root,"request":dto})),
        request(
            3,
            "schema.edit.apply",
            json!({"path":root,"request":dto,"plan_digest":"wrong"}),
        ),
        request(4, "schema.index", json!({"path":root})),
    ]);
    assert_eq!(rows[0]["result"]["index"], json!(project.schema_index()));
    assert_eq!(rows[1]["result"]["preview"], json!(core_preview));
    assert_eq!(rows[2]["result"]["ok"], false);
    assert_eq!(rows[2]["result"]["error"]["code"], "SCHEMA_EDIT_REJECTED");
    assert_eq!(rows[0]["result"]["baseline"], rows[3]["result"]["baseline"]);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        SOURCE
    );
    let rows = exchange(vec![
        request(
            1,
            "schema.edit.apply",
            json!({"path":root,"request":dto,"plan_digest":core_preview.plan_digest}),
        ),
        request(
            2,
            "schema.edit.apply",
            json!({"path":root,"request":dto,"plan_digest":core_preview.plan_digest}),
        ),
    ]);
    assert_eq!(rows[0]["result"]["ok"], true, "{}", rows[0]);
    assert_eq!(rows[1]["result"]["ok"], false);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        changed
    );
    std::fs::remove_dir_all(root).unwrap();
}

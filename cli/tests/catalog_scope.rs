use serde_json::{json, Value};
use std::io::Cursor;

#[test]
fn catalog_scope_cli_uses_same_complete_query_and_formal_projection() {
    let root = std::env::temp_dir().join(format!("cli-catalog-scope-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join(".world/project.json"),r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","content.relations.v1"]}"#).unwrap();
    std::fs::write(root.join("world.wl"),"entity alpha kind place\nentity beta kind place\nrelation_type linked\nrelation_def edge type linked from entity alpha to entity beta\nevent start\n  -> END\n").unwrap();
    let before = std::fs::read(root.join("world.wl")).unwrap();
    let mut output = Vec::new();
    let args = vec![
        "catalog-scope".into(),
        root.to_string_lossy().to_string(),
        "--query".into(),
        json!({"schema_version":1,"filters":[{"dimension":"kind","values":["entity"]}]})
            .to_string(),
        "--page-size".into(),
        "1".into(),
        "--focus".into(),
        json!({"kind":"entity","id":"alpha"}).to_string(),
        "--json".into(),
    ];
    let code = wl::run(&args, &mut output, &mut Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 0);
    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["scope"]["counts"]["matching_objects"], 2);
    assert_eq!(value["page"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(value["relations"]["edges"].as_array().unwrap().len(), 1);
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), before);
    std::fs::remove_dir_all(root).unwrap();
}

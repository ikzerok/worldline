use serde_json::{json, Value};
use worldline_core::{catalog_import::CatalogImportRequest, project::Project};
fn exchange(messages: Vec<String>) -> Vec<Value> {
    let mut output = Vec::new();
    worldline_agent::run(&mut std::io::Cursor::new(messages.join("\n")), &mut output);
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
#[test]
fn catalog_import_rpc_memory_then_save_and_error_boundaries() {
    let root = std::env::temp_dir().join(format!("catalog-import-rpc-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let source = "character lin as \"林\"\nevent start\n  -> END\n";
    std::fs::write(root.join("world.wl"), source).unwrap();
    let mut project = Project::open(&root).unwrap();
    let request:CatalogImportRequest=serde_json::from_value(json!({"schema_version":1,"expected_baseline":project.content_baseline(),"destination":"world.wl","csv":"kind,id,name\ncharacter,lin,新\n","columns":[{"column":0,"field":{"kind":"kind"}},{"column":1,"field":{"kind":"id"}},{"column":2,"field":{"kind":"display"}}]})).unwrap();
    let plan = project.preview_catalog_import(&request).unwrap();
    let applied = project
        .apply_catalog_import(&request, &plan.plan_digest)
        .unwrap();
    let msg = |id, method, params| {
        json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string()
    };
    let open = msg(1, "project.open", json!({"path":root}));
    let apply = msg(
        3,
        "catalog.import.apply",
        json!({"project_id":"p1","request":request,"plan_digest":plan.plan_digest}),
    );
    let result = exchange(vec![
        open.clone(),
        msg(
            2,
            "catalog.import.preview",
            json!({"project_id":"p1","request":request}),
        ),
        apply.clone(),
    ]);
    assert_eq!(result[1]["result"]["plan"], json!(plan));
    assert_eq!(result[2]["result"]["saved"], false);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        source
    );
    let result = exchange(vec![
        open,
        apply,
        msg(
            4,
            "project.save",
            json!({"project_id":"p1","expected_baseline":applied.new_baseline}),
        ),
        msg(
            5,
            "catalog.import.preview",
            json!({"project_id":"p1","request":request}),
        ),
    ]);
    assert_eq!(result[2]["result"]["saved"], true);
    assert_eq!(result[3]["result"]["ok"], false);
    assert!(result[3].get("error").is_none());
    let duplicate = msg(
        6,
        "catalog.import.preview",
        json!({"project_id":"p1","request":request}),
    )
    .replace(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
    );
    let result = exchange(vec![duplicate]);
    assert_eq!(result[0]["error"]["code"], -32700);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn catalog_import_rpc_strict_enum_unknown_duplicate_and_missing_keys() {
    let root = std::env::temp_dir().join(format!("catalog-import-rpc-wire-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let source = "event start\n  -> END\n";
    std::fs::write(root.join("world.wl"), source).unwrap();
    let project = Project::open(&root).unwrap();
    let mut invalid_fields = [
        "kind",
        "id",
        "display",
        "entity_type",
        "description",
        "ignore",
    ]
    .into_iter()
    .map(|kind| json!({"kind":kind,"unknown":true}))
    .collect::<Vec<_>>();
    for kind in ["text", "number", "bool"] {
        invalid_fields
            .push(json!({"kind":"property","key":"x","value_type":{"kind":kind,"unknown":true}}));
    }
    invalid_fields.extend([json!({"kind":"property","key":"x","value_type":{"kind":"ref","target_kind":"entity","unknown":true}}),json!({"kind":"property","key":"x","value_type":{"kind":"text"},"unknown":true}),json!({}),json!({"kind":"property","value_type":{"kind":"text"}}),json!({"kind":"property","key":"x"}),json!({"kind":"property","key":"x","value_type":{"kind":"ref"}})]);
    let make = |id, field| {
        json!({"jsonrpc":"2.0","id":id,"method":"catalog.import.preview","params":{"project_id":"p1","request":{"schema_version":1,"expected_baseline":project.content_baseline(),"destination":"world.wl","csv":"kind,id\n","columns":[{"column":0,"field":field},{"column":1,"field":{"kind":"id"}}]}}}).to_string()
    };
    let mut messages = vec![
        json!({"jsonrpc":"2.0","id":0,"method":"project.open","params":{"path":root}}).to_string(),
    ];
    for (index, field) in invalid_fields.into_iter().enumerate() {
        messages.push(make(index + 1, field));
    }
    let protocol_count = messages.len();
    messages.push(
        make(100, json!({"kind":"kind"}))
            .replace("\"kind\":\"kind\"", "\"kind\":\"kind\",\"kind\":\"id\""),
    );
    messages.push(
        make(
            101,
            json!({"kind":"property","key":"x","value_type":{"kind":"ref","target_kind":"entity"}}),
        )
        .replace(
            "\"target_kind\":\"entity\"",
            "\"target_kind\":\"entity\",\"target_kind\":\"character\"",
        ),
    );
    let result = exchange(messages);
    for response in &result[1..protocol_count] {
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    for response in &result[protocol_count..] {
        assert_eq!(response["error"]["code"], -32700, "{response}");
    }
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        source
    );
    std::fs::remove_dir_all(root).unwrap();
}

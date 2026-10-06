use super::*;

#[test]
fn catalog_sort_rpc_uses_core_order_and_preserves_cursor_and_error_boundaries() {
    let root = temp_entity_project(
        "catalog-sort-rpc",
        "entity a kind place as \"Z\"\nentity b kind place as \"A\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let query = json!({"schema_version":2,"filters":[{"dimension":"kind","values":["entity"]}],"sort":{"field":"name","direction":"ascending"}});
    let (_, results) = exchange(&[req(
        1,
        "catalog.query",
        json!({"path":path,"query":query,"page_size":1}),
    )]);
    let first = &results[0]["result"];
    assert_eq!(first["ok"], true);
    assert_eq!(first["query"]["schema_version"], 1);
    assert_eq!(first["query"]["items"][0]["target"]["id"], "b");
    assert_eq!(first["query"]["items"][0]["display"], "A");
    let cursor = &first["query"]["next"];
    let mut descending = query.clone();
    descending["sort"]["direction"] = json!("descending");
    let mut v1_sort = query.clone();
    v1_sort["schema_version"] = json!(1);
    let mut unknown = query.clone();
    unknown["sort"]["field"] = json!("future");
    let (_, results) = exchange(&[
        req(
            2,
            "catalog.query",
            json!({"path":path,"query":query,"cursor":cursor}),
        ),
        req(
            3,
            "catalog.query",
            json!({"path":path,"query":descending,"cursor":cursor}),
        ),
        req(4, "catalog.query", json!({"path":path,"query":v1_sort})),
        req(5, "catalog.query", json!({"path":path,"query":unknown})),
    ]);
    assert_eq!(
        results[0]["result"]["query"]["items"][0]["target"]["id"],
        "a"
    );
    assert_eq!(results[1]["result"]["error"]["code"], "STALE_CURSOR");
    assert_eq!(results[2]["result"]["error"]["code"], "INVALID_QUERY");
    assert_eq!(results[3]["error"]["code"], -32602);
}

#[test]
fn reference_property_query_rpc_matches_cli_contract_and_preserves_error_boundaries() {
    let root = temp_workspace(
        "catalog-reference-rpc",
        r#"{"schema_version":1,"language_version":"1.13","entry":"world.wl","required_features":["content.object_refs.v1","content.character_refs.v1"]}"#,
        "character keeper as \"保管人\"\nentity ledger kind document\n  property custodian = ref(\"character\", \"keeper\")\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().into_owned();
    let query = json!({"schema_version":3,"filters":[{"dimension":"property","values":[{"key":"custodian","equals":{"type":"reference","value":{"kind":"character","id":"keeper"}}}]}]});
    let mut missing = query.clone();
    missing["filters"][0]["values"][0]["equals"]["value"]["id"] = json!("absent");
    let mut old = query.clone();
    old["schema_version"] = json!(1);
    let mut invalid = query.clone();
    invalid["filters"][0]["values"][0]["equals"]["value"]["kind"] = json!("event");
    let (_, responses) = exchange(&[
        req(1, "catalog.query", json!({"path":path,"query":query})),
        req(2, "catalog.query", json!({"path":path,"query":missing})),
        req(3, "catalog.query", json!({"path":path,"query":old})),
        req(4, "catalog.query", json!({"path":path,"query":invalid})),
    ]);
    assert_eq!(responses[0]["result"]["ok"], true);
    assert_eq!(responses[0]["result"]["query"]["total"], 1);
    assert_eq!(
        responses[0]["result"]["query"]["items"][0]["target"]["id"],
        "ledger"
    );
    assert_eq!(responses[1]["result"]["query"]["total"], 0);
    for response in &responses[2..] {
        assert_eq!(response["result"]["ok"], false);
        assert_eq!(response["result"]["error"]["code"], "INVALID_QUERY");
        assert!(response["error"].is_null());
    }
    std::fs::remove_dir_all(root).unwrap();
}

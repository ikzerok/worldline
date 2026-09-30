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

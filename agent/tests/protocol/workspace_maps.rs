use super::*;

#[test]
fn workspace_check_and_maps_list_share_a_refreshed_snapshot() {
    let root = temp_entity_project(
        "workspace-query",
        "entity lighthouse kind place as \"灯塔\"\nevent start\n  -> END\n",
    );
    let map_path = register_entity_test_map(&root);
    std::fs::write(
        map_path,
        br#"{"schema_version":1,"id":"overview","title":"Overview","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"Places","visible_default":true,"locked":false}},"placements":{"lighthouse_marker":{"layer_id":"places","annotation":"Lighthouse","role":"reference","target_ref":{"kind":"entity","id":"lighthouse"},"geometry":{"kind":"point","position":[0.2,0.3]}}}}"#,
    )
    .unwrap();
    let path = root.to_string_lossy().to_string();
    let (_, responses) = exchange(&[
        req(1, "workspace.check", json!({ "path": path.clone() })),
        req(2, "project.open", json!({ "path": path.clone() })),
        req(3, "workspace.check", json!({ "project_id": "p1" })),
        req(4, "maps.list", json!({ "path": path.clone() })),
        req(5, "maps.list", json!({ "project_id": "p1" })),
        req(6, "project.analyze", json!({ "project_id": "p1" })),
        req(7, "shutdown", json!({})),
    ]);

    for index in [0, 2, 3, 4] {
        let result = &responses[index]["result"];
        assert_eq!(result["ok"], true, "{index}: {responses:?}");
        assert_eq!(result["schema_version"], 1, "{index}: {responses:?}");
        assert_eq!(result["language_version"], "1.10", "{index}: {responses:?}");
        assert!(
            result["workspace_revision"].is_string(),
            "{index}: {responses:?}"
        );
        assert!(result["diagnostics"].is_array(), "{index}: {responses:?}");
        assert!(
            result["workspace_diagnostics"].is_array(),
            "{index}: {responses:?}"
        );
        assert_eq!(result["read_only"], false, "{index}: {responses:?}");
        assert_eq!(result["truncated"], false, "{index}: {responses:?}");
        assert!(result["continuation"].is_null(), "{index}: {responses:?}");
    }
    assert_eq!(responses[0]["result"]["stats"]["events"], 1);
    assert_eq!(responses[3]["result"]["maps"]["overview"]["id"], "overview");
    assert_eq!(
        responses[3]["result"]["references"][0]["target"],
        json!({"kind":"entity","id":"lighthouse"})
    );
    assert_eq!(
        responses[3]["result"]["references"][0]["placements"][0],
        json!({"map_id":"overview","placement_id":"lighthouse_marker"})
    );
    assert_eq!(
        responses[4]["result"]["maps"],
        responses[3]["result"]["maps"]
    );
    assert_eq!(
        responses[5]["result"]["maps"],
        responses[3]["result"]["maps"]
    );
    assert_eq!(
        responses[5]["result"]["references"],
        responses[3]["result"]["references"]
    );
}

#[test]
fn workspace_queries_keep_read_only_diagnostics_separate() {
    let root = temp_workspace(
        "query-read-only",
        r#"{"schema_version":1,"language_version":"1.10","required_features":["future.entities.v2"]}"#,
        "event start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let (_, responses) = exchange(&[
        req(1, "workspace.check", json!({ "path": path.clone() })),
        req(2, "maps.list", json!({ "path": path.clone() })),
        req(3, "project.open", json!({ "path": path })),
        req(4, "workspace.check", json!({ "project_id": "p1" })),
        req(5, "maps.list", json!({ "project_id": "p1" })),
        req(6, "shutdown", json!({})),
    ]);
    for index in [0, 1, 3, 4] {
        let result = &responses[index]["result"];
        assert_eq!(result["ok"], true, "{index}: {responses:?}");
        assert_eq!(result["read_only"], true, "{index}: {responses:?}");
        assert!(
            result["workspace_revision"].is_string(),
            "{index}: {responses:?}"
        );
        assert!(result["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(result["workspace_diagnostics"][0]["code"], "WS003");
    }
    for index in [1, 4] {
        assert!(responses[index]["result"]["maps"].is_object());
        assert!(responses[index]["result"]["references"].is_array());
    }
}

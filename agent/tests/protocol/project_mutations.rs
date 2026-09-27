use super::*;
#[test]
fn project_relation_crud_uses_core_drafts_and_preserves_baseline() {
    let root = temp_relation_project(
        "crud",
        "entity a kind place as \"甲\"\nentity b kind place as \"乙\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let (_, responses) = exchange(&[
        req(1, "project.open", json!({ "path": path })),
        req(
            2,
            "relation.type.create",
            json!({
                "project_id": "p1",
                "relation_type": {
                    "id": "knows",
                    "display": "认识",
                    "inverse_display": "被认识",
                    "direction": "directed",
                    "from_kind": "entity",
                    "to_kind": "entity"
                }
            }),
        ),
        req(
            3,
            "relation.create",
            json!({
                "project_id": "p1",
                "relation": {
                    "id": "a_knows_b",
                    "relation_type": "knows",
                    "from": {"kind": "entity", "id": "a"},
                    "to": {"kind": "entity", "id": "b"},
                    "description": "甲认识乙"
                }
            }),
        ),
        req(
            4,
            "relation.update",
            json!({
                "project_id": "p1",
                "relation": {"id": "a_knows_b", "description": "甲已经认识乙"}
            }),
        ),
        req(
            5,
            "relation.query",
            json!({"project_id":"p1","target":"entity:a"}),
        ),
        req(
            6,
            "relation.delete",
            json!({"project_id":"p1","id":"a_knows_b"}),
        ),
        req(
            7,
            "relation.type.delete",
            json!({"project_id":"p1","id":"knows"}),
        ),
        req(8, "shutdown", json!({})),
    ]);
    for response in [
        &responses[1],
        &responses[2],
        &responses[3],
        &responses[5],
        &responses[6],
    ] {
        assert_eq!(response["result"]["ok"], true, "{responses:?}");
        assert!(response["result"]["baseline"].is_string(), "{responses:?}");
    }
    assert_eq!(responses[2]["result"]["relation"]["id"], "a_knows_b");
    assert_eq!(
        responses[3]["result"]["relation"]["description"],
        "甲已经认识乙"
    );
    assert_eq!(responses[4]["result"]["edges"].as_array().unwrap().len(), 1);
    assert_eq!(responses[5]["result"]["operation"], "delete");
    assert_eq!(responses[6]["result"]["operation"], "delete");
    let source = std::fs::read_to_string(root.join("world.wl")).unwrap();
    assert!(!source.contains("relation_def"), "{source}");
    assert!(!source.contains("relation_type"), "{source}");
}

#[test]
fn project_relation_write_rejects_stale_content_baseline_without_writing() {
    let root = temp_relation_project(
        "stale",
        "entity a kind place as \"甲\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let baseline = worldline_core::project::Project::open(&root)
        .unwrap()
        .content_baseline();
    let source_before = std::fs::read(root.join("world.wl")).unwrap();
    let (_, responses) = exchange(&[
        req(1, "project.open", json!({"path":path})),
        req(
            2,
            "relation.type.create",
            json!({
                "project_id":"p1",
                "baseline":format!("{baseline}-stale"),
                "relation_type":{"id":"knows","display":"认识"}
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    assert_eq!(responses[1]["result"]["ok"], false, "{responses:?}");
    assert_eq!(responses[1]["result"]["error"]["code"], "STALE_BASELINE");
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), source_before);
}

#[test]
fn project_relation_promotion_preview_then_commit_is_explicit() {
    let root = temp_relation_project(
        "promotion",
        "character a\n  relation b as \"旧关系\"\ncharacter b\nrelation_type knows as \"认识\"\n  inverse \"被认识\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let legacy = json!({
        "source": {"kind":"character","id":"a"},
        "target": {"kind":"character","id":"b"},
        "label": "旧关系",
        "occurrence": 1
    });
    let relation = json!({
        "id": "promoted",
        "relation_type": "knows",
        "description": "旧关系",
        "source_note": "由旧人物关系提升",
        "scope_refs": [{"kind":"character","id":"b"}],
        "properties": {"weight": 3, "active": true}
    });
    let (_, responses) = exchange(&[
        req(1, "project.open", json!({ "path": path })),
        req(
            2,
            "relation.promote.preview",
            json!({"project_id":"p1","legacy":legacy,"relation":relation}),
        ),
        req(
            3,
            "relation.promote.commit",
            json!({"project_id":"p1","legacy":legacy,"relation":relation}),
        ),
        req(4, "shutdown", json!({})),
    ]);
    assert_eq!(responses[1]["result"]["ok"], true, "{responses:?}");
    assert_eq!(responses[1]["result"]["operation"], "preview");
    assert_eq!(
        responses[1]["result"]["preview"]["fingerprint_changed"],
        true
    );
    assert!(responses[1]["result"]["preview"]["content_baseline"].is_string());
    assert!(responses[1]["result"]["preview"]["draft"].is_object());
    assert_eq!(
        responses[1]["result"]["preview"]["draft"]["scope_refs"],
        json!([{"kind":"character","id":"b"}])
    );
    assert_eq!(
        responses[1]["result"]["preview"]["draft"]["properties"],
        json!([["active", true], ["weight", 3.0]])
    );
    assert_eq!(responses[2]["result"]["ok"], true, "{responses:?}");
    assert_eq!(responses[2]["result"]["operation"], "commit");
    let source = std::fs::read_to_string(root.join("world.wl")).unwrap();
    assert!(source.contains("relation_def promoted"), "{source}");
    assert!(!source.contains("relation b as"), "{source}");
    assert!(source.contains("scope character b"), "{source}");
    assert!(source.contains("property active = true"), "{source}");
    assert!(source.contains("property weight = 3"), "{source}");
}

#[test]
fn project_relation_promotion_commit_accepts_core_preview_payload() {
    let root = temp_relation_project(
        "promotion-payload",
        "character a\n  relation b as \"旧关系\"\ncharacter b\nrelation_type knows as \"认识\"\nevent start\n  -> END\n",
    );
    let path = root.to_string_lossy().to_string();
    let (_, first) = exchange(&[
        req(1, "project.open", json!({ "path": path.clone() })),
        req(
            2,
            "relation.promote.preview",
            json!({
                "project_id":"p1",
                "legacy": {"source":"character:a","target":"character:b","label":"旧关系","occurrence":1},
                "relation": {"id":"promoted","relation_type":"knows"}
            }),
        ),
    ]);
    let preview = first[1]["result"]["preview"].clone();
    let (_, second) = exchange(&[
        req(
            1,
            "relation.promote.commit",
            json!({"path":path,"preview":preview}),
        ),
        req(2, "shutdown", json!({})),
    ]);
    assert_eq!(second[0]["result"]["ok"], true, "{second:?}");
    assert_eq!(second[0]["result"]["operation"], "commit");
}

#[test]
fn project_read_only_workspace_diagnostics_are_separate_and_repeatable() {
    let unknown_language = temp_workspace(
        "unknown-language",
        r#"{"schema_version":1,"language_version":"2.0","required_features":[]}"#,
        "event start\n  -> END\n",
    );
    let unknown_feature = temp_workspace(
        "unknown-feature",
        r#"{"schema_version":1,"language_version":"1.10","required_features":["future.entities.v2"]}"#,
        "event start\n  -> END\n",
    );
    let (_, responses) = exchange(&[
        req(
            1,
            "project.open",
            json!({ "path": unknown_language.to_string_lossy() }),
        ),
        req(
            2,
            "project.open",
            json!({ "path": unknown_feature.to_string_lossy() }),
        ),
        req(3, "project.analyze", json!({ "project_id": "p1" })),
        req(4, "project.analyze", json!({ "project_id": "p2" })),
        req(
            5,
            "compile",
            json!({ "path": unknown_language.to_string_lossy() }),
        ),
        req(
            6,
            "compile",
            json!({ "path": unknown_feature.to_string_lossy() }),
        ),
        req(7, "shutdown", json!({})),
    ]);
    for (index, response) in [0, 1, 2, 3].map(|index| (index, &responses[index])) {
        let result = &response["result"];
        assert_eq!(result["ok"], true, "{index}: {responses:?}");
        assert_eq!(result["read_only"], true, "{index}: {responses:?}");
        assert!(result["project_id"].is_string() || index >= 2);
        assert!(result["catalog"].is_object(), "{index}: {responses:?}");
        assert!(result["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(result["workspace_diagnostics"][0]["code"], "WS003");
    }
    let compile = &responses[4]["result"];
    assert_eq!(compile["ok"], true);
    assert_eq!(compile["read_only"], true);
    assert_eq!(compile["workspace_diagnostics"][0]["code"], "WS003");
    let compile_feature = &responses[5]["result"];
    assert_eq!(compile_feature["ok"], true);
    assert_eq!(compile_feature["read_only"], true);
    assert_eq!(compile_feature["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(compile_feature["workspace_diagnostics"][0]["code"], "WS003");
}

#[test]
fn project_entity_crud_returns_baseline_and_rejects_stale_write() {
    let root = temp_entity_project("crud", "");
    let path = root.to_string_lossy().to_string();
    let (_, opened) = exchange(&[
        req(1, "project.open", json!({ "path": path.clone() })),
        req(2, "shutdown", json!({})),
    ]);
    assert_eq!(opened[0]["result"]["ok"], true);
    assert_eq!(opened[0]["result"]["language_version"], "1.10");
    let baseline = opened[0]["result"]["baseline"]
        .as_str()
        .unwrap()
        .to_string();

    let (_, responses) = exchange(&[
        req(1, "project.open", json!({ "path": path.clone() })),
        req(
            2,
            "entity.create",
            json!({
                "project_id": "p1",
                "baseline": baseline,
                "entity": {
                    "id": "lighthouse",
                    "entity_type": "place",
                    "display": "雾港灯塔",
                    "description": "静态资料",
                    "properties": { "height": 38, "lit": true }
                }
            }),
        ),
        req(3, "shutdown", json!({})),
    ]);
    assert_eq!(responses[1]["result"]["ok"], true, "{responses:?}");
    let baseline = responses[1]["result"]["baseline"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(std::fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("entity lighthouse kind place"));

    let (_, responses) = exchange(&[
        req(1, "project.open", json!({ "path": path })),
        req(
            2,
            "entity.update",
            json!({
                "project_id": "p1",
                "baseline": "stale",
                "entity": { "id": "lighthouse", "display": "不应写入" }
            }),
        ),
        req(
            3,
            "entity.delete",
            json!({
                "project_id": "p1",
                "baseline": baseline,
                "id": "lighthouse"
            }),
        ),
        req(4, "shutdown", json!({})),
    ]);
    assert_eq!(responses[1]["result"]["ok"], false);
    assert_eq!(responses[1]["result"]["error"]["code"], "STALE_BASELINE");
    assert_eq!(responses[2]["result"]["ok"], true, "{responses:?}");
    assert!(!std::fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("entity lighthouse"));
}

#[test]
fn project_entity_map_change_rejects_stale_baseline_before_write() {
    let root = temp_entity_project(
        "map-baseline",
        "entity lighthouse kind place as \"灯塔\"\nevent start\n  -> END\n",
    );
    let map_path = register_entity_test_map(&root);
    let path = root.to_string_lossy().to_string();
    let baseline = worldline_core::project::Project::open(&root)
        .unwrap()
        .content_baseline();
    let source_before = std::fs::read(root.join("world.wl")).unwrap();
    let replacement = r#"{"schema_version":1,"id":"overview","title":"Overview","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"Places","visible_default":true,"locked":false}},"placements":{"lighthouse_marker":{"layer_id":"places","annotation":"Lighthouse","role":"reference","target_ref":{"kind":"entity","id":"lighthouse"},"geometry":{"kind":"point","position":[0.2,0.3]}}}}"#;
    let lines = vec![
        req(1, "project.open", json!({ "path": path.clone() })),
        req(2, "project.analyze", json!({ "project_id": "p1" })),
        req(
            3,
            "entity.delete",
            json!({
                "project_id": "p1",
                "baseline": baseline,
                "id": "lighthouse"
            }),
        ),
        req(4, "shutdown", json!({})),
    ];
    let mut input = MapMutatingReader::new(&lines, &map_path, replacement.as_bytes());
    let mut output = Vec::new();
    let code = worldline_agent::run(&mut input, &mut output);
    let responses: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(code, 0);
    assert_eq!(responses[0]["result"]["ok"], true);
    assert_eq!(responses[1]["result"]["ok"], true);
    assert_ne!(responses[1]["result"]["baseline"], baseline);
    assert_eq!(responses[2]["result"]["ok"], false);
    assert_eq!(responses[2]["result"]["error"]["code"], "STALE_BASELINE");
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), source_before);
}

#[test]
fn analyze_catalog_preserves_targets_and_source_locations() {
    let source = "tag coordinate as \"坐标\"\ntag harbor as \"港口\"\nmark tag harbor with coordinate\nmark event start with harbor\nevent start\n  -> END\n";
    let (code, resp) = exchange(&[
        req(
            1,
            "compile",
            json!({"source": source, "file_name": "catalog.wl"}),
        ),
        req(2, "analyze", json!({"story_id": "s1"})),
        req(3, "session.open", json!({"story_id": "s1"})),
        req(4, "session.continue", json!({"session_id": "c1"})),
        req(5, "analyze", json!({"story_id": "s1"})),
        req(6, "shutdown", json!({})),
    ]);
    assert_eq!(code, 0);
    assert_eq!(resp.len(), 6);
    for (index, response) in resp.iter().enumerate() {
        assert_eq!(response["id"], index + 1);
        assert!(response.get("error").is_none(), "{response}");
    }
    assert_eq!(resp[0]["result"]["ok"], true);
    let analyzed = &resp[1]["result"];
    for field in ["graph", "anchors", "symbols", "stats", "world", "timeline"] {
        assert!(analyzed.get(field).is_some(), "原字段 {field} 继续保留");
    }
    let catalog = &analyzed["catalog"];
    assert_eq!(catalog["tags"].as_object().unwrap().len(), 2);
    assert!(catalog["tags"]["coordinate"].is_object());
    assert!(catalog["tags"]["harbor"].is_object());
    assert_eq!(catalog["assets"], json!({}));
    assert_eq!(catalog["attachments"], json!([]));
    assert_eq!(catalog["marks"].as_array().unwrap().len(), 2);
    let objects = catalog["objects"].as_array().expect("目录对象数组");
    for (kind, id, line) in [("tag", "harbor", 2), ("event", "start", 5)] {
        let object = objects
            .iter()
            .find(|o| o["target"] == json!({"kind": kind, "id": id}))
            .expect("带稳定 ID 的对象");
        assert_eq!(object["line"], line);
        assert!(object["file"].as_str().unwrap().ends_with("catalog.wl"));
    }
    assert_eq!(resp[3]["result"]["ended"], true);
    assert_eq!(resp[4]["result"]["catalog"], *catalog, "播放不会改变目录");
}

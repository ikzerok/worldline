use super::*;
#[test]
fn authoring_intent_rpc_preview_and_apply_are_atomic() {
    let source =
        "entity lighthouse kind place as \"灯塔😀\"\nevent start\n  你看见灯塔😀。\n  -> END\n";
    let root = temp_authoring_intent_project("rpc", source);
    let root_path = root.to_string_lossy().to_string();
    let map_path = root.join(".world/maps/overview.json");
    let project = worldline_core::project::Project::open(&root).unwrap();
    let source_path = project.entry.clone();
    let baseline = project.content_baseline();
    let expected_text = "灯塔😀";
    let start = source.rfind(expected_text).unwrap();
    let intent = json!({
        "expected_baseline": baseline,
        "target": {
            "kind": "create_entity",
            "value": {
                "path": source_path,
                "draft": {
                    "id": "tower",
                    "entity_type": "place",
                    "display": expected_text,
                    "description": "正文与地图共同引用",
                    "properties": []
                }
            }
        },
        "selection": {
            "path": source_path,
            "start": start,
            "end": start + expected_text.len(),
            "expected_text": expected_text
        },
        "placement": {
            "map_id": "overview",
            "placement_id": "tower_marker",
            "layer_id": "places",
            "geometry": {"kind": "point", "position": [0.4, 0.5]},
            "annotation": "新建入口",
            "role": "reference",
            "label_override": null
        }
    });
    let original_map = std::fs::read(&map_path).unwrap();
    let mut stale_intent = intent.clone();
    stale_intent["expected_baseline"] = json!("stale-baseline");
    let (_, preview_responses) = exchange(&[
        req(1, "project.open", json!({"path":root_path.clone()})),
        req(
            2,
            "authoring.intent.preview",
            json!({"project_id":"p1", "intent":stale_intent}),
        ),
        req(
            3,
            "authoring.intent.preview",
            json!({"project_id":"p1", "intent":intent.clone()}),
        ),
        req(4, "shutdown", json!({})),
    ]);
    let stale = &preview_responses[1]["result"];
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"]["code"], "STALE_BASELINE");
    let preview = &preview_responses[2]["result"];
    assert_eq!(preview["ok"], true, "{preview:?}");
    assert_eq!(preview["operation"], "preview");
    assert_eq!(preview["target"], json!({"kind":"entity","id":"tower"}));
    assert_eq!(
        preview["reference_impact"]["map_placements"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(std::fs::read_to_string(&source_path).unwrap(), source);
    assert_eq!(std::fs::read(&map_path).unwrap(), original_map);

    let mut invalid_intent = intent.clone();
    invalid_intent["placement"]["layer_id"] = json!("missing");
    let (_, apply_responses) = exchange(&[
        req(1, "project.open", json!({"path":root_path})),
        req(
            2,
            "authoring.intent.apply",
            json!({"project_id":"p1", "intent":invalid_intent}),
        ),
        req(
            3,
            "authoring.intent.apply",
            json!({"project_id":"p1", "intent":intent}),
        ),
        req(4, "shutdown", json!({})),
    ]);
    let failed = &apply_responses[1]["result"];
    assert_eq!(failed["ok"], false, "{failed:?}");
    assert_eq!(failed["error"]["code"], "INTENT_REJECTED");
    let applied = &apply_responses[2]["result"];
    assert_eq!(applied["ok"], true, "{applied:?}");
    assert_eq!(applied["operation"], "apply");
    assert_eq!(applied["changed_files"].as_array().unwrap().len(), 2);
    assert!(std::fs::read_to_string(&source_path)
        .unwrap()
        .contains("[[entity:tower|灯塔😀]]"));
    let saved_map: Value = serde_json::from_slice(&std::fs::read(&map_path).unwrap()).unwrap();
    assert_eq!(saved_map["extension"], json!({"preserve":true}));
    assert_eq!(
        saved_map["placements"]["tower_marker"]["target_ref"],
        json!({"kind":"entity","id":"tower"})
    );
}

#[test]
fn markdown_import_rpc_reviews_then_applies_with_explicit_confirmation_to_project_session() {
    let project_root = temp_entity_project("markdown-import-rpc", "event start\n  -> END\n");
    let source = temp_markdown_import_source("review-apply");
    let baseline = worldline_core::project::Project::open(&project_root)
        .unwrap()
        .content_baseline();
    let request = worldline_core::markdown_import::MarkdownImportRequest {
        source_root: source.clone(),
        expected_baseline: baseline.clone(),
        id_overrides: Default::default(),
        namespace: None,
        accept_losses: false,
        allow_language_upgrade: false,
    };
    let digest = worldline_core::project::Project::open(&project_root)
        .unwrap()
        .preview_markdown_import(&request)
        .unwrap()
        .plan_digest;
    let path = project_root.to_string_lossy().to_string();
    let source_path = source.to_string_lossy().to_string();

    let (_, responses) = exchange(&[
        req(1, "project.open", json!({"path":path})),
        req(
            2,
            "markdown.import.preview",
            json!({"project_id":"p1", "source":source_path, "baseline":baseline}),
        ),
        req(
            3,
            "markdown.import.apply",
            json!({
                "project_id":"p1",
                "source":source.to_string_lossy(),
                "baseline":baseline,
                "plan_digest":digest,
                "accept_losses":false,
                "allow_language_upgrade":false
            }),
        ),
        req(
            4,
            "markdown.import.apply",
            json!({
                "project_id":"p1",
                "source":source.to_string_lossy(),
                "baseline":baseline,
                "plan_digest":digest,
                "accept_losses":true,
                "allow_language_upgrade":false
            }),
        ),
        req(5, "project.analyze", json!({"project_id":"p1"})),
        req(6, "shutdown", json!({})),
    ]);

    let opened = &responses[0]["result"];
    assert_eq!(opened["project_id"], "p1");
    assert_eq!(responses[1]["result"]["plan"]["plan_digest"], digest);
    assert_eq!(responses[1]["result"]["plan"]["can_apply"], false);
    assert_eq!(responses[2]["result"]["ok"], false);
    assert_eq!(
        responses[2]["result"]["error"]["code"],
        "CONFIRMATION_REQUIRED"
    );
    assert_eq!(responses[3]["result"]["ok"], true, "{:?}", responses[3]);
    assert_eq!(responses[3]["result"]["operation"], "apply");
    assert_eq!(responses[3]["result"]["plan"]["can_apply"], true);
    assert_eq!(responses[3]["result"]["baseline"], baseline);
    assert_ne!(responses[3]["result"]["new_baseline"], baseline);
    assert!(responses[4]["result"]["catalog"]["entities"]["harbor"].is_object());
    assert!(project_root.join(".world/markdown-imports").exists());
    let _ = std::fs::remove_dir_all(&project_root);
    let _ = std::fs::remove_dir_all(&source);
}

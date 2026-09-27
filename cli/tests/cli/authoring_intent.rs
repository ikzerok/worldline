use super::common::*;
use serde_json::{json, Value};

#[test]
fn authoring_intent_cli_preview_is_read_only_and_apply_is_all_or_nothing() {
    let root = temp_presentation_project("authoring-intent-cli");
    let source =
        "entity lighthouse kind place as \"灯塔😀\"\nevent start\n  你看见灯塔😀。\n  -> END\n";
    std::fs::write(root.join("world.wl"), source).unwrap();
    let map_path = root.join(".world/maps/overview.json");
    let mut map: Value = serde_json::from_slice(&std::fs::read(&map_path).unwrap()).unwrap();
    map["extension"] = json!({"retain": "未来地图数据"});
    std::fs::write(&map_path, serde_json::to_vec(&map).unwrap()).unwrap();
    let project = worldline_core::project::Project::open(&root).unwrap();
    let baseline = project.content_baseline();
    let expected_text = "灯塔😀";
    let start = source.rfind(expected_text).unwrap();
    let target_path = project.entry.clone();
    let intent = json!({
        "expected_baseline": baseline,
        "target": {
            "kind": "create_entity",
            "value": {
                "path": target_path,
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
            "path": target_path,
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
    let intent_json = intent.to_string();
    let original_map = std::fs::read(&map_path).unwrap();
    let mut stale_intent = intent.clone();
    stale_intent["expected_baseline"] = json!("stale-baseline");
    let mut args = vec![
        "authoring-intent".to_string(),
        "preview".into(),
        root.to_string_lossy().into_owned(),
        "--intent-json".into(),
        stale_intent.to_string(),
        "--json".into(),
    ];
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 1);
    let stale = json_lines(&out).remove(0);
    assert_eq!(stale["error"]["code"], "STALE_BASELINE");
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        source
    );

    args[4] = intent_json.clone();
    out.clear();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&out));
    let preview = json_lines(&out).remove(0);
    assert_eq!(preview["ok"], true);
    assert_eq!(preview["operation"], "preview");
    assert_eq!(preview["target"], json!({"kind":"entity","id":"tower"}));
    assert_eq!(
        preview["reference_impact"]["content_references"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        preview["reference_impact"]["map_placements"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(preview["baseline"], baseline);
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        source,
        "预览不能写源码"
    );
    assert_eq!(
        std::fs::read(&map_path).unwrap(),
        original_map,
        "预览不能写地图"
    );

    let mut invalid_intent = intent.clone();
    invalid_intent["placement"]["layer_id"] = json!("missing");
    args[1] = "apply".into();
    args[4] = invalid_intent.to_string();
    out.clear();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 1);
    let failed = json_lines(&out).remove(0);
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["error"]["code"], "INTENT_REJECTED");
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        source
    );
    assert_eq!(std::fs::read(&map_path).unwrap(), original_map);

    args[4] = intent_json;
    out.clear();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::<u8>::new())).unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&out));
    let applied = json_lines(&out).remove(0);
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["operation"], "apply");
    assert_eq!(applied["changed_files"].as_array().unwrap().len(), 2);
    let saved_source = std::fs::read_to_string(root.join("world.wl")).unwrap();
    assert!(saved_source.contains("[[entity:tower|灯塔😀]]"));
    let saved_map: Value = serde_json::from_slice(&std::fs::read(&map_path).unwrap()).unwrap();
    assert_eq!(saved_map["extension"], json!({"retain":"未来地图数据"}));
    assert_eq!(
        saved_map["placements"]["tower_marker"]["target_ref"],
        json!({"kind":"entity","id":"tower"})
    );
}

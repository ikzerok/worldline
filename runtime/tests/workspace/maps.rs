use super::*;

#[test]
fn project_reads_maps_and_indexes_object_placements_without_changing_program() {
    let temp = Temp::new();
    let root = temp.0.join("maps");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "tag lighthouse as \"灯塔\"\nasset base image \"base.png\" as \"底图\"\n",
    )
    .unwrap();
    fs::write(root.join("base.png"), b"placeholder").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{"harbor":".world/maps/harbor.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/harbor.json"),
        r#"{
          "schema_version":1,
          "id":"harbor",
          "title":"雾港",
          "raster_layers":[{"id":"base","asset":{"kind":"asset","id":"base"},"rect":[0,0,1,1]}],
          "canvas":{"width":800,"height":600,"unit":"normalized"},
          "layer_order":["places"],
          "layers":{"places":{"title":"地点","visible_default":true,"locked":false}},
          "placements":{"lighthouse":{"layer_id":"places","target_ref":{"kind":"tag","id":"lighthouse"},"geometry":{"kind":"point","position":[0.5,0.25]},"annotation":"入口","role":"地点"}},
          "future_optional":{"kept":true}
        }"#,
    )
    .unwrap();

    let mut project = Project::open(&root).unwrap();
    let fingerprint = project.compile().analysis.fingerprint;
    let index = project.map_index();
    assert!(
        index.diagnostics.is_empty(),
        "地图应合法: {:?}",
        index.diagnostics
    );
    assert_eq!(index.maps["harbor"].canvas.width, 800);
    assert_eq!(
        index.maps["harbor"].extra["future_optional"]["kept"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(
        index.placements_for(&TargetRef::new("tag", "lighthouse")),
        vec![worldline_core::presentation::MapPlacementRef {
            map_id: "harbor".into(),
            placement_id: "lighthouse".into(),
        }]
    );
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
}

#[test]
fn zero_area_raster_rectangles_are_rejected() {
    let temp = Temp::new();
    let root = temp.0.join("raster-rect");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "// content only\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    for rect in [[0, 0, 0, 1], [0, 0, 1, 0]] {
        let map = serde_json::json!({
            "schema_version": 1, "id": "map", "title": "Map",
            "canvas": {"width": 10, "height": 10, "unit": "normalized"},
            "layer_order": [], "layers": {}, "placements": {},
            "raster_layers": [{"id": "base", "asset": {"kind": "asset", "id": "image"}, "rect": rect}]
        });
        let bytes = serde_json::to_vec(&map).unwrap();
        fs::write(root.join(".world/maps/map.json"), &bytes).unwrap();
        let project = Project::open(&root).unwrap();
        let index = project.map_index();
        assert!(
            index.diagnostics.iter().any(|d| d.code == "MAP008"),
            "{rect:?}"
        );
        assert!(index.maps.is_empty());
        assert_eq!(
            project
                .authoring_document(&project.root.join(".world/maps/map.json"))
                .unwrap()
                .bytes(),
            bytes
        );
    }
}

#[test]
fn serialized_map_source_preserves_unknown_fields_and_required_features() {
    let temp = Temp::new();
    let root = temp.0.join("map-source");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "tag harbor as \"雾港\"\nasset base image \"base.png\" as \"底图\"\n",
    )
    .unwrap();
    fs::write(root.join("base.png"), b"placeholder").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    let map = serde_json::json!({
        "schema_version": 1,
        "required_features": ["presentation.maps.v1"],
        "source": {"author_extension": true},
        "id": "map",
        "title": "Map",
        "raster_layers": [{
            "id": "base",
            "asset": {"kind": "asset", "id": "base", "future_asset": {"kept": true}},
            "rect": [0, 0, 1, 1]
        }],
        "canvas": {"width": 10, "height": 10, "unit": "normalized"},
        "layer_order": ["notes"],
        "layers": {"notes": {"title": "Notes", "visible_default": true, "locked": false}},
        "placements": {"note": {
            "layer_id": "notes",
            "target_ref": {"kind": "tag", "id": "harbor", "future_target": {"kept": true}},
            "geometry": {"kind": "point", "position": [0.5, 0.5], "future_geometry": {"kept": true}},
            "annotation": "source",
            "role": "note"
        }}
    });
    fs::write(
        root.join(".world/maps/map.json"),
        serde_json::to_vec(&map).unwrap(),
    )
    .unwrap();

    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(
        index.diagnostics.is_empty(),
        "地图应合法: {:?}",
        index.diagnostics
    );
    let serialized = serde_json::to_value(&index).unwrap();
    assert_eq!(serialized["maps"]["map"]["source"], map);
}

#[test]
fn pure_content_project_can_save_and_export_but_play_reports_missing_entry() {
    let temp = Temp::new();
    let root = temp.0.join("content-only");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("world.wl"), "tag lighthouse as \"灯塔\"\n").unwrap();

    let mut project = Project::open(&root).unwrap();
    let result = project.compile();
    assert!(result.program.events.is_empty());
    assert!(!result.has_errors());
    project.save().unwrap();
    let package = project.export_files().unwrap();
    assert_eq!(
        package[&PathBuf::from("world.wl")],
        "tag lighthouse as \"灯塔\"\n".as_bytes()
    );

    let run_error = match Story::new(&result.program, &result.analysis) {
        Ok(_) => panic!("纯内容工程不应启动试玩"),
        Err(error) => error,
    };
    assert!(run_error.to_string().contains("没有可运行入口"));
}

#[test]
fn malformed_map_isolated_from_valid_map_and_keeps_original_bytes() {
    let temp = Temp::new();
    let root = temp.0.join("map-isolation");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor as \"雾港\"\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"maps":{"good":".world/maps/good.json","bad":".world/maps/bad.json"}}"#,
    )
    .unwrap();
    let good = r#"{
      "schema_version":1,"id":"good","title":"合法地图",
      "canvas":{"width":10,"height":10,"unit":"normalized"},
      "layer_order":[],"layers":{},"placements":{}
    }"#;
    let bad = br#"{"schema_version":1,"id":"bad","title":"bad","schema_version":1}"#;
    fs::write(root.join(".world/maps/good.json"), good).unwrap();
    fs::write(root.join(".world/maps/bad.json"), bad).unwrap();

    let mut project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(index.maps.contains_key("good"));
    assert!(!index.maps.contains_key("bad"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP001"));
    assert_eq!(
        project
            .authoring_document(&root.join(".world/maps/bad.json"))
            .unwrap()
            .bytes(),
        bad
    );
    assert!(project
        .compile()
        .analysis
        .catalog
        .tags
        .contains_key("harbor"));
}

#[test]
fn map_query_reports_manifest_errors_without_polluting_language_diagnostics() {
    let temp = Temp::new();
    let root = temp.0.join("invalid-registry");
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":[]}"#,
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(project
        .map_index()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "WS004"));
    assert!(!project.compile().has_errors());
}

#[test]
fn raster_layers_requires_an_array_instead_of_silent_legacy_coercion() {
    let temp = Temp::new();
    let root = temp.0.join("raster-format");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/map.json"),
        br#"{
        "schema_version":1,"id":"map","title":"Map",
        "canvas":{"width":10,"height":10,"unit":"normalized"},
        "layers":{},"layer_order":[],"placements":{},
        "raster_layers":{"id":"base","asset":{"kind":"asset","id":"image"},"rect":[0,0,1,1]}
    }"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(!index.maps.contains_key("map"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP008"));
}

#[test]
fn non_image_asset_is_isolated_without_hiding_the_map() {
    let temp = Temp::new();
    let root = temp.0.join("raster-type");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(
        root.join("world.wl"),
        "asset sound audio \"sound.wav\" as \"声音\"\n",
    )
    .unwrap();
    fs::write(root.join("sound.wav"), b"placeholder").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/map.json"),
        br#"{
        "schema_version":1,"id":"map","title":"Map",
        "canvas":{"width":10,"height":10,"unit":"normalized"},
        "layers":{},"layer_order":[],"placements":{},
        "raster_layers":[{"id":"base","asset":{"kind":"asset","id":"sound"},"rect":[0,0,1,1]}]
    }"#,
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(!project.compile().has_errors());
    let index = project.map_index();
    assert!(index.maps["map"].raster_layers[0].asset_info.is_none());
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP010"));
}

#[test]
fn rejected_empty_map_titles_and_roles_always_have_diagnostics() {
    let temp = Temp::new();
    let root = temp.0.join("map-errors");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    let valid = serde_json::json!({"schema_version":1,"id":"map","title":"Map",
        "canvas":{"width":10,"height":10,"unit":"normalized"},
        "layers":{"notes":{"title":"Notes","visible_default":true,"locked":false}},
        "layer_order":["notes"],"placements":{"note":{"layer_id":"notes","target_ref":null,
            "geometry":{"kind":"point","position":[0.5,0.5]},"annotation":"","role":"note"}}});
    for pointer in ["/title", "/placements/note/role"] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = serde_json::json!("");
        fs::write(
            root.join(".world/maps/map.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        let project = Project::open(&root).unwrap();
        let index = project.map_index();
        assert!(index.maps.is_empty());
        assert!(
            index
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == worldline_core::Severity::Error),
            "缺少{pointer}错误原因"
        );
    }
}

#[test]
fn map_geometry_rejects_out_of_range_and_self_intersecting_shapes() {
    let temp = Temp::new();
    let root = temp.0.join("map-geometry");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"maps":{"valid":".world/maps/valid.json","invalid":".world/maps/invalid.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/valid.json"),
        r#"{
          "schema_version":1,"id":"valid","title":"valid",
          "canvas":{"width":10,"height":10,"unit":"normalized"},
          "layer_order":["places"],
          "layers":{"places":{"title":"places","visible_default":true,"locked":false}},
          "placements":{"concave":{"layer_id":"places","target_ref":null,"geometry":{"kind":"polygon","points":[[0.1,0.1],[0.9,0.1],[0.9,0.4],[0.5,0.4],[0.5,0.9],[0.1,0.9]]},"annotation":"shape","role":"note"}}
        }"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/invalid.json"),
        r#"{
          "schema_version":1,"id":"invalid","title":"invalid",
          "canvas":{"width":10,"height":10,"unit":"normalized"},
          "layer_order":["places"],
          "layers":{"places":{"title":"places","visible_default":true,"locked":false}},
          "placements":{
            "cross":{"layer_id":"places","target_ref":null,"geometry":{"kind":"polygon","points":[[0.1,0.1],[0.9,0.9],[0.1,0.9],[0.9,0.1]]},"annotation":"shape","role":"note"},
            "out_of_range":{"layer_id":"places","target_ref":null,"geometry":{"kind":"point","position":[1.1,0.5]},"annotation":"point","role":"note"}
          }
        }"#,
    )
    .unwrap();

    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(index.maps.contains_key("valid"));
    assert!(!index.maps.contains_key("invalid"));
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAP006"));
}

#[test]
fn map_query_reports_unresolved_scope_refs_without_hiding_the_map() {
    let temp = Temp::new();
    let root = temp.0.join("map-scope-ref");
    fs::create_dir_all(root.join(".world/maps")).unwrap();
    fs::write(root.join("world.wl"), "tag harbor\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"maps":{"map":".world/maps/map.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/maps/map.json"),
        br#"{
          "schema_version":1,
          "id":"map",
          "title":"Map",
          "canvas":{"width":10,"height":10,"unit":"normalized"},
          "layer_order":["notes"],
          "layers":{"notes":{"title":"Notes","visible_default":true,"locked":false}},
          "placements":{"note":{
            "layer_id":"notes",
            "target_ref":null,
            "geometry":{"kind":"point","position":[0.5,0.5]},
            "annotation":"scope",
            "role":"note",
            "scope_refs":[{"kind":"period","id":"missing_period"}]
          }}
        }"#,
    )
    .unwrap();

    let project = Project::open(&root).unwrap();
    let index = project.map_index();
    assert!(index.maps.contains_key("map"));
    let unresolved = index
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "MAP011")
        .collect::<Vec<_>>();
    assert_eq!(unresolved.len(), 1);
    assert!(unresolved[0].message.contains("scope_refs"));
}

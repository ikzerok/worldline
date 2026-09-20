use std::fs;
use worldline_core::project::Project;

#[test]
fn references_to_scenes_prevent_deleting_their_parent_event() {
    let root = std::env::temp_dir().join(format!("worldline-scene-impact-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "event note\n  scene inner\n    -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    project
        .create_authoring_document(
            &project.root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"overview":".world/maps/overview.json"}}"#.to_vec(),
        )
        .unwrap();
    let map_path = project.root.join(".world/maps/overview.json");
    project.create_authoring_document(&map_path, br#"{"schema_version":1,"id":"overview","title":"Overview","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"Places","visible_default":true,"locked":false}},"placements":{"scene_marker":{"layer_id":"places","annotation":"Scene","role":"reference","target_ref":{"kind":"scene","id":"note.inner"},"geometry":{"kind":"point","position":[0.2,0.3]}}}}"#.to_vec()).unwrap();
    let before = project.sources();
    let impact = project.deletion_impact(&worldline_core::catalog::TargetRef::new("event", "note"));
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete(), "事件删除也会移除被引用的场景");
    assert_eq!(impact.map_placements.len(), 1);
    assert!(project
        .remove_event("note")
        .unwrap_err()
        .contains("scene_marker"));
    assert_eq!(project.sources(), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn background_asset_remains_referenced_without_any_markers() {
    let root = std::env::temp_dir().join(format!("worldline-raster-impact-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "asset base image \"base.png\" as \"底图\"\n",
    )
    .unwrap();
    fs::write(root.join("base.png"), b"preserved image bytes").unwrap();
    let mut project = Project::open(&root.join("world.wl")).unwrap();
    project
        .create_authoring_document(
            &project.root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"overview":".world/maps/overview.json"}}"#.to_vec(),
        )
        .unwrap();
    project.create_authoring_document(&project.root.join(".world/maps/overview.json"), br#"{"schema_version":1,"id":"overview","title":"Overview","canvas":{"width":100,"height":100,"unit":"normalized"},"raster_layers":[{"id":"background","asset":{"kind":"asset","id":"base"},"rect":[0,0,1,1]}],"layer_order":[],"layers":{},"placements":{}}"#.to_vec()).unwrap();
    let impact = project.deletion_impact(&worldline_core::catalog::TargetRef::new("asset", "base"));
    assert!(impact.target_exists);
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete(), "底图素材仍在使用");
    assert_eq!(impact.map_rasters.len(), 1);
    assert_eq!(impact.map_rasters[0].map_id, "overview");
    assert_eq!(impact.map_rasters[0].raster_layer_id, "background");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scope_only_map_reference_prevents_event_deletion() {
    let root = std::env::temp_dir().join(format!("worldline-scope-impact-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "event note as \"资料\"\n  正文。\n  -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root.join("world.wl")).unwrap();
    project
        .create_authoring_document(
            &project.root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"overview":".world/maps/overview.json"}}"#.to_vec(),
        )
        .unwrap();
    let map_path = project.root.join(".world/maps/overview.json");
    project.create_authoring_document(&map_path, br#"{"schema_version":1,"id":"overview","title":"Overview","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"Places","visible_default":true,"locked":false}},"placements":{"scope_marker":{"layer_id":"places","annotation":"Scope","role":"reference","target_ref":null,"scope_refs":[{"kind":"event","id":"note"}],"geometry":{"kind":"point","position":[0.2,0.3]}}}}"#.to_vec()).unwrap();
    let before = project.sources();
    let impact = project.deletion_impact(&worldline_core::catalog::TargetRef::new("event", "note"));
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete(), "作用域引用仍依赖该事件");
    assert_eq!(impact.map_scopes.len(), 1);
    assert_eq!(impact.map_scopes[0].placement_id, "scope_marker");
    let error = project.remove_event("note").unwrap_err();
    assert!(error.contains("scope_marker"));
    assert_eq!(project.sources(), before);
    let mut map: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&map_path).unwrap().bytes()).unwrap();
    map["placements"]["scope_marker"]["scope_refs"] = serde_json::json!([]);
    project
        .set_authoring_document(&map_path, serde_json::to_vec(&map).unwrap())
        .unwrap();
    project.remove_event("note").unwrap();
    assert!(project.map_index().maps["overview"]
        .placements
        .contains_key("scope_marker"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deletion_impact_lists_existing_content_sources_without_changing_the_project() {
    let root =
        std::env::temp_dir().join(format!("worldline-content-impact-{}", std::process::id()));
    let mut project = Project::new(&root);
    let before = project.sources();
    let target = worldline_core::catalog::TargetRef::new("event", "beacon");
    let impact = project.deletion_impact(&target);
    assert!(impact.target_exists);
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(impact.map_placements.is_empty());
    assert!(!impact.can_delete());
    assert!(impact.content_references.iter().any(|reference| {
        reference.source == worldline_core::catalog::TargetRef::new("event", "arrival")
            && reference.kind == "叙事连接"
            && reference.target == target
    }));
    assert!(impact.content_references.iter().any(|reference| {
        reference.source == worldline_core::catalog::TargetRef::new("event", "farewell")
            && reference.kind == "先后约束"
            && reference.target == target
    }));
    assert!(project.remove_event("beacon").is_err());
    assert_eq!(project.sources(), before);
    let absent =
        project.deletion_impact(&worldline_core::catalog::TargetRef::new("event", "missing"));
    assert!(!absent.target_exists);
    assert!(!absent.can_delete());
}

#[test]
fn deleting_an_event_with_map_placements_is_rejected_without_mutation() {
    let root = std::env::temp_dir().join(format!(
        "worldline-event-map-reference-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "world notes as \"资料\"\nevent note as \"独立资料\"\n  正文。\n  -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root.join("world.wl")).unwrap();
    project
        .create_authoring_document(
            &project.root.join(".world/project.json"),
            br#"{"schema_version":1,"maps":{"overview":".world/maps/overview.json"}}"#.to_vec(),
        )
        .unwrap();
    let map_path = project.root.join(".world/maps/overview.json");
    project.create_authoring_document(&map_path, br#"{"schema_version":1,"id":"overview","title":"Overview","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"Places","visible_default":true,"locked":false}},"placements":{"note_marker":{"layer_id":"places","annotation":"Note","role":"reference","target_ref":{"kind":"event","id":"note"},"geometry":{"kind":"point","position":[0.2,0.3]}}}}"#.to_vec()).unwrap();
    let before_sources = project.sources();
    let impact = project.deletion_impact(&worldline_core::catalog::TargetRef::new("event", "note"));
    assert!(impact.complete);
    assert!(impact.content_references.is_empty());
    assert_eq!(impact.map_placements.len(), 1);
    assert_eq!(impact.map_placements[0].map_id, "overview");
    assert_eq!(impact.map_placements[0].placement_id, "note_marker");
    assert!(!impact.can_delete());
    let index = project.map_index();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    assert_eq!(
        index
            .placements_for(&worldline_core::catalog::TargetRef::new("event", "note"))
            .len(),
        1
    );
    let before_map = project
        .authoring_document(&map_path)
        .unwrap()
        .bytes()
        .to_vec();
    let error = project
        .remove_event("note")
        .expect_err("地图仍引用事件时必须拒绝删除");
    assert!(error.contains("overview"));
    assert!(error.contains("note_marker"));
    assert_eq!(project.sources(), before_sources);
    assert_eq!(
        project.authoring_document(&map_path).unwrap().bytes(),
        before_map
    );
    let unbound = String::from_utf8(before_map).unwrap().replace(
        r#""target_ref":{"kind":"event","id":"note"}"#,
        r#""target_ref":null"#,
    );
    project
        .set_authoring_document(&map_path, b"{invalid json".to_vec())
        .unwrap();
    let incomplete =
        project.deletion_impact(&worldline_core::catalog::TargetRef::new("event", "note"));
    assert!(!incomplete.complete);
    assert!(!incomplete.can_delete());
    assert!(!incomplete.diagnostics.is_empty());
    assert!(project.remove_event("note").is_err());
    assert_eq!(project.sources(), before_sources);
    project
        .set_authoring_document(&map_path, unbound.into_bytes())
        .unwrap();
    assert!(project
        .deletion_impact(&worldline_core::catalog::TargetRef::new("event", "note"))
        .can_delete());
    project.remove_event("note").unwrap();
    assert!(project.event_draft("note").is_err());
    assert!(project.map_index().maps["overview"]
        .placements
        .contains_key("note_marker"));
    fs::remove_dir_all(root).unwrap();
}

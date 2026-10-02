use super::*;
use worldline_core::vector_scene::{Affine, MapScene, PathSegment, SceneGeometry, SceneNode};

#[test]
fn map_scene_white_list_inherits_public_labels_without_ancestor_leaks() {
    let fixture = Fixture::new("scene", SOURCE, "1.10");
    let manifest_path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["maps"] = serde_json::json!({"atlas":".world/atlas.json"});
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .extend([
            serde_json::json!("presentation.maps.v1"),
            serde_json::json!("presentation.vector_scene.v1"),
        ]);
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let mut scene = MapScene::new(1000.0, 500.0);
    let mut parent = SceneNode::new(
        "private_parent",
        "lower",
        SceneGeometry::Group {
            children: vec!["curve".into(), "private_node".into()],
        },
    );
    parent.name = "CANARY_ANCESTOR_NAME".into();
    parent.annotation = "CANARY_ANCESTOR_ANNOTATION".into();
    parent.target_ref = Some(TargetRef::new("entity", "vault"));
    parent.transform = Affine([1.0, 0.0, 0.0, 1.0, 20.0, 30.0]);
    let mut curve = SceneNode::new(
        "curve",
        "lower",
        SceneGeometry::Path {
            segments: vec![
                PathSegment::Move { to: [20.0, 20.0] },
                PathSegment::Cubic {
                    control1: [80.0, 5.0],
                    control2: [120.0, 90.0],
                    to: [180.0, 50.0],
                },
            ],
        },
    );
    curve.name = "CANARY_SELECTED_AUTHOR_NAME".into();
    curve.parent_id = Some(parent.id.clone());
    curve.target_ref = Some(TargetRef::new("entity", "harbor"));
    curve.annotation = "公开航道".into();
    curve.visible = false;
    let mut private = SceneNode::new(
        "private_node",
        "lower",
        SceneGeometry::Point {
            position: [250.0, 100.0],
        },
    );
    private.parent_id = Some(parent.id.clone());
    private.annotation = "CANARY_UNSELECTED_NODE".into();
    scene
        .root_order
        .insert("lower".into(), vec![parent.id.clone()]);
    scene.root_order.insert("upper".into(), Vec::new());
    for node in [parent, curve, private] {
        scene.nodes.insert(node.id.clone(), node);
    }
    let map = serde_json::json!({
        "schema_version":1,"required_features":["presentation.vector_scene.v1"],"id":"atlas","title":"潮汐地图",
        "canvas":{"width":1000,"height":500,"unit":"normalized"},"raster_layers":[],
        "layer_order":["lower","upper"],
        "layers":{"lower":{"title":"CANARY_LAYER","visible_default":false,"locked":false},"upper":{"title":"上层","visible_default":true,"locked":false}},
        "placements":{"top_pin":{"target_ref":null,"role":"note","layer_id":"upper","geometry":{"kind":"point","position":[0.3,0.4]},"label_override":"上层标记","annotation":"公开顶层"}},
        "scene":scene
    });
    fs::write(
        fixture.root.join(".world/atlas.json"),
        serde_json::to_vec(&map).unwrap(),
    )
    .unwrap();
    let project = fixture.project();
    assert!(
        project.map_index().diagnostics.is_empty(),
        "{:?}",
        project.map_index().diagnostics
    );
    let mut choice = selection();
    choice.maps.push(ReaderMapSelection {
        id: "atlas".into(),
        placements: vec!["top_pin".into(), "curve".into()],
        raster_layers: vec![],
    });
    let (preview, files) = package(&project, &choice);
    let all = all_text(&files);
    assert!(!all.contains("CANARY"));
    assert!(!all.contains("private_parent") && !all.contains("private_node"));
    let map_path = route(&preview, "map", "atlas");
    let html = String::from_utf8_lossy(&files[Path::new(&map_path)]);
    assert!(
        html.contains("C80") || html.contains("C 80"),
        "Bezier path lost"
    );
    assert!(
        html.find("<path").unwrap() < html.find("<circle").unwrap(),
        "下层 scene 不应盖过上层 legacy"
    );
    assert!(html.contains("公开港口") && html.contains("公开航道"));
    let harbor = String::from_utf8_lossy(&files[Path::new(&route(&preview, "entity", "harbor"))]);
    assert!(harbor.contains("地图位置") && harbor.contains("#n"));
    let search: serde_json::Value =
        serde_json::from_slice(&files[Path::new("search-index.json")]).unwrap();
    assert!(search
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["kind"] == "map_placement"
            && entry["title"] == "公开港口"
            && entry["url"].as_str().unwrap().contains("#n")));
    let profile = project.create_reader_profile("mapped", &choice).unwrap();
    assert!(profile
        .routes
        .iter()
        .any(|entry| entry.target.as_ref() == Some(&TargetRef::new("map", "atlas"))));
}

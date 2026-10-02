use super::*;

#[test]
fn dash_close_followed_by_implicit_subpaths_cannot_evade_phase_restart_budget() {
    let source = format!(
        "<svg width='200' height='100'><path stroke-dasharray='{}' d='M0 0{}'/></svg>",
        vec!["1000"; 64].join(" "),
        "L0 0Z".repeat(800)
    );
    assert_eq!(
        svg_import::preview_scene(&source).unwrap_err().code,
        "SCENE_LIMIT"
    );
}

#[test]
fn dash_style_numeric_validation_and_negative_zero_preserve_semantics() {
    let mut scene = imported("<line id='p' x2='20' stroke-dasharray='3 2'/>").scene;
    for invalid in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        1e10,
        1e-100,
    ] {
        scene.nodes.get_mut("p").unwrap().style.stroke_dasharray = Some(vec![invalid, 2.0]);
        assert_eq!(
            validate_scene(&scene, &SceneLimits::default())
                .unwrap_err()
                .code,
            "SCENE_STYLE"
        );
    }
    scene.nodes.get_mut("p").unwrap().style.stroke_dasharray = Some(vec![-0.0, 8.0]);
    let svg = scene_to_safe_svg(&scene, 200.0, 100.0).unwrap();
    assert!(svg.contains("stroke-dasharray=\"0 8\""));
    let again = svg_import::preview_scene(&svg).unwrap();
    assert!(again
        .scene
        .nodes
        .values()
        .any(|node| node.style.stroke_dasharray == Some(vec![0.0, 8.0])));
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e10, 1e-100] {
        scene.nodes.get_mut("p").unwrap().style.stroke_dashoffset = Some(invalid);
        assert_eq!(
            validate_scene(&scene, &SceneLimits::default())
                .unwrap_err()
                .code,
            "SCENE_STYLE"
        );
    }
}

#[test]
fn dash_f32_large_coordinate_quantization_cannot_amplify_a_short_source_line() {
    let source = "<svg width='200' height='100'><path stroke-dasharray='.001 .001' d='M999999967 10L999999969 10'/></svg>";
    assert_eq!(
        svg_import::preview_scene(source).unwrap_err().code,
        "SCENE_LIMIT"
    );
}

#[test]
fn dash_raw_text_run_fields_missing_either_capability_block_generic_write_and_delete() {
    let (mut p, mut r) = project("dash-raw-text-capability");
    apply(
        &mut p,
        &mut r,
        vec![
            SceneOp::EnableScene,
            SceneOp::Insert {
                node: point("a", 10.0),
                index: None,
            },
        ],
    );
    p.save().unwrap();
    let file = path(&p);
    let original = p.authoring_document(&file).unwrap().bytes().to_vec();
    for field in ["stroke_dasharray", "stroke_dashoffset"] {
        for missing in ["map", "scene"] {
            let mut raw: Value = serde_json::from_slice(&original).unwrap();
            raw["required_features"]
                .as_array_mut()
                .unwrap()
                .push(json!(SCENE_DASH_FEATURE));
            raw["scene"]["required_features"] = json!([SCENE_DASH_FEATURE]);
            raw["scene"]["nodes"]["a"]["geometry"] = json!({"kind":"text", "x":0, "y":20, "runs":[{"text":"海岛", "style":{"future":{"keep":1}}}]});
            raw["scene"]["nodes"]["a"]["geometry"]["runs"][0]["style"][field] =
                if field == "stroke_dasharray" {
                    json!([])
                } else {
                    json!(0)
                };
            let object = if missing == "map" {
                &mut raw
            } else {
                &mut raw["scene"]
            };
            object["required_features"]
                .as_array_mut()
                .unwrap()
                .retain(|v| v.as_str() != Some(SCENE_DASH_FEATURE));
            let bytes = serde_json::to_vec_pretty(&raw).unwrap();
            let baseline = p.content_baseline();
            assert!(p.set_authoring_document(&file, bytes.clone()).is_err());
            assert_eq!(p.content_baseline(), baseline);
            std::fs::write(&file, &bytes).unwrap();
            let mut reopened = Project::open(&p.root).unwrap();
            let baseline = reopened.content_baseline();
            assert!(reopened.authoring_document(&file).unwrap().is_read_only());
            assert!(reopened
                .set_authoring_document(&file, original.clone())
                .is_err());
            assert!(reopened.delete_authoring_document(&file).is_err());
            assert_eq!(reopened.content_baseline(), baseline);
            assert_eq!(reopened.authoring_document(&file).unwrap().bytes(), bytes);
        }
    }
}

#[test]
fn dash_lexical_underflow_is_located_while_true_scientific_zero_remains_valid() {
    for field in ["stroke-dasharray", "stroke-dashoffset"] {
        for value in ["1e-999", "-1e-999", "+1e-999px"] {
            let source = format!("<svg width='200' height='100'>\n  <path id='underflow' d='M0 0L20 0'\n    {field}='{value}'/></svg>");
            let error = svg_import::preview_scene(&source).unwrap_err();
            assert_eq!(error.code, "SCENE_STYLE");
            assert_eq!(error.node_id.as_deref(), Some("underflow"));
            assert_eq!(error.field.as_deref(), Some(field));
            assert_eq!((error.line, error.column), (Some(3), Some(5)));
        }
    }
    let preview =
        svg_import::preview_scene(&line_source("0e-999 -0e-999 +0.0e-999px", "-0e-999px")).unwrap();
    assert_eq!(
        preview.scene.nodes["route"].style.stroke_dasharray,
        Some(vec![0.0, 0.0, 0.0])
    );
    assert_eq!(
        preview.scene.nodes["route"].style.stroke_dashoffset,
        Some(0.0)
    );
}

#[test]
fn dash_safe_svg_canonicalizes_arc_degrees_before_backend_radian_conversion() {
    for (rotation, canonical) in [(999999720.0, 0.0), (-999999720.0, 0.0), (-270.0, 90.0)] {
        let source = format!("<svg width='200' height='100'><path id='arc' stroke-dasharray='10000 10000' d='M0 0A100000000 .0000100001 {rotation} 0 1 200000000 0'/></svg>");
        // 极端长扁椭圆只用于零度；非零角用普通比例以免本来就超世界/虚线预算。
        let source = if canonical == 0.0 {
            source
        } else {
            source
                .replace("100000000 .0000100001", "100 10")
                .replace("200000000 0", "200 0")
        };
        let preview = svg_import::preview_scene(&source).unwrap();
        let SceneGeometry::Path { segments } = &preview.scene.nodes["arc"].geometry else {
            panic!("path expected")
        };
        let PathSegment::Arc {
            rotation: persisted,
            ..
        } = &segments[1]
        else {
            panic!("arc expected")
        };
        assert_eq!(*persisted, rotation);
        let safe = scene_to_safe_svg(&preview.scene, 200.0, 100.0).unwrap();
        assert!(!safe.contains("999999720"));
        let again = svg_import::preview_scene(&safe).unwrap();
        assert!(again.scene.nodes.values().any(|node| matches!(&node.geometry, SceneGeometry::Path { segments } if segments.iter().any(|segment| matches!(segment, PathSegment::Arc { rotation, .. } if *rotation == canonical)))));
    }
}

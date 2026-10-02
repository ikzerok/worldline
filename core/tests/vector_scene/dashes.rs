use super::*;
use serde_json::{json, Value};

fn imported(body: &str) -> SvgScenePreview {
    svg_import::preview_scene(&format!("<svg width='200' height='100'>{body}</svg>")).unwrap()
}
fn line_source(array: &str, offset: &str) -> String {
    format!("<svg width='200' height='100'><path id='route' fill='none' stroke='blue' stroke-dasharray='{array}' stroke-dashoffset='{offset}' d='M0 20L100 20'/></svg>")
}
fn imports(source: String) -> Vec<SceneOp> {
    vec![
        SceneOp::EnableScene,
        SceneOp::ImportSvg {
            layer_id: "routes".into(),
            title: "航道".into(),
            source,
        },
    ]
}
fn feature(value: &Value) -> bool {
    value
        .get("required_features")
        .and_then(Value::as_array)
        .is_some_and(|f| f.iter().any(|v| v.as_str() == Some(SCENE_DASH_FEATURE)))
}

#[test]
fn dash_odd_px_none_zero_inheritance_and_inline_precedence_round_trip() {
    let preview = imported("<g id='group' stroke='blue' fill='none' stroke-dasharray='3,2px 1' stroke-dashoffset='-2px' opacity='.5' transform='matrix(2 .2 .1 1 4 5)'><path id='route' d='M1 2C10 0 20 10 30 2M4 20Q10 1 40 20A4 5 30 0 1 50 30Z'/><line id='solid' y1='40' x2='70' y2='40' stroke-dasharray='none' stroke-dashoffset='inherit'/><line id='zero' y1='50' x2='70' y2='50' stroke-dasharray='0 0 0'/><line id='inline' y1='60' x2='70' y2='60' stroke-dasharray='20 20' style='stroke-dasharray: 5px, 3; stroke-dashoffset: +1.5e1'/></g>");
    let source = &preview.scene;
    assert_eq!(
        source.nodes["group"].style.stroke_dasharray,
        Some(vec![3.0, 2.0, 1.0])
    );
    assert_eq!(source.nodes["group"].style.stroke_dashoffset, Some(-2.0));
    assert_eq!(source.nodes["route"].style.stroke_dasharray, None);
    assert_eq!(source.nodes["solid"].style.stroke_dasharray, Some(vec![]));
    assert_eq!(
        source.nodes["zero"].style.stroke_dasharray,
        Some(vec![0.0, 0.0, 0.0])
    );
    assert_eq!(
        source.nodes["inline"].style.stroke_dasharray,
        Some(vec![5.0, 3.0])
    );
    assert_eq!(source.nodes["inline"].style.stroke_dashoffset, Some(15.0));
    let effective = node_state(source, "route").unwrap().style;
    assert_eq!(effective.stroke_dasharray, Some(vec![3.0, 2.0, 1.0]));
    assert_eq!(effective.stroke_dashoffset, Some(-2.0));
    assert_eq!(effective.opacity, None);
    let safe = scene_to_safe_svg(source, preview.width, preview.height).unwrap();
    assert!(safe.contains("stroke-dasharray=\"3 2 1\""));
    assert!(safe.contains("stroke-dasharray=\"0 0 0\""));
    assert!(safe.contains("stroke-dasharray=\"none\""));
    assert!(safe.contains("C10 0 20 10 30 2M4 20Q10 1 40 20A4 5 30 0 1 50 30Z"));
    assert!(safe.contains("opacity=\"0.5\""));
    let again = svg_import::preview_scene(&safe).unwrap();
    let a = project_scene(source, 0.25).unwrap();
    let b = project_scene(&again.scene, 0.25).unwrap();
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(&b) {
        assert_eq!(a.paths, b.paths);
        assert_eq!(a.closed, b.closed);
        assert_eq!(a.style, b.style);
        assert_eq!(a.transform, b.transform);
        assert_eq!(a.clips, b.clips);
    }
    assert!(feature(&serde_json::to_value(&again.scene).unwrap()));
}

#[test]
fn dash_invalid_syntax_units_and_unsafe_extensions_have_precise_locations() {
    for array in [
        "",
        "-1 2",
        "1,,2",
        ",1",
        "1,",
        "1-2",
        "1.2.3",
        "1% 2",
        "1em 2",
        "1 px",
        "NaN 2",
        "inf",
        "1e999",
        "1e-100 1",
        "1e-999 0",
        "-1e-999 0",
        "calc(2) 4",
        "var(--x)",
        "url(http://bad)",
    ] {
        let source = format!("<svg width='200' height='100'>\n  <path id='bad' d='M0 0L10 0'\n    stroke-dasharray='{array}'/>\n</svg>");
        let error = svg_import::preview_scene(&source).unwrap_err();
        assert_eq!(error.node_id.as_deref(), Some("bad"), "{array}");
        assert_eq!(error.field.as_deref(), Some("stroke-dasharray"), "{array}");
        assert_eq!((error.line, error.column), (Some(3), Some(5)), "{array}");
    }
    for offset in [
        "none", "1 2", "1%", "1cm", "NaN", "-inf", "1e-100", "1e-999", "-1e-999",
    ] {
        let error = svg_import::preview_scene(&line_source("3 2", offset)).unwrap_err();
        assert_eq!(
            error.field.as_deref(),
            Some("stroke-dashoffset"),
            "{offset}"
        );
    }
    let inline = "<svg width='20' height='20'>\n<path id='bad' d='M0 0L10 0'\n  style='stroke-dasharray:1% 2'/></svg>";
    let error = svg_import::preview_scene(inline).unwrap_err();
    assert_eq!((error.line, error.column), (Some(3), Some(3)));
    assert_eq!(error.field.as_deref(), Some("stroke-dasharray"));
    for attribute in [
        "pathLength='12'",
        "onload='run()'",
        "filter='url(#x)'",
        "stroke-dashcorner='4'",
    ] {
        let source = line_source("3 2", "0").replace("d='M0 20", &format!("{attribute} d='M0 20"));
        assert_eq!(
            svg_import::preview_scene(&source).unwrap_err().code,
            "SCENE_SVG_PROFILE"
        );
    }
}

#[test]
fn dash_text_rejects_effective_values_at_tspan_and_accepts_solid_overrides() {
    let source = "<svg width='200' height='100' stroke-dasharray='3 2'><text id='label' y='20' stroke-dasharray='none'><tspan id='piece' stroke-dasharray='inherit'>海</tspan></text></svg>";
    assert!(svg_import::preview_scene(source).is_ok());
    let source = "<svg width='200' height='100' stroke-dasharray='3 2'><text y='20'><tspan stroke-dasharray='none'>海</tspan><tspan stroke-dasharray='0 0'>岛</tspan></text></svg>";
    assert!(svg_import::preview_scene(source).is_ok());
    let source = "<svg width='200' height='100'><text y='20' stroke-dasharray='none'>海\n<tspan id='bad'\n stroke-dasharray='3 2'>岛</tspan></text></svg>";
    let error = svg_import::preview_scene(source).unwrap_err();
    assert_eq!(error.code, "SCENE_STYLE");
    assert_eq!(error.node_id.as_deref(), Some("bad"));
    assert_eq!(error.field.as_deref(), Some("stroke-dasharray"));
    assert_eq!((error.line, error.column), (Some(3), Some(2)));
    assert!(error.message.contains("文字"));
}

#[test]
fn dash_limits_cover_arrays_short_periods_hidden_geometry_and_subpath_restarts() {
    let too_many = vec!["1"; MAX_DASH_ENTRIES + 1].join(" ");
    assert_eq!(
        svg_import::preview_scene(&line_source(&too_many, "0"))
            .unwrap_err()
            .code,
        "SCENE_LIMIT"
    );
    for array in [".00001 .00001", "0 .00001 0", "0 0 .00001 0"] {
        for offset in ["0", "99999", "-99999"] {
            assert_eq!(
                svg_import::preview_scene(&line_source(array, offset))
                    .unwrap_err()
                    .code,
                "SCENE_LIMIT"
            );
        }
    }
    let mut scene = imported("<path id='p' stroke-dasharray='1 1' d='M0 0L10 0'/>").scene;
    scene.nodes.get_mut("p").unwrap().style.stroke_dasharray = Some(vec![0.00001, 0.00001]);
    scene.nodes.get_mut("p").unwrap().visible = false;
    scene.nodes.get_mut("p").unwrap().style.stroke = Some("none".into());
    assert_eq!(
        validate_scene(&scene, &SceneLimits::default())
            .unwrap_err()
            .code,
        "SCENE_LIMIT"
    );
    assert_eq!(
        scene_to_safe_svg(&scene, 200.0, 100.0).unwrap_err().code,
        "SCENE_LIMIT"
    );
    let mut preview = imported("<path id='p' stroke-dasharray='1000 1000' d='M0 0L1 0M0 2L1 2'/>");
    let mut limits = SceneLimits {
        max_dash_work: 12,
        ..SceneLimits::default()
    };
    assert_eq!(
        validate_scene(&preview.scene, &limits).unwrap_err().code,
        "SCENE_LIMIT"
    );
    limits.max_dash_work = 20;
    assert!(validate_scene(&preview.scene, &limits).is_ok());
    limits.max_dash_entries = 1;
    assert_eq!(
        validate_scene(&preview.scene, &limits).unwrap_err().code,
        "SCENE_LIMIT"
    );
    limits.max_dash_entries = MAX_DASH_ENTRIES + 1;
    assert_eq!(
        validate_scene(&preview.scene, &limits).unwrap_err().code,
        "SCENE_LIMIT"
    );
    limits = SceneLimits {
        max_dash_work: MAX_DASH_WORK + 1,
        ..SceneLimits::default()
    };
    assert_eq!(
        validate_scene(&preview.scene, &limits).unwrap_err().code,
        "SCENE_LIMIT"
    );
    preview
        .scene
        .nodes
        .get_mut("p")
        .unwrap()
        .style
        .stroke_dasharray = Some(vec![0.0; MAX_DASH_ENTRIES]);
    assert!(validate_scene(&preview.scene, &SceneLimits::default()).is_ok());
}

#[test]
fn dash_curves_use_conservative_lengths_and_accumulate_across_nodes() {
    for geometry in [
        "M0 0C100 0 -100 0 0 0",
        "M0 0Q100 0 0 0",
        "M0 0A1 .001 0 0 1 100 0",
    ] {
        let source = format!("<svg width='200' height='100'><path stroke-dasharray='.001 .001' d='{geometry}'/></svg>");
        assert_eq!(
            svg_import::preview_scene(&source).unwrap_err().code,
            "SCENE_LIMIT",
            "{geometry}"
        );
    }
    let source = "<svg width='200' height='100'><g stroke-dasharray='.003 .003'><line x2='200'/><line y1='10' x2='200' y2='10'/></g></svg>";
    assert_eq!(
        svg_import::preview_scene(source).unwrap_err().code,
        "SCENE_LIMIT"
    );
}

#[test]
fn dash_explicit_styles_declare_both_features_and_survive_worker_undo_save_reopen() {
    let (mut p, mut r) = project("dash-lifecycle");
    let before = p.authoring_document(&path(&p)).unwrap().bytes().to_vec();
    let request = batch(&p, r, imports(line_source("15 12 3", "-8")));
    let plan = preview_batch(&p, r, request).unwrap();
    let normalized: SceneBatch =
        serde_json::from_value(serde_json::to_value(plan.normalized_batch()).unwrap()).unwrap();
    let worker = preview_batch(&p, r, normalized).unwrap();
    assert_eq!(plan.document_after(), worker.document_after());
    let after: Value = serde_json::from_slice(plan.document_after()).unwrap();
    assert!(feature(&after));
    assert!(feature(&after["scene"]));
    let result = apply_batch(&mut p, &mut r, &plan).unwrap();
    assert_eq!(result.undo_record.changes.len(), 1);
    let persisted = scene(&p);
    let svg = map_to_safe_svg(&map(&p), None).unwrap();
    assert!(svg.contains("stroke-dasharray=\"15 12 3\""));
    p.save().unwrap();
    assert_eq!(scene(&Project::open(&p.root).unwrap()), persisted);
    let revision = r;
    presentation_commands::undo(&mut p, &mut r, revision, &result.undo_record).unwrap();
    assert_eq!(p.authoring_document(&path(&p)).unwrap().bytes(), before);
    let revision = r;
    let mut redo = result.undo_record.clone();
    for change in &mut redo.changes {
        std::mem::swap(&mut change.before, &mut change.after);
    }
    presentation_commands::undo(&mut p, &mut r, revision, &redo).unwrap();
    assert_eq!(scene(&p), persisted);
}

#[test]
fn dash_cancel_stale_invalid_and_unknown_features_preserve_every_original_byte() {
    let (mut p, mut r) = project("dash-zero-mutation");
    let before = p.content_baseline();
    let request = batch(&p, r, imports(line_source("15 12", "-2")));
    let error = preview_batch_with_control(
        &p,
        r,
        request.clone(),
        &SceneLimits::default(),
        &mut |progress| progress.stage != "svg_validate",
    )
    .unwrap_err();
    assert_eq!(error.code, "SCENE_CANCELLED");
    assert_eq!(p.content_baseline(), before);
    let plan = preview_batch(&p, r, request).unwrap();
    assert_eq!(
        apply_batch_with_control(&mut p, &mut r, &plan, &mut |_| false)
            .unwrap_err()
            .code,
        "SCENE_CANCELLED"
    );
    assert_eq!(p.content_baseline(), before);
    for source in [
        line_source("-1 2", "0"),
        line_source(".00001 .00001", "0"),
        line_source("1e-999 0", "0"),
        line_source("-1e-999 0", "0"),
        line_source("15 12", "1e-999"),
        line_source("15 12", "-1e-999"),
    ] {
        assert!(preview_batch(&p, r, batch(&p, r, imports(source))).is_err());
        assert_eq!(p.content_baseline(), before);
    }
    apply(&mut p, &mut r, vec![SceneOp::EnableScene]);
    let current = p.content_baseline();
    assert_eq!(
        apply_batch(&mut p, &mut r, &plan).unwrap_err().code,
        "SCENE_STALE"
    );
    assert_eq!(p.content_baseline(), current);
    let mut imported = imported("<line x2='20' stroke-dasharray='3 2'/>").scene;
    imported.extra.insert(
        "required_features".into(),
        json!([SCENE_DASH_FEATURE, "future.dash"]),
    );
    assert_eq!(
        validate_scene(&imported, &SceneLimits::default())
            .unwrap_err()
            .code,
        "SCENE_FEATURE"
    );
}

#[test]
fn dash_legacy_styles_do_not_upgrade_and_new_explicit_zero_none_are_protected() {
    let (mut p, mut r) = project("dash-compatibility");
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
    let raw: Value =
        serde_json::from_slice(p.authoring_document(&path(&p)).unwrap().bytes()).unwrap();
    assert!(!feature(&raw));
    assert!(!feature(&raw["scene"]));
    assert!(raw["scene"]["nodes"]["a"]["style"]
        .get("stroke_dasharray")
        .is_none());
    for array in [vec![], vec![0.0], vec![15.0, 12.0]] {
        let mut node = scene(&p).nodes["a"].clone();
        node.style
            .extra
            .insert("future-style".into(), json!({"nested":"preserve"}));
        node.style.stroke_dasharray = Some(array.clone());
        node.style.stroke_dashoffset = Some(0.0);
        apply(&mut p, &mut r, vec![SceneOp::Update { node }]);
        let raw: Value =
            serde_json::from_slice(p.authoring_document(&path(&p)).unwrap().bytes()).unwrap();
        assert!(feature(&raw));
        assert!(feature(&raw["scene"]));
        assert_eq!(scene(&p).nodes["a"].style.stroke_dasharray, Some(array));
        assert_eq!(
            scene(&p).nodes["a"].style.extra["future-style"],
            json!({"nested":"preserve"})
        );
    }
    let mut standalone = scene(&p);
    standalone.extra.remove("required_features");
    assert_eq!(
        validate_scene(&standalone, &SceneLimits::default())
            .unwrap_err()
            .code,
        "SCENE_FEATURE"
    );
    assert_eq!(
        scene_to_safe_svg(&standalone, 200.0, 100.0)
            .unwrap_err()
            .code,
        "SCENE_FEATURE"
    );
    p.save().unwrap();
    for location in ["map", "scene"] {
        let file = path(&p);
        let mut raw: Value =
            serde_json::from_slice(p.authoring_document(&file).unwrap().bytes()).unwrap();
        let object = if location == "map" {
            &mut raw
        } else {
            &mut raw["scene"]
        };
        object["required_features"]
            .as_array_mut()
            .unwrap()
            .retain(|v| v.as_str() != Some(SCENE_DASH_FEATURE));
        let bytes = serde_json::to_vec_pretty(&raw).unwrap();
        std::fs::write(&file, &bytes).unwrap();
        let mut reopened = Project::open(&p.root).unwrap();
        let baseline = reopened.content_baseline();
        assert!(reopened
            .set_authoring_document(&file, b"{}".to_vec())
            .is_err());
        assert!(reopened.delete_authoring_document(&file).is_err());
        assert_eq!(reopened.content_baseline(), baseline);
        assert!(
            reopened.authoring_document(&file).unwrap().is_read_only(),
            "{location}"
        );
        assert_eq!(reopened.authoring_document(&file).unwrap().bytes(), bytes);
    }
}

#[test]
fn dash_public_selection_retains_inherited_styles_without_private_nodes() {
    let (mut p, mut r) = project("dash-public-selection");
    apply(&mut p, &mut r, imports("<svg width='200' height='100'><g stroke='blue' stroke-dasharray='15 12' opacity='.5'><path id='public' d='M0 20L100 20'/><text id='private' stroke-dasharray='none' y='40'>私密文字</text></g></svg>".into()));
    let map = map(&p);
    let public = map
        .scene
        .as_ref()
        .unwrap()
        .nodes
        .values()
        .find(|n| n.name == "public")
        .unwrap()
        .id
        .clone();
    let selected = BTreeSet::from([public]);
    let svg = to_safe_svg(&map, Some(&selected)).unwrap();
    assert!(svg.contains("stroke-dasharray=\"15 12\""));
    assert!(svg.contains("opacity=\"0.5\""));
    assert!(!svg.contains("私密文字"));
    let layers = to_safe_svg_layers_with_links(&map, Some(&selected), &BTreeMap::new()).unwrap();
    assert_eq!(layers.len(), 1);
    assert!(layers["routes"].contains("stroke-dasharray=\"15 12\""));
    assert!(!layers["routes"].contains("私密文字"));
}

#[path = "dash_guards.rs"]
mod guards;

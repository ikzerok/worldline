use super::*;

fn svg(body: &str) -> String {
    format!("<svg viewBox='0 0 100 80'>{body}</svg>")
}

#[test]
fn complete_profile_preserves_curves_text_and_affine() {
    let source = svg("<g id='group' transform='translate(2 3) skewX(20)' fill-rule='evenodd'><rect width='4' height='5' rx='1'/><circle r='2'/><ellipse rx='3' ry='4'/><line x2='3' y2='4'/><polyline points='0,0 2,3'/><polygon points='0,0 2,0 1,2'/><path id='curve' d='M0 0C1 2 3 4 5 6A1 2 0 01 8 9z'/><text x='2' y='3' xml:space='preserve'>A&amp;B<tspan x='10' dx='2' fill='red'>&#x4e2d;&lt;</tspan> C</text></g>");
    let preview = preview_scene(&source).unwrap();
    assert_eq!((preview.width, preview.height), (100.0, 80.0));
    assert_eq!(preview.scene.nodes.len(), 10);
    assert!(
        matches!(&preview.scene.nodes["curve"].geometry, SceneGeometry::Path { segments } if segments.len() == 4)
    );
    let runs = preview
        .scene
        .nodes
        .values()
        .find_map(|n| match &n.geometry {
            SceneGeometry::Text { runs, .. } => Some(runs),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "A&B中< C"
    );
    assert_eq!(runs[1].x, Some(10.0));
    assert_eq!(runs[1].dx, 2.0);
    assert_eq!(runs[1].style.fill.as_deref(), Some("red"));
}

#[test]
fn root_viewport_crop_and_meaningful_transform_survive() {
    let p = preview_scene("<svg width='100' height='100' viewBox='10 20 200 100' preserveAspectRatio='xMidYMid slice' transform='rotate(10)' fill='red'><rect width='3' height='4'/></svg>").unwrap();
    let root = &p.scene.nodes[&p.scene.root_order["svg"][0]];
    assert_eq!(root.clip_rect, Some([60.0, 20.0, 100.0, 100.0]));
    assert_ne!(root.transform, Affine::IDENTITY);
    assert_eq!(root.style.fill.as_deref(), Some("red"));
}

#[test]
fn safe_clip_extension_roundtrips_without_redundant_root() {
    let source = svg("<defs><clipPath id='wl-viewport-42' clipPathUnits='userSpaceOnUse'><rect width='100' height='80'/></clipPath></defs><g data-worldline-viewport='1' clip-path='url(#wl-viewport-42)'><path d='M0 0Q1 2 3 4'/></g>");
    let first = preview_scene(&source).unwrap();
    assert_eq!(first.scene.nodes.len(), 2);
    let safe = super::super::scene_to_safe_svg(&first.scene, first.width, first.height).unwrap();
    let second = preview_scene(&safe).unwrap();
    assert_eq!(
        second
            .scene
            .nodes
            .values()
            .filter(|n| n.clip_rect.is_some())
            .count(),
        1
    );
}

#[test]
fn dangerous_unknown_malformed_and_nested_input_is_located() {
    for body in [
        "<script/>",
        "<g onclick='evil()'/>",
        "<image href='https://x'/>",
        "<path d='M0 0' fill='url(#x)'/>",
        "<g><svg viewBox='0 0 1 1'/></g>",
        "<foreignObject/>",
        "<style/>",
        "<g mystery='1'/>",
        "<text>&external;</text>",
        "<text><tspan><tspan>bad</tspan></tspan></text>",
        "<rect width='NaN'/>",
        "<g id='same'/><g id='same'/>",
        "<g clip-path='url(#wl-viewport-1)'/>",
        "<g data-worldline-viewport='1'/>",
    ] {
        let error = preview_scene(&format!("<svg viewBox='0 0 10 10'>\n{body}</svg>")).unwrap_err();
        assert!(error.line.is_some() && error.column.is_some(), "{body}");
    }
    for source in [
        "<!DOCTYPE svg [<!ENTITY x 'evil'>]><svg/>",
        "<?bad x?><svg/>",
        "<svg/><svg/>",
        "<svg viewBox='0 0 1 1'><g></svg>",
    ] {
        assert!(preview_scene(source).is_err(), "{source}");
    }
}

#[test]
fn clip_definitions_are_strict_unique_local_and_used_once() {
    for body in [
        "<defs><clipPath id='wrong' clipPathUnits='userSpaceOnUse'><rect width='1' height='1'/></clipPath></defs>",
        "<defs><clipPath id='wl-viewport-1' clipPathUnits='objectBoundingBox'><rect width='1' height='1'/></clipPath></defs>",
        "<defs><clipPath id='wl-viewport-1' clipPathUnits='userSpaceOnUse'><rect width='1' height='1' transform='scale(2)'/></clipPath></defs>",
        "<defs><clipPath id='wl-viewport-1' clipPathUnits='userSpaceOnUse'><rect width='1' height='1'/></clipPath></defs>",
        "<g data-worldline-viewport='1' clip-path='url(#wl-viewport-99)'/>",
    ] { assert!(preview_scene(&svg(body)).is_err(), "{body}"); }
}

#[test]
fn budgets_and_cancellation_reject_without_a_partial_scene() {
    let source = svg("<path d='M0 0L1 1'/><text>abc</text>");
    let limits = SceneLimits {
        max_segments: 1,
        ..SceneLimits::default()
    };
    assert_eq!(
        preview_scene_with_control(&source, &limits, &mut |_| true)
            .unwrap_err()
            .code,
        "SCENE_LIMIT"
    );
    let limits = SceneLimits {
        max_text_bytes: 2,
        ..SceneLimits::default()
    };
    assert_eq!(
        preview_scene_with_control(&source, &limits, &mut |_| true)
            .unwrap_err()
            .code,
        "SCENE_LIMIT"
    );
    assert_eq!(
        preview_scene_with_control(&source, &SceneLimits::default(), &mut |_| false)
            .unwrap_err()
            .code,
        "SCENE_CANCELLED"
    );
}

#[test]
fn ids_are_stable_and_generated_names_avoid_later_explicit_ids() {
    let source = svg("<rect id='svg_1' width='1' height='1'/><circle id='with.dot' r='1'/>");
    let first = preview_scene(&source).unwrap();
    assert_eq!(first, preview_scene(&source).unwrap());
    assert!(first.scene.nodes.contains_key("svg_1"));
    assert_eq!(first.scene.nodes.len(), 3);
}

#[test]
fn xml_declarations_entities_and_whitespace_are_controlled() {
    let p = preview_scene("<?xml version='1.0' encoding='UTF-8' standalone='yes'?><svg viewBox='0 0 10 10'><text>  A\n\t B &amp; &#65;<tspan> C </tspan> </text></svg>").unwrap();
    let runs = p
        .scene
        .nodes
        .values()
        .find_map(|n| match &n.geometry {
            SceneGeometry::Text { runs, .. } => Some(runs),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        "A B & A C"
    );
    for source in [
        "<?xml version='1.0' mystery='x'?><svg/>",
        "<?xml version='1.0' version='1.0'?><svg/>",
        "<?xml version='1.0' standalone='yes' encoding='utf-8'?><svg/>",
        "<?xml version='1.1'?><svg/>",
        "&#32;<svg viewBox='0 0 10 10'/>",
        "<svg viewBox='0 0 10 10'><text>&#0;</text></svg>",
        "<svg viewBox='0 0 10 10'><text>]]></text></svg>",
    ] {
        assert!(preview_scene(source).is_err(), "{source}");
    }
    let error = preview_scene("<svg viewBox='0 0 10 10'>\r<g bad='1'/></svg>").unwrap_err();
    assert_eq!(error.line, Some(2));
}

#[test]
fn nested_real_viewport_groups_keep_both_clips() {
    let p = preview_scene(&svg("<defs><clipPath id='wl-viewport-2' clipPathUnits='userSpaceOnUse'><rect width='100' height='80'/></clipPath><clipPath id='wl-viewport-9' clipPathUnits='userSpaceOnUse'><rect x='2' y='3' width='4' height='5'/></clipPath></defs><g data-worldline-viewport='1' clip-path='url(#wl-viewport-2)'><g data-worldline-viewport='1' clip-path='url(#wl-viewport-9)'><circle r='1'/></g></g>")).unwrap();
    assert_eq!(
        p.scene
            .nodes
            .values()
            .filter(|n| n.clip_rect.is_some())
            .count(),
        2
    );
    assert_eq!(p.scene.nodes.len(), 3);
}

#[test]
fn numeric_styles_trim_whitespace_and_do_not_treat_opacity_as_a_length() {
    let parsed = preview_scene(&svg(
        "<rect width='10' height='10' fill=' red ' stroke-width=' 2px ' opacity=' 0.5 '/>",
    ))
    .unwrap();
    let node = parsed
        .scene
        .nodes
        .values()
        .find(|n| matches!(n.geometry, SceneGeometry::Rect { .. }))
        .unwrap();
    assert_eq!(node.style.fill.as_deref(), Some("red"));
    assert_eq!(node.style.stroke_width, Some(2.0));
    assert_eq!(node.style.opacity, Some(0.5));
    for attribute in [
        "opacity='0.5px'",
        "fill-opacity='0.5px'",
        "stroke-opacity='0.5px'",
        "stroke-miterlimit='4px'",
    ] {
        assert!(
            preview_scene(&svg(&format!("<rect width='10' height='10' {attribute}/>"))).is_err()
        );
    }
}

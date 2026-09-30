//! SVG 子集、安全拒绝和事务回归。
use std::collections::BTreeMap;
use worldline_core::{
    map_creation::{self, CreateMapRequest, MISSING_DOCUMENT_HASH},
    presentation::MapGeometry,
    presentation_commands::{document_hash, Revision},
    project::Project,
    svg_import,
};
const SVG:&str="<svg xmlns='http://www.w3.org/2000/svg' viewBox='10 20 100 100'><g fill='#abc' stroke='#123456'><rect x='20' y='30' width='40' height='20' fill-opacity='0.5'/><ellipse cx='60' cy='70' rx='20' ry='10'/><path d='M10 20L30 20v20h-20z'/></g></svg>";
#[test]
fn shapes_normalize_inherit_style_and_close_paths() {
    let p = svg_import::preview(SVG).unwrap();
    assert_eq!(p.shapes.len(), 3);
    assert_eq!(p.shapes[0].style["fill"], "#aabbcc");
    assert_eq!(p.shapes[0].style["fill_opacity"], 0.5);
    assert_eq!(p.shapes[0].geometry.points()[0], [0.1, 0.1]);
    assert_eq!(p.shapes[1].geometry.points().len(), 64);
    assert!(matches!(p.shapes[2].geometry, MapGeometry::Polygon { .. }));
}
#[test]
fn unsafe_unsupported_and_malformed_svg_are_rejected() {
    for body in [
        "<script/>",
        "<image href='https://evil'/>",
        "<foreignObject/>",
        "<rect onload='x' width='1' height='1'/>",
        "<rect transform='scale(2)'/>",
        "<path d='M0 0A1 2 3 4 5 6 7'/>",
        "<rect width='2' height='2' fill='url(#x)'/>",
        "<rect x='-1' width='2' height='2'/>",
        "<g style='fill:red'/>",
        "<rect width='NaN' height='2'/>",
        "<rect width='2' height='2' fill-opacity='2'/>",
    ] {
        assert!(
            svg_import::preview(&format!("<svg viewBox='0 0 10 10'>{body}</svg>")).is_err(),
            "accepted {body}"
        );
    }
    for source in [
        "<!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><svg/>",
        "<svg viewBox='0 0 10 10'><rect width='1' height='1'/>",
        "<svg viewBox='0 0 10 10'/><svg/>",
    ] {
        assert!(svg_import::preview(source).is_err());
    }
}
fn project(name: &str) -> (Project, Revision) {
    let mut p = Project::new(
        &std::env::temp_dir().join(format!("svg-import-{name}-{}", std::process::id())),
    );
    let mut r = Revision::default();
    let baseline = BTreeMap::from([(
        p.root.join(".world/project.json"),
        MISSING_DOCUMENT_HASH.into(),
    )]);
    map_creation::create_map(
        &mut p,
        &mut r,
        CreateMapRequest::new("map", "地图", 100, 100),
        baseline,
    )
    .unwrap();
    (p, r)
}
fn baseline(p: &Project) -> BTreeMap<std::path::PathBuf, String> {
    let path = p.root.join(".world/maps/map.json");
    BTreeMap::from([(
        path.clone(),
        document_hash(p.authoring_document(&path).unwrap().bytes()),
    )])
}
#[test]
fn import_is_atomic_undoable_by_snapshot_and_rejects_stale() {
    let (mut p, mut r) = project("atomic");
    let before = p.clone();
    let expected = r;
    let base = baseline(&p);
    assert_eq!(
        svg_import::apply(&mut p, &mut r, "map", "svg_1", SVG, expected, base.clone()).unwrap(),
        3
    );
    assert_eq!(r, expected.next_presentation());
    let path = p.root.join(".world/maps/map.json");
    let after = p.authoring_document(&path).unwrap().bytes().to_vec();
    let value: serde_json::Value = serde_json::from_slice(&after).unwrap();
    assert_eq!(
        value["placements"]["svg_1_0001"]["style"]["fill"],
        "#aabbcc"
    );
    assert!(svg_import::apply(&mut p, &mut r, "map", "svg_2", SVG, expected, base).is_err());
    assert_eq!(p.authoring_document(&path).unwrap().bytes(), after);
    let base = baseline(&p);
    let expected = r;
    assert!(svg_import::apply(&mut p, &mut r, "map", "svg_1", SVG, expected, base).is_err());
    assert_eq!(p.authoring_document(&path).unwrap().bytes(), after);
    p = before;
    assert!(
        !String::from_utf8_lossy(p.authoring_document(&path).unwrap().bytes()).contains("svg_1")
    );
}
#[test]
fn later_invalid_shape_does_not_leave_layer_or_earlier_shape() {
    let (mut p, mut r) = project("invalid");
    let base = baseline(&p);
    let expected = r;
    let source="<svg viewBox='0 0 10 10'><rect width='1' height='1'/><polygon points='0,0 9,9 0,9 9,0'/></svg>";
    assert!(svg_import::apply(
        &mut p,
        &mut r,
        "map",
        "svg_1",
        source,
        expected,
        base.clone()
    )
    .is_err());
    assert_eq!(r, expected);
    assert_eq!(baseline(&p), base);
}
#[test]
fn changed_bytes_and_unknown_features_refuse_import() {
    let (mut p, mut r) = project("features");
    let stale = baseline(&p);
    let path = p.root.join(".world/maps/map.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&path).unwrap().bytes()).unwrap();
    value["required_features"] = serde_json::json!(["future.vector"]);
    p.save().unwrap();
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    p = Project::open(&p.entry).unwrap();
    let expected = r;
    assert!(svg_import::apply(&mut p, &mut r, "map", "svg_1", SVG, expected, stale).is_err());
    let base = baseline(&p);
    assert!(
        svg_import::apply(&mut p, &mut r, "map", "svg_1", SVG, expected, base.clone()).is_err()
    );
    assert_eq!(baseline(&p), base);
}

#[test]
fn imported_shape_ids_preserve_svg_paint_order_beyond_nine_shapes() {
    let (mut p, mut r) = project("order");
    let base = baseline(&p);
    let expected = r;
    let body = (0..12)
        .map(|i| format!("<rect x='{i}' y='0' width='1' height='1'/>"))
        .collect::<String>();
    let source = format!("<svg viewBox='0 0 20 20'>{body}</svg>");
    svg_import::apply(&mut p, &mut r, "map", "svg_1", &source, expected, base).unwrap();
    let maps = p.map_index();
    let positions = maps.maps["map"]
        .placements
        .values()
        .map(|p| p.geometry.points()[0][0])
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|p| p[0] < p[1]));
}

fn preview_path(d: &str) -> Vec<[f64; 2]> {
    svg_import::preview(&format!("<svg viewBox='0 0 100 100'><path d='{d}'/></svg>"))
        .unwrap()
        .shapes[0]
        .geometry
        .points()
        .to_vec()
}
#[test]
fn curves_support_absolute_relative_and_repeated_parameters() {
    for (absolute, relative) in [
        (
            "M10 10 C20 10 20 20 30 20 40 20 40 30 50 30",
            "m10 10 c10 0 10 10 20 10 10 0 10 10 20 10",
        ),
        (
            "M10 10 Q20 20 30 10 40 0 50 10",
            "m10 10 q10 10 20 0 10 -10 20 0",
        ),
    ] {
        assert_eq!(preview_path(absolute), preview_path(relative));
        assert!(preview_path(absolute).len() > 4);
    }
    assert_eq!(
        preview_path("M10 10 C20 10 20 20 30 20").last(),
        Some(&[0.3, 0.2])
    );
    assert_eq!(
        preview_path("M10 10 Q20 20 30 10").last(),
        Some(&[0.3, 0.1])
    );
}
fn distance_to_polyline(point: [f64; 2], points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|p| {
            let d = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
            let length = d[0] * d[0] + d[1] * d[1];
            let t = if length == 0. {
                0.
            } else {
                ((point[0] - p[0][0]) * d[0] + (point[1] - p[0][1]) * d[1]) / length
            }
            .clamp(0., 1.);
            (point[0] - p[0][0] - t * d[0]).hypot(point[1] - p[0][1] - t * d[1])
        })
        .fold(f64::INFINITY, f64::min)
}
#[test]
fn flattened_curves_meet_normalized_error_even_with_loop_or_cusp() {
    for (path, control) in [
        (
            "M10 10 C90 10 10 90 90 90",
            vec![[0.1, 0.1], [0.9, 0.1], [0.1, 0.9], [0.9, 0.9]],
        ),
        (
            "M50 50 C90 10 10 10 50 50",
            vec![[0.5, 0.5], [0.9, 0.1], [0.1, 0.1], [0.5, 0.5]],
        ),
        (
            "M10 50 Q90 50 20 50",
            vec![[0.1, 0.5], [0.9, 0.5], [0.2, 0.5]],
        ),
    ] {
        let points = preview_path(path);
        for i in 0..=1000 {
            let t = f64::from(i) / 1000.;
            let mut row = control.clone();
            while row.len() > 1 {
                row = row
                    .windows(2)
                    .map(|p| {
                        [
                            p[0][0] * (1. - t) + p[1][0] * t,
                            p[0][1] * (1. - t) + p[1][1] * t,
                        ]
                    })
                    .collect();
            }
            assert!(
                distance_to_polyline(row[0], &points) <= 0.00025 + 1e-12,
                "{path} at {t}"
            );
        }
    }
}
#[test]
fn svg_transform_list_and_nested_groups_follow_svg_order() {
    let source="<svg viewBox='0 0 100 100'><g transform='translate(10 20)'><g transform='scale(2)'><path transform='rotate(90,10,10)' d='M10 10L20 10' stroke='#000' stroke-width='3'/></g></g><path transform='scale(2) translate(10 20)' d='M0 0L10 0'/></svg>";
    let p = svg_import::preview(source).unwrap();
    for (actual, expected) in p.shapes[0]
        .geometry
        .points()
        .iter()
        .zip([[0.3, 0.4], [0.3, 0.6]])
    {
        assert!((actual[0] - expected[0]).abs() < 1e-12 && (actual[1] - expected[1]).abs() < 1e-12);
    }
    assert_eq!(p.shapes[0].style["stroke_width"], 6.);
    assert_eq!(p.shapes[1].geometry.points(), &[[0.2, 0.4], [0.4, 0.4]]);
    let p=svg_import::preview("<svg viewBox='0 0 100 100' transform='translate(100 0) scale(-1 1)'><path d='M10 10L20 20'/></svg>").unwrap();
    assert_eq!(p.shapes[0].geometry.points(), &[[0.9, 0.1], [0.8, 0.2]]);
}
#[test]
fn invalid_transforms_curves_and_precision_budget_are_rejected() {
    for transform in [
        "scale(2 3)",
        "scale(0)",
        "matrix(1 0 0 1 0 0)",
        "skewX(30)",
        "rotate(1 2)",
        "translate(NaN)",
        "scale(1e308) scale(1e308)",
        "translate(1),",
    ] {
        assert!(
            svg_import::preview(&format!(
                "<svg viewBox='0 0 100 100'><path transform='{transform}' d='M1 1L2 2'/></svg>"
            ))
            .is_err(),
            "{transform}"
        );
    }
    for path in [
        "M0 0 Q1 2",
        "M0 0 C1 2 3 4 5",
        "M0 0 Q1e 2 3 4",
        "M50 0Q50 -0.001 50 0",
        "M50 50C1e308 1e308 1e308 1e308 50 50",
        "M1 1ZL2 2",
        "M0 0Q1 2 3 4M5 6L7 8",
    ] {
        assert!(
            svg_import::preview(&format!(
                "<svg viewBox='0 0 100 100'><path d='{path}'/></svg>"
            ))
            .is_err(),
            "{path}"
        );
    }
    let path = format!("M10 10{}", " C90 10 10 90 10 10".repeat(1000));
    let error = svg_import::preview(&format!(
        "<svg viewBox='0 0 100 100'><path d='{path}'/></svg>"
    ))
    .unwrap_err();
    assert!(error.contains("4096"), "{error}");
}
#[test]
fn failed_transformed_curve_import_leaves_project_and_revision_unchanged() {
    let (mut p, mut r) = project("curves-atomic");
    let base = baseline(&p);
    let expected = r;
    let source="<svg viewBox='0 0 100 100'><path d='M10 10Q20 30 40 10'/><path transform='scale(2 3)' d='M1 1L2 2'/></svg>";
    assert!(svg_import::apply(
        &mut p,
        &mut r,
        "map",
        "curve",
        source,
        expected,
        base.clone()
    )
    .is_err());
    assert_eq!(baseline(&p), base);
    assert_eq!(r, expected);
}

#[test]
fn transformed_curve_uses_final_space_precision_and_correct_endpoints() {
    let source="<svg viewBox='0 0 100 50'><g transform='translate(20 5) scale(2)'><path d='M0 0 C20 0 0 20 20 20' stroke='#123' stroke-width='2'/></g></svg>";
    let preview = svg_import::preview(source).unwrap();
    let points = preview.shapes[0].geometry.points();
    assert_eq!(points.first(), Some(&[0.2, 0.1]));
    assert_eq!(points.last(), Some(&[0.6, 0.9]));
    for i in 0..=1000 {
        let t = f64::from(i) / 1000.;
        let x = 0.2 + 0.4 * (3. * (1. - t) * (1. - t) * t + t * t * t);
        let y = 0.1 + 0.8 * (3. * (1. - t) * t * t + t * t * t);
        assert!(distance_to_polyline([x, y], points) <= 0.00025 + 1e-12);
    }
}
#[test]
fn complete_import_has_a_total_point_budget() {
    let coordinates = (0..101)
        .map(|i| format!("{},1", i % 100))
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!(
        "<svg viewBox='0 0 100 100'>{}</svg>",
        format!("<polyline points='{coordinates}'/>").repeat(1000)
    );
    let error = svg_import::preview(&source).unwrap_err();
    assert!(error.contains("100000"), "{error}");
}

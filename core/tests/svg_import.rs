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
        "<path d='M0 0C1 2 3 4 5 6'/>",
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

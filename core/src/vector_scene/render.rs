use super::{
    validate_scene, view_box_transform, world_bounds, Affine, MapScene, PathSegment, SceneError,
    SceneGeometry, SceneLimits, ScenePublicLink, SceneStyle,
};
use crate::presentation::MapDocument;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

pub fn scene_to_safe_svg(scene: &MapScene, width: f64, height: f64) -> Result<String, SceneError> {
    render(
        scene,
        width,
        height,
        &scene.root_order.keys().cloned().collect::<Vec<_>>(),
        None,
        &BTreeMap::new(),
        None,
    )
}

pub fn to_safe_svg(
    map: &MapDocument,
    selected: Option<&BTreeSet<String>>,
) -> Result<String, SceneError> {
    to_safe_svg_with_links(map, selected, &BTreeMap::new())
}

pub fn to_safe_svg_with_links(
    map: &MapDocument,
    selected: Option<&BTreeSet<String>>,
    links: &BTreeMap<String, ScenePublicLink>,
) -> Result<String, SceneError> {
    let empty = MapScene::new(map.canvas.width as f64, map.canvas.height as f64);
    let scene = map.scene.as_ref().unwrap_or(&empty);
    let hidden: BTreeSet<_> = map
        .layers
        .values()
        .filter(|l| !l.visible_default)
        .map(|l| l.id.clone())
        .collect();
    render(
        scene,
        map.canvas.width as f64,
        map.canvas.height as f64,
        &map.layer_order,
        selected,
        links,
        Some(&hidden),
    )
}

pub fn to_safe_svg_layers_with_links(
    map: &MapDocument,
    selected: Option<&BTreeSet<String>>,
    links: &BTreeMap<String, ScenePublicLink>,
) -> Result<BTreeMap<String, String>, SceneError> {
    if selected.is_some_and(BTreeSet::is_empty) {
        return Ok(BTreeMap::new());
    }
    let Some(scene) = &map.scene else {
        return Ok(BTreeMap::new());
    };
    let width = map.canvas.width as f64;
    let height = map.canvas.height as f64;
    let context = Context::new(scene, width, height, selected, links)?;
    let mut result = BTreeMap::new();
    for layer in &map.layer_order {
        if selected.is_none() && map.layers.get(layer).is_some_and(|l| !l.visible_default) {
            continue;
        }
        if selected.is_some()
            && scene
                .root_order
                .get(layer)
                .is_none_or(|roots| roots.iter().all(|id| !context.needed.contains(id)))
        {
            continue;
        }
        let mut output = root(scene, width, height);
        context.defs(&mut output, Some(layer), None);
        if let Some(roots) = scene.root_order.get(layer) {
            for id in roots {
                context.node(id, &mut output)?;
            }
        }
        output.push_str("</svg>");
        result.insert(layer.clone(), output);
    }
    Ok(result)
}

fn render(
    scene: &MapScene,
    width: f64,
    height: f64,
    order: &[String],
    selected: Option<&BTreeSet<String>>,
    links: &BTreeMap<String, ScenePublicLink>,
    hidden: Option<&BTreeSet<String>>,
) -> Result<String, SceneError> {
    let context = Context::new(scene, width, height, selected, links)?;
    let mut output = root(scene, width, height);
    context.defs(&mut output, None, hidden);
    for layer in order {
        if selected.is_none() && hidden.is_some_and(|h| h.contains(layer)) {
            continue;
        }
        if let Some(roots) = scene.root_order.get(layer) {
            for id in roots {
                context.node(id, &mut output)?;
            }
        }
    }
    output.push_str("</svg>");
    Ok(output)
}

fn root(scene: &MapScene, width: f64, height: f64) -> String {
    let [x, y, w, h] = scene.view_box;
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{x} {y} {w} {h}\" preserveAspectRatio=\"{}\">",escape(&scene.preserve_aspect_ratio))
}

struct Context<'a> {
    scene: &'a MapScene,
    selected: Option<&'a BTreeSet<String>>,
    needed: BTreeSet<String>,
    clips: BTreeMap<String, usize>,
    links: &'a BTreeMap<String, ScenePublicLink>,
}
impl<'a> Context<'a> {
    fn new(
        scene: &'a MapScene,
        width: f64,
        height: f64,
        selected: Option<&'a BTreeSet<String>>,
        links: &'a BTreeMap<String, ScenePublicLink>,
    ) -> Result<Self, SceneError> {
        validate_scene(scene, &SceneLimits::default())?;
        world_bounds::viewport(
            scene,
            view_box_transform(scene.view_box, width, height, &scene.preserve_aspect_ratio)?,
        )?;
        let mut anchors = BTreeSet::new();
        for link in links.values() {
            if link.anchor.is_empty()
                || !link
                    .anchor
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                || !anchors.insert(&link.anchor)
            {
                return Err(SceneError::new(
                    "SCENE_REFERENCE",
                    "公开 SVG anchor 无效或重复",
                ));
            }
            if link.href.as_deref().is_some_and(|href| {
                href.is_empty()
                    || href.starts_with('/')
                    || href.contains([':', '\\'])
                    || href.chars().any(char::is_control)
            }) {
                return Err(SceneError::new(
                    "SCENE_REFERENCE",
                    "公开 SVG 链接必须为安全包内相对路径",
                ));
            }
        }
        let mut needed = BTreeSet::new();
        if let Some(selected) = selected {
            for id in selected {
                let mut current = Some(id.as_str());
                while let Some(id) = current {
                    let Some(node) = scene.nodes.get(id) else {
                        break;
                    };
                    if !needed.insert(id.into()) {
                        break;
                    }
                    current = node.parent_id.as_deref();
                }
            }
        }
        let clips = scene
            .nodes
            .values()
            .filter(|n| n.clip_rect.is_some() && (selected.is_none() || needed.contains(&n.id)))
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();
        Ok(Self {
            scene,
            selected,
            needed,
            clips,
            links,
        })
    }

    fn defs(&self, out: &mut String, layer: Option<&str>, hidden: Option<&BTreeSet<String>>) {
        let mut body = String::new();
        for (id, index) in &self.clips {
            let node = &self.scene.nodes[id];
            if layer.is_some_and(|l| node.layer_id != l)
                || (self.selected.is_some() && !self.needed.contains(id))
            {
                continue;
            }
            if self.selected.is_none() {
                if hidden.is_some_and(|layers| layers.contains(&node.layer_id)) {
                    continue;
                }
                let mut ancestor = Some(id.as_str());
                let mut visible = true;
                while let Some(current) = ancestor {
                    let n = &self.scene.nodes[current];
                    visible &= n.visible;
                    ancestor = n.parent_id.as_deref();
                }
                if !visible {
                    continue;
                }
            }
            let [x, y, w, h] = node.clip_rect.unwrap();
            let _=write!(body,"<clipPath id=\"wl-viewport-{index}\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"/></clipPath>");
        }
        if !body.is_empty() {
            out.push_str("<defs>");
            out.push_str(&body);
            out.push_str("</defs>");
        }
    }

    fn node(&self, id: &str, out: &mut String) -> Result<(), SceneError> {
        let node = &self.scene.nodes[id];
        if self.selected.is_some() && !self.needed.contains(id)
            || self.selected.is_none() && !node.visible
        {
            return Ok(());
        }
        let explicit = self.selected.is_none_or(|ids| ids.contains(id));
        let link = explicit.then(|| self.links.get(id)).flatten();
        if let Some(link) = link {
            let _ = write!(out, "<a id=\"{}\"", escape(&link.anchor));
            if let Some(href) = &link.href {
                let _ = write!(out, " href=\"{}\"", escape(href));
            }
            let _ = write!(out, "><title>{}</title>", escape(&link.label));
        }
        let is_group = matches!(node.geometry, SceneGeometry::Group { .. });
        let transparent = is_group
            && node.extra.get("svg_root").and_then(|v| v.as_bool()) == Some(true)
            && node.transform == Affine::IDENTITY
            && node.clip_rect.is_none()
            && node.style == SceneStyle::default();
        let mut attributes = String::new();
        if node.transform != Affine::IDENTITY {
            matrix_attr(&mut attributes, node.transform);
        }
        append_style(&mut attributes, &node.style);
        if let Some(index) = self.clips.get(id) {
            let _ = write!(
                attributes,
                " data-worldline-viewport=\"1\" clip-path=\"url(#wl-viewport-{index})\""
            );
        }
        match &node.geometry {
            SceneGeometry::Group { children } => {
                if !transparent {
                    out.push_str("<g");
                    out.push_str(&attributes);
                    out.push('>');
                }
                for child in children {
                    self.node(child, out)?;
                }
                if !transparent {
                    out.push_str("</g>");
                }
            }
            geometry if explicit => {
                let mut shape = String::new();
                geometry_svg(&mut shape, geometry);
                let at = shape.find([' ', '/', '>']).unwrap_or(shape.len());
                out.push_str(&shape[..at]);
                out.push_str(&attributes);
                out.push_str(&shape[at..]);
            }
            _ => {}
        }
        if link.is_some() {
            out.push_str("</a>");
        }
        Ok(())
    }
}

pub(super) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn matrix_attr(out: &mut String, matrix: Affine) {
    let [a, b, c, d, e, f] = matrix.0;
    let _ = write!(out, " transform=\"matrix({a} {b} {c} {d} {e} {f})\"");
}

fn append_style(out: &mut String, style: &SceneStyle) {
    for (key, value) in [
        ("fill", &style.fill),
        ("stroke", &style.stroke),
        ("fill-rule", &style.fill_rule),
        ("stroke-linecap", &style.line_cap),
        ("stroke-linejoin", &style.line_join),
        ("font-family", &style.font_family),
        ("font-weight", &style.font_weight),
        ("font-style", &style.font_style),
        ("text-anchor", &style.text_anchor),
    ] {
        if let Some(value) = value {
            let _ = write!(out, " {key}=\"{}\"", escape(value));
        }
    }
    for (key, value) in [
        ("stroke-width", style.stroke_width),
        ("opacity", style.opacity),
        ("fill-opacity", style.fill_opacity),
        ("stroke-opacity", style.stroke_opacity),
        ("stroke-miterlimit", style.miter_limit),
        ("font-size", style.font_size),
    ] {
        if let Some(value) = value {
            let _ = write!(out, " {key}=\"{value}\"");
        }
    }
}

fn geometry_svg(out: &mut String, geometry: &SceneGeometry) {
    match geometry {
        SceneGeometry::Group { .. } => {}
        SceneGeometry::Point { position } => {
            let _ = write!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"5\"/>",
                position[0], position[1]
            );
        }
        SceneGeometry::Polyline { points } | SceneGeometry::Polygon { points } => {
            let tag = if matches!(geometry, SceneGeometry::Polygon { .. }) {
                "polygon"
            } else {
                "polyline"
            };
            let _ = write!(out, "<{tag} points=\"");
            for p in points {
                let _ = write!(out, "{},{} ", p[0], p[1]);
            }
            out.push_str("\"/>");
        }
        SceneGeometry::Rect {
            x,
            y,
            width,
            height,
            rx,
            ry,
        } => {
            let _=write!(out,"<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" rx=\"{rx}\" ry=\"{ry}\"/>");
        }
        SceneGeometry::Ellipse { cx, cy, rx, ry } => {
            let _ = write!(
                out,
                "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\"/>"
            );
        }
        SceneGeometry::Path { segments } => {
            out.push_str("<path d=\"");
            for segment in segments {
                match segment {
                    PathSegment::Move { to } => {
                        let _ = write!(out, "M{} {}", to[0], to[1]);
                    }
                    PathSegment::Line { to } => {
                        let _ = write!(out, "L{} {}", to[0], to[1]);
                    }
                    PathSegment::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        let _ = write!(
                            out,
                            "C{} {} {} {} {} {}",
                            control1[0], control1[1], control2[0], control2[1], to[0], to[1]
                        );
                    }
                    PathSegment::Quadratic { control, to } => {
                        let _ = write!(out, "Q{} {} {} {}", control[0], control[1], to[0], to[1]);
                    }
                    PathSegment::Arc {
                        rx,
                        ry,
                        rotation,
                        large_arc,
                        sweep,
                        to,
                    } => {
                        let _ = write!(
                            out,
                            "A{rx} {ry} {rotation} {} {} {} {}",
                            u8::from(*large_arc),
                            u8::from(*sweep),
                            to[0],
                            to[1]
                        );
                    }
                    PathSegment::Close => out.push('Z'),
                }
            }
            out.push_str("\"/>");
        }
        SceneGeometry::Text { x, y, runs } => {
            let _ = write!(out, "<text x=\"{x}\" y=\"{y}\" xml:space=\"preserve\">");
            for run in runs {
                out.push_str("<tspan");
                for (key, value) in [
                    ("x", run.x),
                    ("y", run.y),
                    ("dx", Some(run.dx)),
                    ("dy", Some(run.dy)),
                ] {
                    if let Some(value) = value {
                        let _ = write!(out, " {key}=\"{value}\"");
                    }
                }
                append_style(out, &run.style);
                let _ = write!(out, ">{}</tspan>", escape(&run.text));
            }
            out.push_str("</text>");
        }
    }
}

/// 完整地图的矢量交换，旧 normalized placements 临时投影，绝不持久复制。
pub fn map_to_safe_svg(
    map: &MapDocument,
    selected: Option<&BTreeSet<String>>,
) -> Result<String, SceneError> {
    let scene = super::legacy::scene_with_placements(map)?;
    let hidden = map
        .layers
        .values()
        .filter(|l| !l.visible_default)
        .map(|l| l.id.clone())
        .collect();
    render(
        &scene,
        map.canvas.width as f64,
        map.canvas.height as f64,
        &map.layer_order,
        selected,
        &BTreeMap::new(),
        Some(&hidden),
    )
}

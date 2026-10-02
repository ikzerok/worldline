//! 精确地图白名单与公开链接复用 core 场景 serializer。
use super::site::{html_escape, relative_url};
use super::*;
use crate::presentation::MapGeometry;
use crate::vector_scene::{SceneGeometry, ScenePublicLink};

pub(super) struct MapExportContext<'a> {
    pub project: &'a Project,
    pub compiled: &'a CompileResult,
    pub selection: &'a ReaderExportSelection,
    pub routes: &'a BTreeMap<TargetRef, String>,
    pub overrides: &'a [ReaderProfileRoute],
}

pub(super) fn append_maps(
    context: MapExportContext<'_>,
    pages: &mut Vec<PublicPage>,
    included: &mut Vec<ReaderExportIncluded>,
    progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
) -> Result<(), String> {
    let selection = context.selection;
    if selection.maps.is_empty() {
        return Ok(());
    }
    let index = context.project.map_index();
    if index
        .diagnostics
        .iter()
        .any(|d| d.severity == crate::Severity::Error)
    {
        return Err("地图存在结构错误，请修复后公开".into());
    }
    let mut map_routes = BTreeMap::new();
    for (i, choice) in selection.maps.iter().enumerate() {
        let route = super::routes::target_route(
            selection,
            &TargetRef::new("map", &choice.id),
            format!("maps/m{:04}.html", i + 1),
            context.overrides,
        )?;
        if map_routes.insert(choice.id.clone(), route).is_some() {
            return Err("公开地图不能重复".into());
        }
    }
    for (map_number, choice) in selection.maps.iter().enumerate() {
        super::progress::report(progress, "maps", map_number, selection.maps.len())?;
        let map = index.maps.get(&choice.id).ok_or("公开地图不存在")?;
        let route = &map_routes[&choice.id];
        let selected_rasters: BTreeSet<_> = choice.raster_layers.iter().cloned().collect();
        let selected: BTreeSet<_> = choice.placements.iter().cloned().collect();
        if selected_rasters.len() != choice.raster_layers.len()
            || selected.len() != choice.placements.len()
        {
            return Err("公开地图标记或底图层不能重复".into());
        }
        if selected_rasters
            .iter()
            .any(|id| !map.raster_layers.iter().any(|layer| layer.id == *id))
            || selected.iter().any(|id| {
                !map.placements.contains_key(id)
                    && !map
                        .scene
                        .as_ref()
                        .is_some_and(|scene| scene.nodes.contains_key(id))
            })
        {
            return Err("公开地图标记或底图层不存在".into());
        }
        let item_context = MapItemContext {
            export: &context,
            current: route,
            map_routes: &map_routes,
            map_id: &choice.id,
        };
        let mut svg = format!("<div class=\"map-controls\"><button data-map-action=\"in\">放大</button><button data-map-action=\"out\">缩小</button><button data-map-action=\"reset\">重置</button></div><svg class=\"reader-map\" xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" role=\"img\" aria-label=\"{}\" style=\"width:100%;height:auto;border:1px solid #888\">", map.canvas.width, map.canvas.height, html_escape(&map.title));
        for layer in map
            .raster_layers
            .iter()
            .filter(|layer| selected_rasters.contains(&layer.id))
        {
            let asset_route = context
                .routes
                .get(&layer.asset)
                .ok_or("公开底图必须另行选择其附件")?;
            if !["png", "jpg", "jpeg", "webp", "gif", "bmp"]
                .iter()
                .any(|ext| asset_route.ends_with(&format!(".{ext}")))
            {
                return Err("公开底图必须为安全图像附件".into());
            }
            let [x0, y0, x1, y1] = layer.rect;
            svg.push_str(&format!(
                "<image href=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                html_escape(&relative_url(route, asset_route)),
                x0 * f64::from(map.canvas.width),
                y0 * f64::from(map.canvas.height),
                (x1 - x0) * f64::from(map.canvas.width),
                (y1 - y0) * f64::from(map.canvas.height)
            ));
        }
        let mut details = String::from("<ul class=\"map-details\">");
        let mut text = String::new();
        let mut anchors = Vec::new();
        let mut links = BTreeMap::new();
        let mut selected_scene = BTreeSet::new();
        let mut has_geometry = !selected_rasters.is_empty();
        if let Some(scene) = &map.scene {
            for (number, node) in scene
                .nodes
                .values()
                .filter(|node| selected.contains(&node.id))
                .enumerate()
            {
                if number.is_multiple_of(64) {
                    super::progress::report(progress, "maps", number, selected.len())?;
                }
                selected_scene.insert(node.id.clone());
                let label = public_label(
                    &context,
                    node.label_override.as_deref(),
                    node.target_ref.as_ref(),
                    "地图图元",
                );
                let item = item(
                    &node.id,
                    label,
                    &node.annotation,
                    node.target_ref.as_ref(),
                    node.navigation.as_ref().map(|nav| nav.map_id.as_str()),
                    &item_context,
                );
                links.insert(
                    node.id.clone(),
                    ScenePublicLink {
                        href: item.object_link.clone().or_else(|| item.map_link.clone()),
                        anchor: item.anchor.clone(),
                        label: item.label.clone(),
                    },
                );
                let body_text = if let SceneGeometry::Text { runs, .. } = &node.geometry {
                    runs.iter()
                        .map(|run| run.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    String::new()
                };
                has_geometry |= !matches!(node.geometry, SceneGeometry::Group { .. });
                append_details(&mut details, &mut text, &mut anchors, item, &body_text);
            }
        }
        let scene_layers =
            crate::vector_scene::to_safe_svg_layers_with_links(map, Some(&selected_scene), &links)
                .map_err(|e| format!("无法安全公开矢量场景：{e}"))?;
        let mut number = 0usize;
        for layer_id in &map.layer_order {
            for marker in map
                .placements
                .values()
                .filter(|marker| marker.layer_id == *layer_id && selected.contains(&marker.id))
            {
                if number.is_multiple_of(64) {
                    super::progress::report(progress, "maps", number, selected.len())?;
                }
                number += 1;
                let label = match &marker.geometry {
                    MapGeometry::Text { text, .. } => text.clone(),
                    _ => public_label(
                        &context,
                        marker.label_override.as_deref(),
                        marker.target_ref.as_ref(),
                        "地图标记",
                    ),
                };
                let item = item(
                    &marker.id,
                    label,
                    &marker.annotation,
                    marker.target_ref.as_ref(),
                    marker.navigation.as_ref().map(|nav| nav.map_id.as_str()),
                    &item_context,
                );
                let shape = super::map_geometry::geometry(
                    marker,
                    f64::from(map.canvas.width),
                    f64::from(map.canvas.height),
                );
                let contents = format!("<title>{}</title>{shape}", html_escape(&item.label));
                svg.push_str(&format!("<g id=\"{}\">", item.anchor));
                if let Some(href) = item.object_link.as_ref().or(item.map_link.as_ref()) {
                    svg.push_str(&format!("<a href=\"{}\">{contents}</a>", html_escape(href)));
                } else {
                    svg.push_str(&contents);
                }
                svg.push_str("</g>");
                append_details(&mut details, &mut text, &mut anchors, item, "");
                has_geometry = true;
            }
            if let Some(fragment) = scene_layers.get(layer_id) {
                svg.push_str(fragment);
            }
        }
        svg.push_str("</svg>");
        details.push_str("</ul>");
        pages.push(PublicPage {
            title: map.title.clone(),
            output_path: route.into(),
            body_html: format!("{svg}{details}"),
            searchable_text: text,
            kind: "map".into(),
            aliases: Vec::new(),
            anchors,
            empty_content: !has_geometry,
        });
        included.push(ReaderExportIncluded {
            target: Some(TargetRef::new("map", &choice.id)),
            manuscript_id: None,
            chapter_id: None,
            title: map.title.clone(),
            output_path: route.clone(),
        });
    }
    Ok(())
}

fn public_label(
    context: &MapExportContext<'_>,
    explicit: Option<&str>,
    target: Option<&TargetRef>,
    fallback: &str,
) -> String {
    explicit
        .map(str::to_owned)
        .or_else(|| {
            if context.selection.schema_version != READER_SITE_SCHEMA_VERSION {
                return None;
            }
            let target = target?;
            context.routes.get(target)?;
            Some(
                context
                    .compiled
                    .analysis
                    .catalog
                    .object(target)?
                    .display
                    .clone(),
            )
        })
        .unwrap_or_else(|| fallback.into())
}

struct MapItemContext<'a, 'b> {
    export: &'a MapExportContext<'b>,
    current: &'a str,
    map_routes: &'a BTreeMap<String, String>,
    map_id: &'a str,
}
struct PublicMapItem {
    label: String,
    annotation: String,
    anchor: String,
    object_link: Option<String>,
    map_link: Option<String>,
    has_private_link: bool,
}
fn item(
    id: &str,
    label: String,
    annotation: &str,
    target: Option<&TargetRef>,
    child_map: Option<&str>,
    context: &MapItemContext<'_, '_>,
) -> PublicMapItem {
    let object_link = target
        .and_then(|target| context.export.routes.get(target))
        .map(|route| relative_url(context.current, route));
    let map_link = child_map
        .and_then(|id| context.map_routes.get(id))
        .map(|route| relative_url(context.current, route));
    let has_private_link =
        (target.is_some() && object_link.is_none()) || (child_map.is_some() && map_link.is_none());
    PublicMapItem {
        label,
        annotation: annotation.into(),
        anchor: super::routes::public_anchor(context.map_id, id),
        object_link,
        map_link,
        has_private_link,
    }
}
fn append_details(
    details: &mut String,
    text: &mut String,
    anchors: &mut Vec<PublicAnchor>,
    item: PublicMapItem,
    body_text: &str,
) {
    details.push_str(&format!(
        "<li><strong>{}</strong> {} {}",
        html_escape(&item.label),
        html_escape(&item.annotation),
        html_escape(body_text)
    ));
    for (label, href) in [
        ("阅读资料", item.object_link),
        ("进入子地图", item.map_link),
    ] {
        if let Some(href) = href {
            details.push_str(&format!(" <a href=\"{}\">{label}</a>", html_escape(&href)));
        }
    }
    if item.has_private_link {
        details.push_str(" 未公开内容");
    }
    details.push_str("</li>");
    let plain = format!("{} {} {}", item.label, item.annotation, body_text);
    text.push_str(&plain);
    text.push('\n');
    anchors.push(PublicAnchor {
        id: item.anchor,
        label: item.label,
        text: plain,
    });
}

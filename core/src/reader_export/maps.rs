//! 仅从显式公开的地图几何生成 SVG；原始 JSON 和样式绝不进入 HTML。
use super::site::{html_escape, relative_url};
use super::*;
use crate::presentation::{MapGeometry, MapPlacement};

pub(super) fn append_maps(
    project: &Project,
    selection: &ReaderExportSelection,
    routes: &BTreeMap<TargetRef, String>,
    pages: &mut Vec<PublicPage>,
    included: &mut Vec<ReaderExportIncluded>,
) -> Result<(), String> {
    if selection.maps.is_empty() {
        return Ok(());
    }
    if selection.maps.len() > 100 {
        return Err("公开地图数量超过 100".into());
    }
    let index = project.map_index();
    if index
        .diagnostics
        .iter()
        .any(|d| d.severity == crate::Severity::Error)
    {
        return Err("地图存在结构错误，请修复后公开".into());
    }
    let mut map_routes = BTreeMap::new();
    for (i, choice) in selection.maps.iter().enumerate() {
        if map_routes
            .insert(choice.id.clone(), format!("maps/m{:04}.html", i + 1))
            .is_some()
        {
            return Err("公开地图不能重复".into());
        }
    }
    for choice in &selection.maps {
        let map = index.maps.get(&choice.id).ok_or("公开地图不存在")?;
        let route = &map_routes[&choice.id];
        if choice.placements.len() > 5000 || choice.raster_layers.len() > 128 {
            return Err("公开地图标记或底图层超过限制".into());
        }
        let selected_rasters: BTreeSet<_> = choice.raster_layers.iter().collect();
        let selected_markers: BTreeSet<_> = choice.placements.iter().collect();
        if selected_rasters.len() != choice.raster_layers.len()
            || selected_markers.len() != choice.placements.len()
        {
            return Err("公开地图标记或底图层不能重复".into());
        }
        if selected_rasters
            .iter()
            .any(|id| !map.raster_layers.iter().any(|layer| &layer.id == *id))
            || selected_markers
                .iter()
                .any(|id| !map.placements.contains_key(*id))
        {
            return Err("公开地图标记或底图层不存在".into());
        }
        let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" role=\"img\" aria-label=\"{}\" style=\"width:100%;height:auto;border:1px solid #888\">", map.canvas.width, map.canvas.height, html_escape(&map.title));
        // Selection is authorization, never a new visual stacking order.
        for layer in map
            .raster_layers
            .iter()
            .filter(|layer| selected_rasters.contains(&layer.id))
        {
            let asset_route = routes
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
        let mut details = String::from("<ul>");
        let mut text = String::new();
        let selected_markers = &selected_markers;
        let markers = map.layer_order.iter().flat_map(|layer_id| {
            map.placements.values().filter(move |marker| {
                &marker.layer_id == layer_id && selected_markers.contains(&marker.id)
            })
        });
        for marker in markers {
            let label = match &marker.geometry {
                MapGeometry::Text { text, .. } => text.as_str(),
                _ => marker.label_override.as_deref().unwrap_or("地图标记"),
            };
            let label = html_escape(label);
            let object_route = marker
                .target_ref
                .as_ref()
                .and_then(|target| routes.get(target));
            let child_route = marker
                .navigation
                .as_ref()
                .and_then(|nav| map_routes.get(&nav.map_id));
            let link = object_route.or(child_route);
            let shape = geometry(
                marker,
                f64::from(map.canvas.width),
                f64::from(map.canvas.height),
            );
            if let Some(link) = link {
                svg.push_str(&format!(
                    "<a href=\"{}\"><title>{}</title>{}</a>",
                    html_escape(&relative_url(route, link)),
                    label,
                    shape
                ));
            } else {
                svg.push_str(&format!("<g><title>{label}</title>{shape}</g>"));
            }
            details.push_str(&format!(
                "<li><strong>{label}</strong> {}",
                html_escape(&marker.annotation)
            ));
            text.push_str(match &marker.geometry {
                MapGeometry::Text { text, .. } => text,
                _ => marker.label_override.as_deref().unwrap_or("地图标记"),
            });
            text.push(' ');
            text.push_str(&marker.annotation);
            text.push(' ');
            if let Some(link) = object_route {
                details.push_str(&format!(
                    " <a href=\"{}\">阅读资料</a>",
                    html_escape(&relative_url(route, link))
                ));
            } else if marker.target_ref.is_some() {
                details.push_str(" 未公开内容");
            }
            if let Some(link) = child_route {
                details.push_str(&format!(
                    " <a href=\"{}\">进入子地图</a>",
                    html_escape(&relative_url(route, link))
                ));
            } else if marker.navigation.is_some() {
                details.push_str(" 未公开内容");
            }
            details.push_str("</li>");
        }
        svg.push_str("</svg>");
        details.push_str("</ul>");
        pages.push(PublicPage {
            title: map.title.clone(),
            output_path: route.into(),
            body_html: format!("{svg}{details}"),
            searchable_text: text,
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

fn geometry(marker: &MapPlacement, width: f64, height: f64) -> String {
    let color = |name: &str, fallback: &str| {
        marker
            .style
            .as_ref()
            .and_then(|s| s.get(name))
            .and_then(|v| v.as_str())
            .filter(|v| {
                *v == "none"
                    || (v.len() == 7
                        && v.starts_with('#')
                        && v[1..].bytes().all(|c| c.is_ascii_hexdigit()))
            })
            .unwrap_or(fallback)
            .to_owned()
    };
    let stroke = color("stroke", "#446688");
    let fill = color("fill", "#88aacc");
    let number = |name: &str, max: f64, fallback: f64| {
        marker
            .style
            .as_ref()
            .and_then(|s| s.get(name))
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite() && (0.0..=max).contains(v))
            .unwrap_or(fallback)
    };
    let stroke_width = number("stroke_width", 100.0, 2.0);
    let stroke_opacity = number("stroke_opacity", 1.0, 1.0);
    let fill_opacity = number("fill_opacity", 1.0, 1.0);
    let style = format!("fill=\"{fill}\" fill-opacity=\"{fill_opacity}\" stroke=\"{stroke}\" stroke-opacity=\"{stroke_opacity}\" stroke-width=\"{stroke_width}\"");
    let points = marker
        .geometry
        .points()
        .iter()
        .map(|p| format!("{},{}", p[0] * width, p[1] * height))
        .collect::<Vec<_>>()
        .join(" ");
    match &marker.geometry {
        MapGeometry::Point { position } => format!("<circle cx=\"{}\" cy=\"{}\" r=\"5\" {style}/>", position[0] * width, position[1] * height),
        MapGeometry::Polyline { .. } => format!("<polyline points=\"{points}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" stroke-opacity=\"{stroke_opacity}\"/>"),
        MapGeometry::Text { position, text, font_size, color } => {
            let x = position[0] * width;
            let y = position[1] * height + font_size;
            let lines = text.split('\n').enumerate().map(|(i, line)| format!("<tspan x=\"{x}\" y=\"{}\">{}</tspan>", y + i as f64 * font_size * 1.2, html_escape(line))).collect::<String>();
            format!("<text xml:space=\"preserve\" font-family=\"sans-serif\" font-size=\"{font_size}\" fill=\"{color}\">{lines}</text>")
        }
        MapGeometry::Polygon { .. } => format!("<polygon points=\"{points}\" {style}/>"),
    }
}

use super::site::html_escape;
use crate::presentation::{MapGeometry, MapPlacement};

pub(super) fn geometry(marker: &MapPlacement, width: f64, height: f64) -> String {
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

//! 受限 SVG 到原生地图几何的转换；绝不保存可执行的 SVG 文档。
mod geometry;
mod path;
mod transaction;
mod transform;
use crate::presentation::MapGeometry;
pub use crate::vector_scene::SvgScenePreview;
use quick_xml::{events::Event, Reader};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
pub use transaction::apply;

#[derive(Clone, Debug)]
pub struct SvgShape {
    pub geometry: MapGeometry,
    pub style: Map<String, Value>,
}
#[derive(Clone, Debug)]
pub struct SvgPreview {
    pub shapes: Vec<SvgShape>,
    pub width: f64,
    pub height: f64,
}
/// 旧采样 placement 兼容入口，不承诺新版可编辑曲线保真。
/// 新作者入口请使用 preview_scene 与 vector_scene::SceneBatch。
/// 解析上限约束 CPU/内存；不解析实体、CSS、链接或嵌入内容。
pub fn preview(source: &str) -> Result<SvgPreview, String> {
    if source.len() > 2 * 1024 * 1024 {
        return Err("SVG 超过 2 MiB 上限".into());
    }
    let mut reader = Reader::from_str(source);
    let mut stack: Vec<(String, BTreeMap<String, String>, transform::Transform)> = Vec::new();
    let mut bounds = None;
    let mut shapes = Vec::new();
    let mut total_points = 0;
    let mut closed = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| format!("SVG XML 无效：{e}"))?
        {
            Event::Start(e) | Event::Empty(e) => {
                let empty =
                    source.as_bytes().get(reader.buffer_position() as usize - 2) == Some(&b'/');
                let tag = std::str::from_utf8(e.name().as_ref())
                    .map_err(|_| "SVG 标签无效")?
                    .to_string();
                if closed
                    || (stack.is_empty() && tag != "svg")
                    || (!stack.is_empty() && tag == "svg")
                {
                    return Err("SVG 必须只有一个根元素".into());
                }
                if !matches!(
                    tag.as_str(),
                    "svg"
                        | "g"
                        | "rect"
                        | "circle"
                        | "ellipse"
                        | "line"
                        | "polyline"
                        | "polygon"
                        | "path"
                ) {
                    return Err(format!("不支持 SVG 元素：{tag}"));
                }
                if stack
                    .last()
                    .is_some_and(|(name, _, _)| name != "svg" && name != "g")
                {
                    return Err("SVG 图形不能包含子元素".into());
                }
                let mut attrs = BTreeMap::new();
                for attr in e.attributes() {
                    let attr = attr.map_err(|e| format!("SVG 属性无效：{e}"))?;
                    let key = std::str::from_utf8(attr.key.as_ref())
                        .map_err(|_| "SVG 属性无效")?
                        .to_string();
                    let value = std::str::from_utf8(&attr.value)
                        .map_err(|_| "SVG 属性无效")?
                        .to_string();
                    if value.contains('&') {
                        return Err("SVG 不支持实体引用".into());
                    }
                    let allowed = matches!(
                        key.as_str(),
                        "id" | "transform"
                            | "fill"
                            | "stroke"
                            | "stroke-width"
                            | "fill-opacity"
                            | "stroke-opacity"
                    ) || match tag.as_str() {
                        "svg" => matches!(
                            key.as_str(),
                            "xmlns" | "width" | "height" | "viewBox" | "version"
                        ),
                        "rect" => matches!(key.as_str(), "x" | "y" | "width" | "height"),
                        "circle" => matches!(key.as_str(), "cx" | "cy" | "r"),
                        "ellipse" => matches!(key.as_str(), "cx" | "cy" | "rx" | "ry"),
                        "line" => matches!(key.as_str(), "x1" | "y1" | "x2" | "y2"),
                        "polyline" | "polygon" => key == "points",
                        "path" => key == "d",
                        _ => false,
                    };
                    if !allowed {
                        return Err(format!("不支持 SVG 属性：{key}"));
                    }
                    if key == "xmlns" && value != "http://www.w3.org/2000/svg" {
                        return Err("SVG 命名空间不受支持".into());
                    }
                    attrs.insert(key, value);
                }
                let mut style = stack.last().map(|(_, s, _)| s.clone()).unwrap_or_else(|| {
                    BTreeMap::from([
                        ("fill".into(), "#000000".into()),
                        ("stroke".into(), "none".into()),
                        ("stroke-width".into(), "1".into()),
                    ])
                });
                for key in [
                    "fill",
                    "stroke",
                    "stroke-width",
                    "fill-opacity",
                    "stroke-opacity",
                ] {
                    if let Some(value) = attrs.get(key) {
                        style.insert(key.into(), value.clone());
                    }
                }
                let local_transform = attrs
                    .get("transform")
                    .map(|value| transform::parse(value))
                    .transpose()?
                    .unwrap_or(transform::Transform::IDENTITY);
                let transform = stack
                    .last()
                    .map(|(_, _, t)| *t)
                    .unwrap_or(transform::Transform::IDENTITY)
                    .then(local_transform)?;
                let mut style_json = style_value(&style)?;
                let width = style_json["stroke_width"].as_f64().unwrap() * transform.scale();
                if !width.is_finite() || !(0.0..=100.0).contains(&width) {
                    return Err("SVG 变换后的描边宽度须在 0–100 之间".into());
                }
                style_json.insert("stroke_width".into(), json!(width));
                if tag == "svg" {
                    bounds = Some(geometry::bounds(&attrs)?);
                }
                if tag != "svg" && tag != "g" {
                    let geometry =
                        geometry::parse(&tag, &attrs, bounds.ok_or("缺少 SVG 坐标系")?, transform)?;
                    total_points += geometry.points().len();
                    if total_points > 100_000 {
                        return Err("SVG 总点数超过 100000 上限".into());
                    }
                    shapes.push(SvgShape {
                        geometry,
                        style: style_json,
                    });
                    if shapes.len() > 1000 {
                        return Err("SVG 图形超过 1000 个上限".into());
                    }
                }
                if !empty {
                    stack.push((tag, style, transform));
                    if stack.len() > 32 {
                        return Err("SVG 嵌套过深".into());
                    }
                } else if tag == "svg" {
                    closed = true;
                }
            }
            Event::End(e) => {
                let name = e.name();
                let tag = std::str::from_utf8(name.as_ref()).map_err(|_| "SVG 标签无效")?;
                if stack.pop().is_none_or(|(name, _, _)| name != tag) {
                    return Err("SVG 标签未匹配".into());
                }
                if stack.is_empty() {
                    closed = true;
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::Comment(_) => {}
            Event::Text(e) if e.as_ref().iter().all(u8::is_ascii_whitespace) => {}
            _ => return Err("SVG 不支持文本、实体、处理指令或活动内容".into()),
        }
    }
    if !closed || !stack.is_empty() || shapes.is_empty() {
        return Err("SVG 未闭合或没有支持的图形".into());
    }
    let [_, _, width, height] = bounds.ok_or("缺少 SVG 坐标系")?;
    Ok(SvgPreview {
        shapes,
        width,
        height,
    })
}
fn style_value(style: &BTreeMap<String, String>) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for key in ["fill", "stroke"] {
        let value = &style[key];
        let color = if value == "none" {
            value.clone()
        } else if let Some(hex) = value.strip_prefix('#').filter(|hex| {
            (hex.len() == 3 || hex.len() == 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
        }) {
            if hex.len() == 3 {
                format!("#{}", hex.chars().flat_map(|c| [c, c]).collect::<String>())
            } else {
                value.clone()
            }
        } else {
            return Err(format!("SVG {key} 仅支持 #RGB、#RRGGBB 或 none"));
        };
        out.insert(key.into(), json!(color));
    }
    let width = geometry::number(&style["stroke-width"])?;
    if !(0.0..=100.0).contains(&width) {
        return Err("SVG 描边宽度须在 0–100 之间".into());
    }
    out.insert("stroke_width".into(), json!(width));
    for key in ["fill-opacity", "stroke-opacity"] {
        let alpha = style
            .get(key)
            .map(|v| geometry::number(v))
            .transpose()?
            .unwrap_or(1.0);
        if !(0.0..=1.0).contains(&alpha) {
            return Err("SVG 透明度须在 0–1 之间".into());
        }
        out.insert(key.replace('-', "_"), json!(alpha));
    }
    Ok(out)
}

/// 完整受控 SVG profile 的只读预览；曲线和根 viewport 裁剪保留为 typed scene。
pub fn preview_scene(
    source: &str,
) -> Result<crate::vector_scene::SvgScenePreview, crate::vector_scene::SceneError> {
    crate::vector_scene::preview_scene(source)
}

/// 有预算与取消进度的只读场景预检；false 取消，原输入由调用方保留。
pub fn preview_scene_with_control(
    source: &str,
    limits: &crate::vector_scene::SceneLimits,
    progress: &mut dyn FnMut(crate::vector_scene::SceneProgress) -> bool,
) -> Result<crate::vector_scene::SvgScenePreview, crate::vector_scene::SceneError> {
    crate::vector_scene::preview_scene_with_control(source, limits, progress)
}

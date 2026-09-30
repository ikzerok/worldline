//! 坐标转换与直线路径解析；拒绝不支持命令，避免静默损失图形。
use super::*;
pub(super) fn number(text: &str) -> Result<f64, String> {
    let value = text
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("SVG 数字无效：{text}"))?;
    if !value.is_finite() {
        return Err("SVG 数字必须有限".into());
    }
    Ok(value)
}
fn nums(text: &str) -> Result<Vec<f64>, String> {
    text.split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|v| !v.is_empty())
        .map(number)
        .collect()
}
fn get(attrs: &BTreeMap<String, String>, key: &str, default: f64) -> Result<f64, String> {
    attrs.get(key).map(|v| number(v)).unwrap_or(Ok(default))
}
pub(super) fn bounds(attrs: &BTreeMap<String, String>) -> Result<[f64; 4], String> {
    let b = if let Some(v) = attrs.get("viewBox") {
        let v = nums(v)?;
        if v.len() != 4 {
            return Err("viewBox 必须有四个数字".into());
        }
        [v[0], v[1], v[2], v[3]]
    } else {
        [0., 0., get(attrs, "width", 0.)?, get(attrs, "height", 0.)?]
    };
    if b[2] <= 0. || b[3] <= 0. || b[2] > 1_000_000. || b[3] > 1_000_000. {
        return Err("SVG 宽高必须在 0–1000000 之间".into());
    }
    Ok(b)
}
pub(super) fn parse(
    tag: &str,
    a: &BTreeMap<String, String>,
    b: [f64; 4],
    transform: super::transform::Transform,
) -> Result<MapGeometry, String> {
    let (mut points, closed) = match tag {
        "rect" => {
            let x = get(a, "x", 0.)?;
            let y = get(a, "y", 0.)?;
            let w = get(a, "width", 0.)?;
            let h = get(a, "height", 0.)?;
            if w <= 0. || h <= 0. {
                return Err("矩形宽高必须为正".into());
            }
            (vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true)
        }
        "circle" | "ellipse" => {
            let cx = get(a, "cx", 0.)?;
            let cy = get(a, "cy", 0.)?;
            let rx = get(a, if tag == "circle" { "r" } else { "rx" }, 0.)?;
            let ry = if tag == "circle" {
                rx
            } else {
                get(a, "ry", 0.)?
            };
            if rx <= 0. || ry <= 0. {
                return Err("圆或椭圆半径必须为正".into());
            }
            (
                (0..64)
                    .map(|i| {
                        let t = i as f64 * std::f64::consts::TAU / 64.;
                        [cx + rx * t.cos(), cy + ry * t.sin()]
                    })
                    .collect(),
                true,
            )
        }
        "line" => (
            vec![
                [get(a, "x1", 0.)?, get(a, "y1", 0.)?],
                [get(a, "x2", 0.)?, get(a, "y2", 0.)?],
            ],
            false,
        ),
        "polygon" | "polyline" => {
            let v = nums(a.get("points").ok_or("缺少 points")?)?;
            if v.len() % 2 != 0 {
                return Err("points 坐标须成对".into());
            }
            (v.as_chunks::<2>().0.to_vec(), tag == "polygon")
        }
        "path" => return super::path::parse(a.get("d").ok_or("缺少路径 d")?, b, transform),
        _ => return Err("不支持图形".into()),
    };
    if closed && points.first() == points.last() {
        points.pop();
    }
    if points.len() < if closed { 3 } else { 2 } || points.len() > 4096 {
        return Err("SVG 图形点数不符合要求（最多 4096）".into());
    }
    let points = points
        .into_iter()
        .map(|p| {
            let p = transform.point(p);
            let p = [(p[0] - b[0]) / b[2], (p[1] - b[1]) / b[3]];
            if p.iter()
                .any(|n| !n.is_finite() || *n < -1e-9 || *n > 1. + 1e-9)
            {
                Err("SVG 图形超出 viewBox，未进行裁剪".into())
            } else {
                Ok([p[0].clamp(0., 1.), p[1].clamp(0., 1.)])
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(if closed {
        MapGeometry::Polygon { points }
    } else {
        MapGeometry::Polyline { points }
    })
}

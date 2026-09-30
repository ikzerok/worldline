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
        "path" => path(a.get("d").ok_or("缺少路径 d")?)?,
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
fn path(source: &str) -> Result<(Vec<[f64; 2]>, bool), String> {
    // 命令和数值分词支持紧凑写法及指数；曲线/多子路径明确拒绝。
    let mut tokens = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_whitespace() || c == ',' {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_alphabetic() {
            i += 1;
        } else {
            if matches!(c, '+' | '-') {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                i += 1;
                if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                    i += 1;
                }
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        if start == i {
            return Err("SVG 路径包含无效字符".into());
        }
        tokens.push(&source[start..i]);
        if tokens.len() > 16384 {
            return Err("SVG 路径过长".into());
        }
    }
    let mut points = Vec::new();
    let mut current = [0., 0.];
    let mut command = ' ';
    let mut i = 0;
    let mut closed = false;
    while i < tokens.len() {
        if tokens[i].len() == 1 && tokens[i].as_bytes()[0].is_ascii_alphabetic() {
            command = tokens[i].chars().next().unwrap();
            i += 1;
            if !matches!(
                command,
                'M' | 'm' | 'L' | 'l' | 'H' | 'h' | 'V' | 'v' | 'Z' | 'z'
            ) {
                return Err(format!("暂不支持 SVG 路径命令 {command}（曲线请先转折线）"));
            }
            if matches!(command, 'Z' | 'z') {
                closed = true;
                if i != tokens.len() {
                    return Err("暂不支持多个 SVG 子路径".into());
                }
                break;
            }
        }
        if points.is_empty() && !matches!(command, 'M' | 'm') {
            return Err("SVG 路径须以 M 开始".into());
        }
        if !points.is_empty() && matches!(command, 'M' | 'm') {
            return Err("暂不支持多个 SVG 子路径".into());
        }
        let count = if matches!(command, 'H' | 'h' | 'V' | 'v') {
            1
        } else {
            2
        };
        if i + count > tokens.len() {
            return Err("SVG 路径坐标不完整".into());
        }
        let x = number(tokens[i])?;
        let y = if count == 2 {
            number(tokens[i + 1])?
        } else {
            0.
        };
        i += count;
        current = match command {
            'M' | 'L' => [x, y],
            'm' | 'l' => [current[0] + x, current[1] + y],
            'H' => [x, current[1]],
            'h' => [current[0] + x, current[1]],
            'V' => [current[0], x],
            'v' => [current[0], current[1] + x],
            _ => return Err("SVG 路径命令缺失".into()),
        };
        points.push(current);
        if command == 'M' {
            command = 'L';
        }
        if command == 'm' {
            command = 'l';
        }
    }
    Ok((points, closed))
}

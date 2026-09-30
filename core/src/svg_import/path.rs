//! 路径在最终归一化空间自适应细分；预算不足明确失败，不静默降精度。
use super::{geometry::number, transform::Transform};
use crate::presentation::MapGeometry;
const ERROR: f64 = 0.00025;
const MAX_POINTS: usize = 4096;
fn tokens(source: &str) -> Result<Vec<&str>, String> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_alphabetic() {
            i += 1;
        } else {
            if matches!(c, b'+' | b'-') {
                i += 1;
            }
            let mut digits = 0;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
                digits += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                    digits += 1;
                }
            }
            if digits == 0 {
                return Err("SVG 路径包含无效数字".into());
            }
            if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                i += 1;
                if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                    i += 1;
                }
                let exponent_start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i == exponent_start {
                    return Err("SVG 数字指数不完整".into());
                }
            }
        }
        out.push(&source[start..i]);
        if out.len() > 16384 {
            return Err("SVG 路径过长".into());
        }
    }
    Ok(out)
}
pub(super) fn numbers(source: &str) -> Result<Vec<f64>, String> {
    tokens(source)?.into_iter().map(number).collect()
}
fn normalized(p: [f64; 2], bounds: [f64; 4], transform: Transform) -> Result<[f64; 2], String> {
    let p = transform.point(p);
    let p = [
        (p[0] - bounds[0]) / bounds[2],
        (p[1] - bounds[1]) / bounds[3],
    ];
    if p.iter().any(|v| !v.is_finite()) {
        return Err("SVG 路径坐标或变换溢出".into());
    }
    Ok(p)
}
fn push(points: &mut Vec<[f64; 2]>, p: [f64; 2]) -> Result<(), String> {
    if points.len() >= MAX_POINTS {
        return Err("SVG 曲线精度要求超过每图形 4096 点预算".into());
    }
    if p.iter()
        .any(|v| !v.is_finite() || *v < -1e-9 || *v > 1. + 1e-9)
    {
        return Err("SVG 图形超出 viewBox，未进行裁剪".into());
    }
    points.push([p[0].clamp(0., 1.), p[1].clamp(0., 1.)]);
    Ok(())
}
fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0].hypot(d[1]);
    if length == 0. {
        return (p[0] - a[0]).hypot(p[1] - a[1]);
    }
    let unit = [d[0] / length, d[1] / length];
    let along = ((p[0] - a[0]) * unit[0] + (p[1] - a[1]) * unit[1]).clamp(0., length);
    (p[0] - a[0] - along * unit[0]).hypot(p[1] - a[1] - along * unit[1])
}
// 检查曲线的真实轴向极值，不能让细分容差掩盖轻微越界。
fn check_curve_bounds(control: &[[f64; 2]]) -> Result<(), String> {
    for axis in 0..2 {
        let magnitude = control.iter().map(|p| p[axis].abs()).fold(1., f64::max);
        let p = control
            .iter()
            .map(|p| p[axis] / magnitude)
            .collect::<Vec<_>>();
        let mut roots = Vec::new();
        if p.len() == 3 {
            let denominator = p[0] - 2. * p[1] + p[2];
            if denominator != 0. {
                roots.push((p[0] - p[1]) / denominator);
            }
        } else {
            let a = -p[0] + 3. * p[1] - 3. * p[2] + p[3];
            let b = 2. * (p[0] - 2. * p[1] + p[2]);
            let c = p[1] - p[0];
            if a == 0. {
                if b != 0. {
                    roots.push(-c / b);
                }
            } else {
                let discriminant = b * b - 4. * a * c;
                if discriminant >= 0. {
                    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
                    if q == 0. {
                        roots.push(-b / (2. * a));
                    } else {
                        roots.push(q / a);
                        roots.push(c / q);
                    }
                }
            }
        }
        for t in roots.into_iter().filter(|t| *t > 0. && *t < 1.) {
            let mut row = control.to_vec();
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
            if row[0]
                .iter()
                .any(|v| !v.is_finite() || *v < -1e-9 || *v > 1. + 1e-9)
            {
                return Err("SVG 曲线超出 viewBox，未进行裁剪".into());
            }
        }
    }
    Ok(())
}
fn flatten(control: &[[f64; 2]], depth: usize, out: &mut Vec<[f64; 2]>) -> Result<(), String> {
    let last = control[control.len() - 1];
    let error = control[1..control.len() - 1]
        .iter()
        .map(|p| distance(*p, control[0], last))
        .fold(0., f64::max);
    if error.is_finite() && error <= ERROR {
        return push(out, last);
    }
    if depth >= 16 {
        return Err("SVG 曲线无法在 16 层细分内满足精度，请简化路径".into());
    }
    let mut row = control.to_vec();
    let mut left = vec![row[0]];
    let mut right = vec![last];
    while row.len() > 1 {
        row = row
            .windows(2)
            .map(|p| [p[0][0] / 2. + p[1][0] / 2., p[0][1] / 2. + p[1][1] / 2.])
            .collect();
        left.push(row[0]);
        right.push(row[row.len() - 1]);
    }
    right.reverse();
    flatten(&left, depth + 1, out)?;
    flatten(&right, depth + 1, out)
}
pub(super) fn parse(
    source: &str,
    bounds: [f64; 4],
    transform: Transform,
) -> Result<MapGeometry, String> {
    let tokens = tokens(source)?;
    let mut points = Vec::new();
    let mut current = [0., 0.];
    let mut command = ' ';
    let mut i = 0;
    let mut closed = false;
    while i < tokens.len() {
        if tokens[i].len() == 1 && tokens[i].as_bytes()[0].is_ascii_alphabetic() {
            command = tokens[i].as_bytes()[0] as char;
            i += 1;
            if !matches!(
                command.to_ascii_uppercase(),
                'M' | 'L' | 'H' | 'V' | 'Z' | 'C' | 'Q'
            ) {
                return Err(format!(
                    "暂不支持 SVG 路径命令 {command}（仅支持 M/L/H/V/Z/C/Q）"
                ));
            }
            if command.eq_ignore_ascii_case(&'Z') {
                if points.is_empty() {
                    return Err("SVG 路径须以 M 开始".into());
                }
                if i != tokens.len() {
                    return Err("暂不支持多个 SVG 子路径".into());
                }
                closed = true;
                break;
            }
        }
        let upper = command.to_ascii_uppercase();
        if points.is_empty() && upper != 'M' {
            return Err("SVG 路径须以 M 开始".into());
        }
        if !points.is_empty() && upper == 'M' {
            return Err("暂不支持多个 SVG 子路径".into());
        }
        let count = match upper {
            'H' | 'V' => 1,
            'C' => 6,
            'Q' => 4,
            _ => 2,
        };
        if i + count > tokens.len() {
            return Err("SVG 路径坐标不完整".into());
        }
        let values = tokens[i..i + count]
            .iter()
            .map(|v| number(v))
            .collect::<Result<Vec<_>, _>>()?;
        i += count;
        let relative = command.is_ascii_lowercase();
        let position = |x: f64, y: f64| {
            if relative {
                [current[0] + x, current[1] + y]
            } else {
                [x, y]
            }
        };
        let endpoint = match upper {
            'H' => [
                if relative {
                    current[0] + values[0]
                } else {
                    values[0]
                },
                current[1],
            ],
            'V' => [
                current[0],
                if relative {
                    current[1] + values[0]
                } else {
                    values[0]
                },
            ],
            _ => position(values[count - 2], values[count - 1]),
        };
        if matches!(upper, 'C' | 'Q') {
            let mut control = vec![normalized(current, bounds, transform)?];
            for pair in values.as_chunks::<2>().0 {
                control.push(normalized(position(pair[0], pair[1]), bounds, transform)?);
            }
            check_curve_bounds(&control)?;
            flatten(&control, 0, &mut points)?;
        } else {
            push(&mut points, normalized(endpoint, bounds, transform)?)?;
        }
        current = endpoint;
        if command == 'M' {
            command = 'L';
        }
        if command == 'm' {
            command = 'l';
        }
    }
    if closed && points.first() == points.last() {
        points.pop();
    }
    if points.len() < if closed { 3 } else { 2 } {
        return Err("SVG 路径点数不足".into());
    }
    Ok(if closed {
        MapGeometry::Polygon { points }
    } else {
        MapGeometry::Polyline { points }
    })
}

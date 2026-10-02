//! 在任何渲染库展开 dash 前，按本地几何弧长上界计费。
use super::{PathSegment, SceneError, SceneGeometry, SceneStyle};

pub(super) fn work(
    geometry: &SceneGeometry,
    style: &SceneStyle,
    maximum: usize,
) -> Result<usize, SceneError> {
    if let SceneGeometry::Text { runs, .. } = geometry {
        for run in runs {
            if !run.text.is_empty() && active(&run.style.inherited(style)).is_some() {
                return Err(SceneError::new("SCENE_STYLE", "首版不支持文字/tspan 的非零虚线描边；请为文字片段显式选择实线（none），原输入保留"));
            }
        }
        return Ok(0);
    }
    if matches!(geometry, SceneGeometry::Group { .. }) {
        return Ok(0);
    }
    let Some((period, entries)) = active(style) else {
        return Ok(0);
    };
    let (length, subpaths, segments) = measure(geometry);
    // 后端使用 f32 局部路径：大坐标附近相邻端点舍入可把很短的线变长。
    // 同时包住控制点、圆角/圆弧生成的浮点运算误差，不能只计 f64 原路径。
    let rounding = (length + (coordinate_scale(geometry) + 1.0) * segments as f64)
        * f64::from(f32::EPSILON)
        * 128.0;
    let length = length + rounding;
    // 每子路径额外两周期覆盖任意相位、零长度项与 f32 舍入；不因隐藏/无stroke减免。
    let cycles = (length / period).ceil() + 2.0 * subpaths as f64;
    let cost = cycles * entries as f64 + segments as f64;
    if !cost.is_finite() || cost > maximum as f64 {
        return Err(super::validate::limit("虚线派生工作量"));
    }
    Ok(cost as usize)
}

fn active(style: &SceneStyle) -> Option<(f64, usize)> {
    let array = style.stroke_dasharray.as_ref()?;
    let sum: f64 = array.iter().sum();
    if sum == 0.0 {
        return None;
    }
    let repeat = if array.len() % 2 == 1 { 2 } else { 1 };
    Some((sum * repeat as f64, array.len() * repeat))
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn measure(geometry: &SceneGeometry) -> (f64, usize, usize) {
    match geometry {
        SceneGeometry::Group { .. } | SceneGeometry::Text { .. } => (0.0, 0, 0),
        SceneGeometry::Point { .. } => (40.0, 1, 4),
        SceneGeometry::Rect { width, height, .. } => (2.0 * (width + height), 1, 8),
        SceneGeometry::Ellipse { rx, ry, .. } => (8.0 * rx.max(*ry), 1, 4),
        SceneGeometry::Polyline { points } | SceneGeometry::Polygon { points } => {
            let mut length: f64 = points.windows(2).map(|p| distance(p[0], p[1])).sum();
            if matches!(geometry, SceneGeometry::Polygon { .. }) {
                if let (Some(first), Some(last)) = (points.first(), points.last()) {
                    length += distance(*last, *first);
                }
            }
            (length, 1, points.len())
        }
        SceneGeometry::Path { segments } => path_measure(segments),
    }
}

fn path_measure(segments: &[PathSegment]) -> (f64, usize, usize) {
    let mut current = [0.0; 2];
    let mut start = current;
    let mut length = 0.0;
    let mut subpaths = 0;
    for segment in segments {
        match segment {
            PathSegment::Move { to } => {
                current = *to;
                start = *to;
                subpaths += 1;
            }
            PathSegment::Line { to } => {
                length += distance(current, *to);
                current = *to;
            }
            PathSegment::Cubic {
                control1,
                control2,
                to,
            } => {
                length += distance(current, *control1)
                    + distance(*control1, *control2)
                    + distance(*control2, *to);
                current = *to;
            }
            PathSegment::Quadratic { control, to } => {
                length += distance(current, *control) + distance(*control, *to);
                current = *to;
            }
            PathSegment::Arc {
                rx,
                ry,
                rotation,
                to,
                ..
            } => {
                if *rx == 0.0 || *ry == 0.0 {
                    length += distance(current, *to);
                } else if current != *to {
                    let (s, c) = rotation.rem_euclid(360.0).to_radians().sin_cos();
                    let dx = (current[0] - to[0]) * 0.5;
                    let dy = (current[1] - to[1]) * 0.5;
                    let correction = ((c * dx + s * dy) / rx)
                        .hypot((-s * dx + c * dy) / ry)
                        .max(1.0);
                    // 包住修正后整个椭圆，也包住后端的有界三次曲线近似。
                    length += 8.0 * rx.max(*ry) * correction;
                }
                current = *to;
            }
            PathSegment::Close => {
                length += distance(current, start);
                current = start;
                // SVG 允许 Z 后直接继续绘制，后端可能开始新的隐式子路径。
                subpaths += 1;
            }
        }
    }
    (length, subpaths, segments.len())
}

fn coordinate_scale(geometry: &SceneGeometry) -> f64 {
    fn point(p: [f64; 2]) -> f64 {
        p[0].abs().max(p[1].abs())
    }
    match geometry {
        SceneGeometry::Group { .. } | SceneGeometry::Text { .. } => 0.0,
        SceneGeometry::Point { position } => point(*position) + 5.0,
        SceneGeometry::Polyline { points } | SceneGeometry::Polygon { points } => {
            points.iter().map(|p| point(*p)).fold(0.0, f64::max)
        }
        SceneGeometry::Rect {
            x,
            y,
            width,
            height,
            ..
        } => point([*x, *y]).max(point([x + width, y + height])),
        SceneGeometry::Ellipse { cx, cy, rx, ry } => (cx.abs() + rx).max(cy.abs() + ry),
        SceneGeometry::Path { segments } => segments
            .iter()
            .map(|segment| match segment {
                PathSegment::Move { to } | PathSegment::Line { to } => point(*to),
                PathSegment::Cubic {
                    control1,
                    control2,
                    to,
                } => point(*control1).max(point(*control2)).max(point(*to)),
                PathSegment::Quadratic { control, to } => point(*control).max(point(*to)),
                PathSegment::Arc { rx, ry, to, .. } => point(*to).max(*rx).max(*ry),
                PathSegment::Close => 0.0,
            })
            .fold(0.0, f64::max),
    }
}

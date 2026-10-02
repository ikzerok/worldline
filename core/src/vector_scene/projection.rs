use super::{
    validate_scene, viewport, Affine, MapScene, PathSegment, SceneClip, SceneError, SceneGeometry,
    SceneLimits, ScenePrimitive, SceneStyle,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneNodeState {
    pub transform: Affine,
    pub style: SceneStyle,
    pub visible: bool,
    pub locked: bool,
    pub clips: Vec<SceneClip>,
}

pub fn node_world_transform(scene: &MapScene, id: &str) -> Result<Affine, SceneError> {
    Ok(node_state(scene, id)?.transform)
}

pub fn node_state(scene: &MapScene, id: &str) -> Result<SceneNodeState, SceneError> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut next = Some(id);
    while let Some(id) = next {
        if !seen.insert(id) || chain.len() >= SceneLimits::default().max_depth {
            return Err(SceneError::new("SCENE_STRUCTURE", "父级循环或层级超预算"));
        }
        let node = scene
            .nodes
            .get(id)
            .ok_or_else(|| SceneError::new("SCENE_REFERENCE", "节点不存在").at(id, "id"))?;
        chain.push(node);
        next = node.parent_id.as_deref();
    }
    let mut state = SceneNodeState {
        transform: Affine::IDENTITY,
        style: SceneStyle::default(),
        visible: true,
        locked: false,
        clips: Vec::new(),
    };
    for node in chain.into_iter().rev() {
        state.transform = viewport::checked_affine(state.transform.then(node.transform))?;
        state.style = node.style.inherited(&state.style);
        state.visible &= node.visible;
        state.locked |= node.locked;
        if let Some(rect) = node.clip_rect {
            state.clips.push(SceneClip {
                rect,
                transform: state.transform,
            });
        }
    }
    Ok(state)
}

pub fn project_scene(scene: &MapScene, tolerance: f64) -> Result<Vec<ScenePrimitive>, SceneError> {
    validate_scene(scene, &SceneLimits::default())?;
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(SceneError::new("SCENE_GEOMETRY", "投影精度必须为有限正数"));
    }
    let mut output = Vec::new();
    let mut total = 0;
    fn walk(
        scene: &MapScene,
        id: &str,
        tolerance: f64,
        total: &mut usize,
        out: &mut Vec<ScenePrimitive>,
    ) -> Result<(), SceneError> {
        let node = &scene.nodes[id];
        let state = node_state(scene, id)?;
        if let SceneGeometry::Group { children } = &node.geometry {
            for child in children {
                walk(scene, child, tolerance, total, out)?;
            }
            return Ok(());
        }
        let mut paths = Vec::new();
        let mut closed = Vec::new();
        let mut text = Vec::new();
        let mut text_origin = [0.0; 2];
        match &node.geometry {
            SceneGeometry::Group { .. } => {}
            SceneGeometry::Point { position } => {
                paths.push(vec![state.transform.point(*position)]);
                closed.push(false);
                *total += 1;
            }
            SceneGeometry::Polyline { points } | SceneGeometry::Polygon { points } => {
                paths.push(points.iter().map(|p| state.transform.point(*p)).collect());
                closed.push(matches!(node.geometry, SceneGeometry::Polygon { .. }));
                *total += points.len();
            }
            SceneGeometry::Rect {
                x,
                y,
                width,
                height,
                rx,
                ry,
            } => {
                let rx = rx.min(width * 0.5);
                let ry = ry.min(height * 0.5);
                let segments = if rx == 0.0 || ry == 0.0 {
                    vec![
                        PathSegment::Move { to: [*x, *y] },
                        PathSegment::Line {
                            to: [x + width, *y],
                        },
                        PathSegment::Line {
                            to: [x + width, y + height],
                        },
                        PathSegment::Line {
                            to: [*x, y + height],
                        },
                        PathSegment::Close,
                    ]
                } else {
                    vec![
                        PathSegment::Move { to: [x + rx, *y] },
                        PathSegment::Line {
                            to: [x + width - rx, *y],
                        },
                        PathSegment::Arc {
                            rx,
                            ry,
                            rotation: 0.0,
                            large_arc: false,
                            sweep: true,
                            to: [x + width, y + ry],
                        },
                        PathSegment::Line {
                            to: [x + width, y + height - ry],
                        },
                        PathSegment::Arc {
                            rx,
                            ry,
                            rotation: 0.0,
                            large_arc: false,
                            sweep: true,
                            to: [x + width - rx, y + height],
                        },
                        PathSegment::Line {
                            to: [x + rx, y + height],
                        },
                        PathSegment::Arc {
                            rx,
                            ry,
                            rotation: 0.0,
                            large_arc: false,
                            sweep: true,
                            to: [*x, y + height - ry],
                        },
                        PathSegment::Line { to: [*x, y + ry] },
                        PathSegment::Arc {
                            rx,
                            ry,
                            rotation: 0.0,
                            large_arc: false,
                            sweep: true,
                            to: [x + rx, *y],
                        },
                        PathSegment::Close,
                    ]
                };
                (paths, closed) = sample(&segments, state.transform, tolerance, total)?;
            }
            SceneGeometry::Ellipse { cx, cy, rx, ry } => {
                let segments = vec![
                    PathSegment::Move { to: [cx + rx, *cy] },
                    PathSegment::Arc {
                        rx: *rx,
                        ry: *ry,
                        rotation: 0.0,
                        large_arc: false,
                        sweep: true,
                        to: [cx - rx, *cy],
                    },
                    PathSegment::Arc {
                        rx: *rx,
                        ry: *ry,
                        rotation: 0.0,
                        large_arc: false,
                        sweep: true,
                        to: [cx + rx, *cy],
                    },
                    PathSegment::Close,
                ];
                (paths, closed) = sample(&segments, state.transform, tolerance, total)?;
            }
            SceneGeometry::Path { segments } => {
                (paths, closed) = sample(segments, state.transform, tolerance, total)?
            }
            SceneGeometry::Text { x, y, runs } => {
                text = runs.clone();
                text_origin = [*x, *y];
            }
        }
        if *total > SceneLimits::default().max_projection_points {
            return Err(super::validate::limit("临时投影点数"));
        }
        for p in paths.iter().flatten() {
            super::validate::point(*p)?;
        }
        let bounds = super::world_bounds::node_bounds(node, state.transform, &state.style)?;
        out.push(ScenePrimitive {
            node_id: id.into(),
            paths,
            closed,
            style: state.style,
            text,
            text_origin,
            transform: state.transform,
            bounds,
            clips: state.clips,
        });
        Ok(())
    }
    for id in scene.root_order.values().flatten() {
        walk(scene, id, tolerance, &mut total, &mut output)?;
    }
    Ok(output)
}

type Paths = (Vec<Vec<[f64; 2]>>, Vec<bool>);
fn sample(
    segments: &[PathSegment],
    matrix: Affine,
    tolerance: f64,
    total: &mut usize,
) -> Result<Paths, SceneError> {
    let mut paths = Vec::new();
    let mut closed = Vec::new();
    let mut path = Vec::new();
    let mut current = [0.0; 2];
    let mut start = current;
    for segment in segments {
        if path.is_empty() && !matches!(segment, PathSegment::Move { .. } | PathSegment::Close) {
            push(&mut path, matrix.point(current), total)?;
        }
        match segment {
            PathSegment::Move { to } => {
                if !path.is_empty() {
                    paths.push(std::mem::take(&mut path));
                    closed.push(false);
                }
                current = *to;
                start = *to;
                push(&mut path, matrix.point(*to), total)?;
            }
            PathSegment::Line { to } => {
                push(&mut path, matrix.point(*to), total)?;
                current = *to;
            }
            PathSegment::Cubic {
                control1,
                control2,
                to,
            } => {
                flatten(
                    &[
                        matrix.point(current),
                        matrix.point(*control1),
                        matrix.point(*control2),
                        matrix.point(*to),
                    ],
                    tolerance,
                    0,
                    &mut path,
                    total,
                )?;
                current = *to;
            }
            PathSegment::Quadratic { control, to } => {
                flatten(
                    &[
                        matrix.point(current),
                        matrix.point(*control),
                        matrix.point(*to),
                    ],
                    tolerance,
                    0,
                    &mut path,
                    total,
                )?;
                current = *to;
            }
            PathSegment::Arc {
                rx,
                ry,
                rotation,
                large_arc,
                sweep,
                to,
            } => {
                arc(
                    current,
                    *to,
                    [*rx, *ry, *rotation],
                    *large_arc,
                    *sweep,
                    matrix,
                    tolerance,
                    &mut path,
                    total,
                )?;
                current = *to;
            }
            PathSegment::Close => {
                if !path.is_empty() {
                    paths.push(std::mem::take(&mut path));
                    closed.push(true);
                }
                current = start;
            }
        }
    }
    if !path.is_empty() {
        paths.push(path);
        closed.push(false);
    }
    Ok((paths, closed))
}
fn push(path: &mut Vec<[f64; 2]>, point: [f64; 2], total: &mut usize) -> Result<(), SceneError> {
    super::validate::point(point)?;
    if *total >= SceneLimits::default().max_projection_points {
        return Err(super::validate::limit("临时投影点数"));
    }
    *total += 1;
    path.push(point);
    Ok(())
}
fn distance(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let square = dx * dx + dy * dy;
    let t = if square == 0.0 {
        0.0
    } else {
        ((point[0] - a[0]) * dx + (point[1] - a[1]) * dy) / square
    }
    .clamp(0.0, 1.0);
    (point[0] - a[0] - t * dx).hypot(point[1] - a[1] - t * dy)
}
fn flatten(
    control: &[[f64; 2]],
    tolerance: f64,
    depth: usize,
    path: &mut Vec<[f64; 2]>,
    total: &mut usize,
) -> Result<(), SceneError> {
    let last = control[control.len() - 1];
    if control[1..control.len() - 1]
        .iter()
        .all(|p| distance(*p, control[0], last) <= tolerance)
    {
        return push(path, last, total);
    }
    if depth >= 32 {
        return Err(super::validate::limit("曲线投影递归"));
    }
    let mut left = vec![control[0]];
    let mut right = vec![last];
    let mut row = control.to_vec();
    while row.len() > 1 {
        row = row
            .windows(2)
            .map(|p| [(p[0][0] + p[1][0]) * 0.5, (p[0][1] + p[1][1]) * 0.5])
            .collect();
        left.push(row[0]);
        right.push(row[row.len() - 1]);
    }
    right.reverse();
    flatten(&left, tolerance, depth + 1, path, total)?;
    flatten(&right, tolerance, depth + 1, path, total)
}

#[allow(clippy::too_many_arguments)]
fn arc(
    from: [f64; 2],
    to: [f64; 2],
    parameters: [f64; 3],
    large: bool,
    sweep: bool,
    matrix: Affine,
    tolerance: f64,
    path: &mut Vec<[f64; 2]>,
    total: &mut usize,
) -> Result<(), SceneError> {
    let [mut rx, mut ry, rotation] = parameters;
    if rx == 0.0 || ry == 0.0 || from == to {
        return push(path, matrix.point(to), total);
    }
    let (sin, cos) = rotation.rem_euclid(360.0).to_radians().sin_cos();
    let dx = (from[0] - to[0]) * 0.5;
    let dy = (from[1] - to[1]) * 0.5;
    let x = cos * dx + sin * dy;
    let y = -sin * dx + cos * dy;
    let scale = (x / rx).hypot(y / ry);
    if !scale.is_finite() || scale == 0.0 {
        return Err(SceneError::new("SCENE_NUMERIC", "圆弧投影精度无法安全表达"));
    }
    if scale > 1.0 {
        rx *= scale;
        ry *= scale;
    }
    let ux = x / rx;
    let uy = y / ry;
    let lambda = ux * ux + uy * uy;
    let factor =
        ((1.0 - lambda).max(0.0) / lambda).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let cxp = factor * rx * uy;
    let cyp = -factor * ry * ux;
    let center = [
        cos * cxp - sin * cyp + (from[0] + to[0]) * 0.5,
        sin * cxp + cos * cyp + (from[1] + to[1]) * 0.5,
    ];
    let start = ((y - cyp) / ry).atan2((x - cxp) / rx);
    let end = ((-y - cyp) / ry).atan2((-x - cxp) / rx);
    let mut delta = end - start;
    if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    }
    let radius = rx.max(ry)
        * (matrix.0[0].abs() + matrix.0[1].abs() + matrix.0[2].abs() + matrix.0[3].abs());
    let step = if radius <= tolerance {
        std::f64::consts::FRAC_PI_2
    } else {
        2.0 * (1.0 - tolerance / radius).clamp(-1.0, 1.0).acos()
    };
    if !step.is_finite() || step <= 0.0 {
        return Err(super::validate::limit("圆弧投影精度"));
    }
    let count = (delta.abs() / step).ceil().max(1.0);
    if count
        > SceneLimits::default()
            .max_projection_points
            .saturating_sub(*total) as f64
    {
        return Err(super::validate::limit("圆弧投影点数"));
    }
    for i in 1..=count as usize {
        let angle = start + delta * i as f64 / count;
        let (s, c) = angle.sin_cos();
        let point = if i == count as usize {
            to
        } else {
            [
                center[0] + cos * rx * c - sin * ry * s,
                center[1] + sin * rx * c + cos * ry * s,
            ]
        };
        push(path, matrix.point(point), total)?;
    }
    Ok(())
}

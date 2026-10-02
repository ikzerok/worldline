use super::{
    viewport, Affine, MapScene, PathSegment, SceneError, SceneGeometry, SceneNode, SceneStyle,
};

/// 对曲线控制凸包与椭圆/圆弧保守包络校验，不能让离散采样漏掉峰值。
pub(super) fn node_bounds(
    node: &SceneNode,
    matrix: Affine,
    style: &SceneStyle,
) -> Result<[f64; 4], SceneError> {
    viewport::checked_affine(matrix)?;
    let mut bounds = Bounds::default();
    let mut push = |p| bounds.push(matrix.point(p));
    if let Some([x, y, w, h]) = node.clip_rect {
        for p in corners(x, y, x + w, y + h) {
            push(p)?;
        }
    }
    match &node.geometry {
        SceneGeometry::Group { .. } => {}
        SceneGeometry::Point { position } => {
            for p in corners(
                position[0] - 5.0,
                position[1] - 5.0,
                position[0] + 5.0,
                position[1] + 5.0,
            ) {
                push(p)?;
            }
        }
        SceneGeometry::Polyline { points } | SceneGeometry::Polygon { points } => {
            for p in points {
                push(*p)?;
            }
        }
        SceneGeometry::Rect {
            x,
            y,
            width,
            height,
            ..
        } => {
            for p in corners(*x, *y, x + width, y + height) {
                push(p)?;
            }
        }
        SceneGeometry::Ellipse { cx, cy, rx, ry } => {
            for p in corners(cx - rx, cy - ry, cx + rx, cy + ry) {
                push(p)?;
            }
        }
        SceneGeometry::Path { segments } => {
            let mut current = [0.0; 2];
            let mut start = current;
            for segment in segments {
                match segment {
                    PathSegment::Move { to } => {
                        push(*to)?;
                        current = *to;
                        start = *to;
                    }
                    PathSegment::Line { to } => {
                        push(*to)?;
                        current = *to;
                    }
                    PathSegment::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        push(*control1)?;
                        push(*control2)?;
                        push(*to)?;
                        current = *to;
                    }
                    PathSegment::Quadratic { control, to } => {
                        push(*control)?;
                        push(*to)?;
                        current = *to;
                    }
                    PathSegment::Arc {
                        rx,
                        ry,
                        rotation,
                        to,
                        ..
                    } => {
                        if *rx > 0.0 && *ry > 0.0 && *to != current {
                            let (s, c) = rotation.rem_euclid(360.0).to_radians().sin_cos();
                            let dx = (current[0] - to[0]) * 0.5;
                            let dy = (current[1] - to[1]) * 0.5;
                            let factor = ((c * dx + s * dy) / rx)
                                .hypot((-s * dx + c * dy) / ry)
                                .max(1.0);
                            let radius = rx.max(*ry) * factor * 2.0;
                            viewport::number(radius)?;
                            for p in corners(
                                current[0] - radius,
                                current[1] - radius,
                                current[0] + radius,
                                current[1] + radius,
                            ) {
                                push(p)?;
                            }
                        }
                        push(*to)?;
                        current = *to;
                    }
                    PathSegment::Close => {
                        current = start;
                        push(start)?;
                    }
                }
            }
        }
        SceneGeometry::Text { x, y, runs } => {
            let mut cursor = [*x, *y];
            for run in runs {
                let effective = run.style.inherited(style);
                let size = effective.font_size.unwrap_or(16.0);
                cursor = [
                    run.x.unwrap_or(cursor[0]) + run.dx,
                    run.y.unwrap_or(cursor[1]) + run.dy,
                ];
                let width = size * run.text.chars().count() as f64 * 2.0;
                for p in corners(
                    cursor[0] - width,
                    cursor[1] - size * 2.0,
                    cursor[0] + width,
                    cursor[1] + size * 2.0,
                ) {
                    push(p)?;
                }
                cursor[0] += width;
            }
        }
    }
    let mut result = bounds.finish();
    if style.stroke.as_deref().is_some_and(|value| value != "none")
        && !matches!(node.geometry, SceneGeometry::Group { .. })
    {
        let width = style.stroke_width.unwrap_or(1.0);
        let expansion = width
            * style.miter_limit.unwrap_or(4.0)
            * (matrix.0[0].abs() + matrix.0[1].abs() + matrix.0[2].abs() + matrix.0[3].abs());
        viewport::number(expansion)?;
        result[0] -= expansion;
        result[1] -= expansion;
        result[2] += expansion;
        result[3] += expansion;
    }
    for value in result {
        viewport::number(value)?;
    }
    Ok(result)
}

pub(super) fn viewport(scene: &MapScene, matrix: Affine) -> Result<(), SceneError> {
    fn walk(
        scene: &MapScene,
        id: &str,
        matrix: Affine,
        style: &SceneStyle,
    ) -> Result<(), SceneError> {
        let node = &scene.nodes[id];
        let world = viewport::checked_affine(matrix.then(node.transform))
            .map_err(|e| e.at(id, "transform"))?;
        let effective = node.style.inherited(style);
        node_bounds(node, world, &effective).map_err(|e| e.at(id, "geometry"))?;
        if let SceneGeometry::Group { children } = &node.geometry {
            for child in children {
                walk(scene, child, world, &effective)?;
            }
        }
        Ok(())
    }
    for root in scene.root_order.values().flatten() {
        walk(scene, root, matrix, &SceneStyle::default())?;
    }
    Ok(())
}

pub(super) fn corners(x1: f64, y1: f64, x2: f64, y2: f64) -> [[f64; 2]; 4] {
    [[x1, y1], [x2, y1], [x2, y2], [x1, y2]]
}
#[derive(Default)]
struct Bounds {
    value: Option<[f64; 4]>,
}
impl Bounds {
    fn push(&mut self, p: [f64; 2]) -> Result<(), SceneError> {
        super::validate::point(p)?;
        let b = self.value.get_or_insert([p[0], p[1], p[0], p[1]]);
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
        Ok(())
    }
    fn finish(self) -> [f64; 4] {
        self.value.unwrap_or([0.0; 4])
    }
}

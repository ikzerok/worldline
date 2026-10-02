use super::{
    style::validate_style, viewport, Affine, MapScene, PathSegment, SceneError, SceneGeometry,
    SceneLimits, SceneNode, SceneStyle, SCENE_SCHEMA_VERSION,
};
use std::collections::BTreeSet;
use std::io::Write;

pub fn validate_scene(scene: &MapScene, limits: &SceneLimits) -> Result<(), SceneError> {
    check_limits(limits)?;
    if scene.schema_version != SCENE_SCHEMA_VERSION {
        return Err(SceneError::new(
            "SCENE_SCHEMA",
            "不支持的 scene schema，只能只读保留",
        ));
    }
    if scene
        .extra
        .get("required_features")
        .is_some_and(|v| !crate::workspace_documents::features_supported(v))
    {
        return Err(SceneError::new(
            "SCENE_FEATURE",
            "scene 包含未知必需能力，只能只读保留",
        ));
    }
    if scene.nodes.len() > limits.max_nodes {
        return Err(limit("场景节点数"));
    }
    if scene.extra.keys().any(|key| {
        [
            "schema_version",
            "view_box",
            "preserve_aspect_ratio",
            "root_order",
            "nodes",
        ]
        .contains(&key.as_str())
    }) {
        return Err(SceneError::new(
            "SCENE_STRUCTURE",
            "scene.extra 不能覆盖已知字段",
        ));
    }
    viewport::view_box_transform(
        scene.view_box,
        scene.view_box[2],
        scene.view_box[3],
        &scene.preserve_aspect_ratio,
    )?;
    let mut visited = BTreeSet::new();
    let mut totals = Totals::default();
    for (layer, roots) in &scene.root_order {
        if !crate::workspace_documents::valid_id(layer) {
            return Err(SceneError::new("SCENE_STRUCTURE", "图层 ID 无效"));
        }
        for id in roots {
            walk(
                scene,
                id,
                None,
                layer,
                1,
                Affine::IDENTITY,
                &SceneStyle::default(),
                limits,
                &mut visited,
                &mut totals,
            )?;
        }
    }
    if visited.len() != scene.nodes.len() {
        return Err(SceneError::new(
            "SCENE_STRUCTURE",
            "场景含孤儿节点或缺少 root_order",
        ));
    }
    if totals.segments > limits.max_segments {
        return Err(limit("路径段数"));
    }
    if totals.text > limits.max_text_bytes {
        return Err(limit("文本字节数"));
    }
    let mut writer = BoundedWriter {
        count: 0,
        max: limits.max_document_bytes,
    };
    serde_json::to_writer(&mut writer, scene).map_err(|_| limit("场景文档字节数"))?;
    Ok(())
}

pub(super) fn check_limits(limits: &SceneLimits) -> Result<(), SceneError> {
    let hard = SceneLimits::default();
    for (value, max) in [
        (limits.max_source_bytes, hard.max_source_bytes),
        (limits.max_nodes, hard.max_nodes),
        (limits.max_import_shapes, hard.max_import_shapes),
        (limits.max_depth, hard.max_depth),
        (limits.max_segments, hard.max_segments),
        (limits.max_text_bytes, hard.max_text_bytes),
        (limits.max_operations, hard.max_operations),
        (limits.max_document_bytes, hard.max_document_bytes),
        (limits.max_projection_points, hard.max_projection_points),
    ] {
        if value > max {
            return Err(limit("调用方不可放宽硬预算"));
        }
    }
    Ok(())
}

pub(super) fn limit(name: &str) -> SceneError {
    SceneError::new("SCENE_LIMIT", format!("{name}超过安全预算"))
}

#[derive(Default)]
struct Totals {
    segments: usize,
    text: usize,
}

#[allow(clippy::too_many_arguments)]
fn walk(
    scene: &MapScene,
    id: &str,
    parent: Option<&str>,
    layer: &str,
    depth: usize,
    matrix: Affine,
    inherited: &SceneStyle,
    limits: &SceneLimits,
    visited: &mut BTreeSet<String>,
    totals: &mut Totals,
) -> Result<(), SceneError> {
    if depth > limits.max_depth {
        return Err(limit("场景层级").at(id, "parent_id"));
    }
    let Some(node) = scene.nodes.get(id) else {
        return Err(SceneError::new("SCENE_STRUCTURE", "引用的节点不存在").at(id, "children"));
    };
    if !visited.insert(id.into()) {
        return Err(SceneError::new("SCENE_STRUCTURE", "节点重复或层级循环").at(id, "parent_id"));
    }
    if node.id != id
        || !crate::workspace_documents::valid_id(id)
        || node.parent_id.as_deref() != parent
        || node.layer_id != layer
    {
        return Err(
            SceneError::new("SCENE_STRUCTURE", "节点 ID、父子关系或图层不一致").at(id, "parent_id"),
        );
    }
    validate_node(node, totals).map_err(|mut e| {
        e.node_id = Some(id.into());
        e
    })?;
    let world =
        viewport::checked_affine(matrix.then(node.transform)).map_err(|e| e.at(id, "transform"))?;
    let effective = node.style.inherited(inherited);
    super::world_bounds::node_bounds(node, world, &effective).map_err(|e| e.at(id, "geometry"))?;
    if let SceneGeometry::Group { children } = &node.geometry {
        for child in children {
            walk(
                scene,
                child,
                Some(id),
                layer,
                depth + 1,
                world,
                &effective,
                limits,
                visited,
                totals,
            )?;
        }
    }
    Ok(())
}

fn validate_node(node: &SceneNode, totals: &mut Totals) -> Result<(), SceneError> {
    let reserved = [
        "id",
        "name",
        "layer_id",
        "parent_id",
        "geometry",
        "transform",
        "style",
        "visible",
        "locked",
        "clip_rect",
        "target_ref",
        "annotation",
        "role",
        "label_override",
        "navigation",
        "scope_refs",
    ];
    if node
        .extra
        .keys()
        .any(|key| reserved.contains(&key.as_str()))
    {
        return Err(SceneError::new(
            "SCENE_STRUCTURE",
            "node.extra 不能覆盖已知字段",
        ));
    }
    viewport::checked_affine(node.transform)?;
    validate_style(&node.style)?;
    if let Some([x, y, w, h]) = node.clip_rect {
        for value in [x, y, w, h, x + w, y + h] {
            viewport::number(value)?;
        }
        if w <= 0.0
            || h <= 0.0
            || !matches!(node.geometry, SceneGeometry::Group { .. })
            || node.extra.get("svg_root").and_then(|v| v.as_bool()) != Some(true)
        {
            return Err(SceneError::new(
                "SCENE_GEOMETRY",
                "clip_rect 只允许正宽高的根 viewport 组",
            ));
        }
    }
    match &node.geometry {
        SceneGeometry::Group { .. } => {}
        SceneGeometry::Point { position } => point(*position)?,
        SceneGeometry::Polyline { points } | SceneGeometry::Polygon { points } => {
            let minimum = if matches!(node.geometry, SceneGeometry::Polygon { .. }) {
                3
            } else {
                2
            };
            if points.len() < minimum {
                return Err(SceneError::new("SCENE_GEOMETRY", "线/面顶点数量不足"));
            }
            totals.segments = totals.segments.saturating_add(points.len());
            for p in points {
                point(*p)?;
            }
        }
        SceneGeometry::Rect {
            x,
            y,
            width,
            height,
            rx,
            ry,
        } => {
            for v in [x, y, width, height, rx, ry] {
                viewport::number(*v)?;
            }
            if *width < 0.0 || *height < 0.0 || *rx < 0.0 || *ry < 0.0 {
                return Err(SceneError::new("SCENE_GEOMETRY", "矩形宽高/圆角不能为负"));
            }
        }
        SceneGeometry::Ellipse { cx, cy, rx, ry } => {
            for v in [cx, cy, rx, ry] {
                viewport::number(*v)?;
            }
            if *rx < 0.0 || *ry < 0.0 {
                return Err(SceneError::new("SCENE_GEOMETRY", "椭圆半径不能为负"));
            }
        }
        SceneGeometry::Path { segments } => {
            totals.segments = totals.segments.saturating_add(segments.len());
            if !segments.is_empty() && !matches!(segments.first(), Some(PathSegment::Move { .. })) {
                return Err(SceneError::new("SCENE_GEOMETRY", "路径必须从 Move 开始"));
            }
            for segment in segments {
                match segment {
                    PathSegment::Move { to } | PathSegment::Line { to } => point(*to)?,
                    PathSegment::Cubic {
                        control1,
                        control2,
                        to,
                    } => {
                        point(*control1)?;
                        point(*control2)?;
                        point(*to)?;
                    }
                    PathSegment::Quadratic { control, to } => {
                        point(*control)?;
                        point(*to)?;
                    }
                    PathSegment::Arc {
                        rx,
                        ry,
                        rotation,
                        to,
                        ..
                    } => {
                        for v in [rx, ry, rotation] {
                            viewport::number(*v)?;
                        }
                        if *rx < 0.0 || *ry < 0.0 {
                            return Err(SceneError::new("SCENE_GEOMETRY", "圆弧半径不能为负"));
                        }
                        point(*to)?;
                    }
                    PathSegment::Close => {}
                }
            }
        }
        SceneGeometry::Text { x, y, runs } => {
            point([*x, *y])?;
            for run in runs {
                for v in run.x.into_iter().chain(run.y).chain([run.dx, run.dy]) {
                    viewport::number(v)?;
                }
                if run
                    .text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r'))
                {
                    return Err(SceneError::new("SCENE_GEOMETRY", "文字包含非法控制字符"));
                }
                totals.text = totals.text.saturating_add(run.text.len());
                validate_style(&run.style)?;
                if run
                    .extra
                    .keys()
                    .any(|key| ["text", "x", "y", "dx", "dy", "style"].contains(&key.as_str()))
                {
                    return Err(SceneError::new(
                        "SCENE_STRUCTURE",
                        "text run.extra 不能覆盖已知字段",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn point(p: [f64; 2]) -> Result<(), SceneError> {
    viewport::number(p[0])?;
    viewport::number(p[1])?;
    Ok(())
}

pub(super) fn serialized_size<T: serde::Serialize>(
    value: &T,
    maximum: usize,
) -> Result<usize, SceneError> {
    let mut writer = BoundedWriter {
        count: 0,
        max: maximum,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| limit("序列化字节数"))?;
    Ok(writer.count)
}

struct BoundedWriter {
    count: usize,
    max: usize,
}
impl Write for BoundedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.count = self.count.saturating_add(buf.len());
        if self.count > self.max {
            return Err(std::io::Error::other("scene byte budget"));
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

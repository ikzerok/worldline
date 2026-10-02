use super::{
    view_box_transform, MapScene, SceneError, SceneGeometry, SceneNavigation, SceneNode,
    SceneStyle, TextRun,
};
use crate::presentation::{MapDocument, MapGeometry, MapPlacement};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(super) fn placement_node(
    map: &MapDocument,
    placement: &MapPlacement,
    scene: &MapScene,
    strict: bool,
) -> Result<SceneNode, SceneError> {
    let width = map.canvas.width as f64;
    let height = map.canvas.height as f64;
    let point = |p: [f64; 2]| [p[0] * width, p[1] * height];
    let geometry = match &placement.geometry {
        MapGeometry::Point { position } => SceneGeometry::Point {
            position: point(*position),
        },
        MapGeometry::Polyline { points } => SceneGeometry::Polyline {
            points: points.iter().copied().map(point).collect(),
        },
        MapGeometry::Polygon { points } => SceneGeometry::Polygon {
            points: points.iter().copied().map(point).collect(),
        },
        MapGeometry::Text {
            position,
            text,
            font_size,
            ..
        } => {
            let [x, y] = point(*position);
            SceneGeometry::Text {
                x,
                y: y + font_size,
                runs: text
                    .split('\n')
                    .enumerate()
                    .map(|(i, line)| TextRun {
                        text: line.into(),
                        x: Some(x),
                        y: Some(y + font_size + i as f64 * font_size * 1.2),
                        ..TextRun::default()
                    })
                    .collect(),
            }
        }
    };
    let mut node = SceneNode::new(&placement.id, &placement.layer_id, geometry);
    node.transform =
        view_box_transform(scene.view_box, width, height, &scene.preserve_aspect_ratio)?
            .inverse()
            .ok_or_else(|| {
                SceneError::new("SCENE_NUMERIC", "目标 scene 逆矩阵无法安全表达旧标记")
            })?;
    node.target_ref = placement.target_ref.clone();
    node.annotation = placement.annotation.clone();
    node.role = placement.role.clone();
    node.label_override = placement.label_override.clone();
    node.navigation = placement.navigation.as_ref().map(|nav| SceneNavigation {
        map_id: nav.map_id.clone(),
        extra: nav.extra.clone(),
    });
    node.scope_refs = placement.scope_refs.clone();
    node.extra = placement.extra.clone();
    if !placement.extensions.is_empty() {
        node.extra.insert(
            "extensions".into(),
            Value::Object(placement.extensions.clone()),
        );
    }
    node.style = legacy_style(placement, strict)?;
    Ok(node)
}

fn legacy_style(placement: &MapPlacement, strict: bool) -> Result<SceneStyle, SceneError> {
    let mut style = SceneStyle {
        fill: Some("#88aacc".into()),
        stroke: Some("#446688".into()),
        stroke_width: Some(2.0),
        ..SceneStyle::default()
    };
    if let Some(raw) = &placement.style {
        for (key, value) in raw {
            match key.as_str() {
                "fill" | "stroke" => {
                    let valid = value.as_str().filter(|s| {
                        *s == "none"
                            || s.len() == 7
                                && s.starts_with('#')
                                && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
                    });
                    if let Some(value) = valid {
                        if key == "fill" {
                            style.fill = Some(value.into());
                        } else {
                            style.stroke = Some(value.into());
                        }
                    } else if strict {
                        return Err(style_loss(&placement.id, key));
                    }
                }
                "stroke_width" | "stroke_opacity" | "fill_opacity" => {
                    let max = if key == "stroke_width" { 100.0 } else { 1.0 };
                    let valid = value
                        .as_f64()
                        .filter(|v| v.is_finite() && (0.0..=max).contains(v));
                    if let Some(value) = valid {
                        match key.as_str() {
                            "stroke_width" => style.stroke_width = Some(value),
                            "stroke_opacity" => style.stroke_opacity = Some(value),
                            _ => style.fill_opacity = Some(value),
                        }
                    } else if strict {
                        return Err(style_loss(&placement.id, key));
                    }
                }
                "opacity" | "font_size" | "font_family" | "font_weight" | "font_style"
                | "text_anchor" | "fill_rule" | "line_cap" | "line_join" | "miter_limit"
                    if strict =>
                {
                    return Err(style_loss(&placement.id, key))
                }
                _ => {
                    if strict {
                        style.extra.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }
    if matches!(placement.geometry, MapGeometry::Polyline { .. }) {
        style.fill = Some("none".into());
    }
    if let MapGeometry::Text {
        font_size, color, ..
    } = &placement.geometry
    {
        if strict
            && placement.style.as_ref().is_some_and(|s| {
                s.get("fill")
                    .and_then(Value::as_str)
                    .is_some_and(|v| v != color)
            })
        {
            return Err(style_loss(&placement.id, "fill"));
        }
        style.fill = Some(color.clone());
        style.stroke = Some("none".into());
        style.font_size = Some(*font_size);
        style.font_family = Some("sans-serif".into());
    }
    super::style::validate_style(&style)?;
    Ok(style)
}

fn style_loss(id: &str, key: &str) -> SceneError {
    SceneError::new(
        "SCENE_MIGRATION_STYLE",
        format!("旧样式 `{key}` 无法无损迁移；请先明确修正或保留旧标记"),
    )
    .at(id, format!("style.{key}"))
}

pub(super) fn scene_with_placements(map: &MapDocument) -> Result<MapScene, SceneError> {
    let mut scene = map
        .scene
        .clone()
        .unwrap_or_else(|| MapScene::new(map.canvas.width as f64, map.canvas.height as f64));
    let mut roots: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for placement in map.placements.values() {
        if scene.nodes.contains_key(&placement.id) {
            return Err(SceneError::new(
                "SCENE_STRUCTURE",
                "旧标记与scene节点ID重复",
            ));
        }
        let node = placement_node(map, placement, &scene, false)?;
        roots
            .entry(placement.layer_id.clone())
            .or_default()
            .push(placement.id.clone());
        scene.nodes.insert(placement.id.clone(), node);
    }
    for (layer, mut old) in roots {
        old.extend(scene.root_order.remove(&layer).unwrap_or_default());
        scene.root_order.insert(layer, old);
    }
    Ok(scene)
}

pub(super) fn migrate(
    map: &MapDocument,
    scene: &mut MapScene,
    root: &mut Map<String, Value>,
    ids: &[String],
) -> Result<(), SceneError> {
    let selected: std::collections::BTreeSet<_> = ids.iter().cloned().collect();
    if ids.is_empty() || selected.len() != ids.len() {
        return Err(SceneError::new("SCENE_STRUCTURE", "迁移选择为空或重复"));
    }
    for id in ids {
        if !map.placements.contains_key(id) {
            return Err(SceneError::new("SCENE_REFERENCE", "待迁移旧标记不存在").at(id, "id"));
        }
    }
    let mut converted = BTreeMap::new();
    for layer in &map.layer_order {
        let originals: Vec<_> = map
            .placements
            .values()
            .filter(|p| p.layer_id == *layer)
            .collect();
        let Some(start) = originals.iter().position(|p| selected.contains(&p.id)) else {
            continue;
        };
        if originals[start..].iter().any(|p| !selected.contains(&p.id)) {
            return Err(SceneError::new(
                "SCENE_MIGRATION_ORDER",
                "部分中段迁移会改变绘制顺序；请预览迁移本层全部旧标记",
            ));
        }
        if map.layers[layer].locked {
            return Err(SceneError::new("SCENE_LOCKED", "迁移图层已锁定"));
        }
        let mut new_roots = Vec::new();
        for placement in &originals[start..] {
            if scene.nodes.contains_key(&placement.id) {
                return Err(SceneError::new("SCENE_STRUCTURE", "迁移ID与scene重复"));
            }
            let node = placement_node(map, placement, scene, true)?;
            new_roots.push(node.id.clone());
            converted.insert(node.id.clone(), node);
        }
        new_roots.extend(scene.root_order.remove(layer).unwrap_or_default());
        scene.root_order.insert(layer.clone(), new_roots);
    }
    scene.nodes.extend(converted);
    let placements = root
        .get_mut("placements")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| SceneError::new("SCENE_SCHEMA", "地图缺少旧标记集合"))?;
    for id in ids {
        placements.remove(id);
    }
    Ok(())
}

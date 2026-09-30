use super::*;
use crate::catalog::Catalog;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(super) fn apply_placement(
    object: &mut Map<String, Value>,
    command: &Command,
    catalog: &Catalog,
    source_unresolved: bool,
    affected: &mut BTreeSet<TargetRef>,
) -> Result<(), EditError> {
    match command {
        Command::CreatePlacement {
            placement_id,
            layer_id,
            target_ref,
            geometry,
            annotation,
            role,
            label_override,
            ..
        } => {
            validate_id(placement_id, "标记 ID")?;
            validate_geometry(geometry)?;
            ensure_geometry_feature(object, geometry)?;
            validate_text(annotation, "annotation")?;
            validate_text(role, "role")?;
            validate_target(target_ref.as_ref(), catalog, source_unresolved, affected)?;
            let layers = layers_mut(object)?;
            let layer = layers
                .get(layer_id)
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("图层 `{layer_id}` 不存在"),
                })?;
            if layer.get("locked").and_then(Value::as_bool) == Some(true) {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 已锁定"),
                });
            }
            let placements = placements_mut(object)?;
            if placements.contains_key(placement_id) {
                return Err(EditError::InvalidSchema {
                    message: format!("标记 `{placement_id}` 已存在"),
                });
            }
            let mut placement = Map::new();
            placement.insert("layer_id".into(), Value::String(layer_id.clone()));
            placement.insert(
                "target_ref".into(),
                target_ref.as_ref().map(target_value).unwrap_or(Value::Null),
            );
            placement.insert("geometry".into(), serde_json::to_value(geometry).unwrap());
            placement.insert("annotation".into(), Value::String(annotation.clone()));
            placement.insert("role".into(), Value::String(role.clone()));
            if let Some(label) = label_override {
                placement.insert("label_override".into(), Value::String(label.clone()));
            }
            placements.insert(placement_id.clone(), Value::Object(placement));
        }
        Command::UpdatePlacement {
            placement_id,
            geometry,
            target_ref,
            annotation,
            role,
            label_override,
            layer_id,
            ..
        } => {
            let original_layer = object
                .get("placements")
                .and_then(Value::as_object)
                .and_then(|placements| placements.get(placement_id))
                .and_then(Value::as_object)
                .and_then(|placement| placement.get("layer_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("标记 `{placement_id}` 不存在或缺少 layer_id"),
                })?
                .to_owned();
            let old_layer = object
                .get("layers")
                .and_then(Value::as_object)
                .and_then(|layers| layers.get(&original_layer))
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("图层 `{original_layer}` 不存在"),
                })?;
            if old_layer.get("locked").and_then(Value::as_bool) == Some(true) {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{original_layer}` 已锁定"),
                });
            }
            if let Some(layer_id) = layer_id {
                validate_id(layer_id, "图层 ID")?;
                let new_layer = object
                    .get("layers")
                    .and_then(Value::as_object)
                    .and_then(|layers| layers.get(layer_id))
                    .ok_or_else(|| EditError::MissingReference {
                        message: format!("图层 `{layer_id}` 不存在"),
                    })?;
                if new_layer.get("locked").and_then(Value::as_bool) == Some(true) {
                    return Err(EditError::ReadOnlyFeature {
                        message: format!("图层 `{layer_id}` 已锁定"),
                    });
                }
            }
            if let Some(geometry) = geometry {
                validate_geometry(geometry)?;
                ensure_geometry_feature(object, geometry)?;
            }
            if let Some(target_ref) = target_ref {
                validate_target(target_ref.as_ref(), catalog, source_unresolved, affected)?;
            }
            if let Some(annotation) = annotation {
                validate_text(annotation, "annotation")?;
            }
            if let Some(role) = role {
                validate_text(role, "role")?;
            }
            let placement = placement_mut(object, placement_id)?;
            if let Some(geometry) = geometry {
                let mut next = serde_json::to_value(geometry).unwrap();
                if let (Some(old), Some(new)) = (
                    placement.get("geometry").and_then(Value::as_object),
                    next.as_object_mut(),
                ) {
                    for (key, value) in old {
                        if !["kind", "position", "points", "text", "font_size", "color"]
                            .contains(&key.as_str())
                        {
                            new.insert(key.clone(), value.clone());
                        }
                    }
                }
                placement.insert("geometry".into(), next);
            }
            if let Some(target_ref) = target_ref {
                placement.insert(
                    "target_ref".into(),
                    target_ref.as_ref().map(target_value).unwrap_or(Value::Null),
                );
            }
            if let Some(annotation) = annotation {
                placement.insert("annotation".into(), Value::String(annotation.clone()));
            }
            if let Some(role) = role {
                placement.insert("role".into(), Value::String(role.clone()));
            }
            if let Some(label_override) = label_override {
                placement.insert(
                    "label_override".into(),
                    label_override
                        .as_ref()
                        .map(|label| Value::String(label.clone()))
                        .unwrap_or(Value::Null),
                );
            }
            if let Some(layer_id) = layer_id {
                placement.insert("layer_id".into(), Value::String(layer_id.clone()));
            }
        }
        Command::DeletePlacement { placement_id, .. } => {
            let layer_id = object
                .get("placements")
                .and_then(Value::as_object)
                .and_then(|placements| placements.get(placement_id))
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("标记 `{placement_id}` 不存在"),
                })?
                .as_object()
                .and_then(|placement| placement.get("layer_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| EditError::InvalidSchema {
                    message: format!("标记 `{placement_id}` 缺少有效 layer_id"),
                })?
                .to_owned();
            let locked = object
                .get("layers")
                .and_then(Value::as_object)
                .and_then(|layers| layers.get(&layer_id))
                .and_then(|layer| layer.get("locked"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if locked {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 已锁定"),
                });
            }
            let old = placements_mut(object)?
                .remove(placement_id)
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("标记 `{placement_id}` 不存在"),
                })?;
            if let Some(target) = old.get("target_ref").and_then(parse_target_value) {
                affected.insert(target);
            }
        }
        _ => unreachable!("placement dispatcher only receives placement commands"),
    }
    Ok(())
}

fn placements_mut(object: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, EditError> {
    object
        .get_mut("placements")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 placements 必须是对象".into(),
        })
}

fn placement_mut<'a>(
    object: &'a mut Map<String, Value>,
    placement_id: &str,
) -> Result<&'a mut Map<String, Value>, EditError> {
    placements_mut(object)?
        .get_mut(placement_id)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| EditError::MissingReference {
            message: format!("标记 `{placement_id}` 不存在"),
        })
}

fn validate_target(
    target: Option<&TargetRef>,
    catalog: &crate::catalog::Catalog,
    source_unresolved: bool,
    affected: &mut BTreeSet<TargetRef>,
) -> Result<(), EditError> {
    let Some(target) = target else {
        return Ok(());
    };
    if catalog.object(target).is_none() {
        return Err(if source_unresolved {
            EditError::UnresolvedReference {
                message: format!(
                    "对象 {} `{}` 尚未解析，无法建立新的地图引用",
                    target.kind, target.id
                ),
            }
        } else {
            EditError::MissingReference {
                message: format!("对象 {} `{}` 不存在", target.kind, target.id),
            }
        });
    }
    affected.insert(target.clone());
    Ok(())
}

fn target_value(target: &TargetRef) -> Value {
    serde_json::json!({"kind": target.kind, "id": target.id})
}

fn parse_target_value(value: &Value) -> Option<TargetRef> {
    let object = value.as_object()?;
    Some(TargetRef::new(
        object.get("kind")?.as_str()?,
        object.get("id")?.as_str()?,
    ))
}

fn validate_geometry(geometry: &MapGeometry) -> Result<(), EditError> {
    if let MapGeometry::Text {
        text,
        font_size,
        color,
        ..
    } = geometry
    {
        if !crate::presentation::valid_map_text(text, *font_size, color) {
            return Err(EditError::InvalidGeometry {
                message: "文字标签必须为非空纯文本（最多160字、4行），字号12–64，颜色#RRGGBB"
                    .into(),
            });
        }
    }
    let points = geometry.points();
    let minimum = match geometry {
        MapGeometry::Point { .. } | MapGeometry::Text { .. } => 1,
        MapGeometry::Polyline { .. } => 2,
        MapGeometry::Polygon { .. } => 3,
    };
    if points.len() < minimum {
        return Err(EditError::InvalidGeometry {
            message: format!("几何至少需要 {minimum} 个点"),
        });
    }
    if points.iter().any(|point| {
        point
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    }) {
        return Err(EditError::InvalidGeometry {
            message: "几何坐标必须是有限数且在 [0,1] 内".into(),
        });
    }
    if let MapGeometry::Polygon { points } = geometry {
        if points.first() == points.last() || points.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(EditError::InvalidGeometry {
                message: "多边形不能重复首尾点或相邻点".into(),
            });
        }
        if polygon_area(points).abs() <= f64::EPSILON || polygon_self_intersects(points) {
            return Err(EditError::InvalidGeometry {
                message: "多边形必须是非退化的简单多边形".into(),
            });
        }
    }
    Ok(())
}

fn ensure_geometry_feature(
    object: &mut Map<String, Value>,
    geometry: &MapGeometry,
) -> Result<(), EditError> {
    let feature = match geometry {
        MapGeometry::Text { .. } => crate::presentation::TEXT_GEOMETRY_FEATURE,
        MapGeometry::Polyline { .. } | MapGeometry::Polygon { .. } => {
            "presentation.geometry.line_area.v1"
        }
        MapGeometry::Point { .. } => return Ok(()),
    };
    let features = object
        .entry("required_features")
        .or_insert_with(|| Value::Array(Vec::new()));
    let values = features
        .as_array_mut()
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 required_features 必须是数组".into(),
        })?;
    if values.iter().any(|value| value.as_str() == Some(feature)) {
        return Ok(());
    }
    values.push(Value::String(feature.into()));
    Ok(())
}

fn polygon_area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        * 0.5
}

fn polygon_self_intersects(points: &[[f64; 2]]) -> bool {
    let len = points.len();
    for first in 0..len {
        let first_next = (first + 1) % len;
        for second in (first + 1)..len {
            let second_next = (second + 1) % len;
            if first == second
                || first_next == second
                || second_next == first
                || (first == 0 && second_next == 0)
            {
                continue;
            }
            if segments_intersect(
                points[first],
                points[first_next],
                points[second],
                points[second_next],
            ) {
                return true;
            }
        }
    }
    false
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    fn orientation(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    fn on_segment(a: [f64; 2], b: [f64; 2], point: [f64; 2]) -> bool {
        point[0] >= a[0].min(b[0])
            && point[0] <= a[0].max(b[0])
            && point[1] >= a[1].min(b[1])
            && point[1] <= a[1].max(b[1])
    }
    let ab_c = orientation(a, b, c);
    let ab_d = orientation(a, b, d);
    let cd_a = orientation(c, d, a);
    let cd_b = orientation(c, d, b);
    let epsilon = 1e-12;
    (ab_c * ab_d < -epsilon && cd_a * cd_b < -epsilon)
        || (ab_c.abs() <= epsilon && on_segment(a, b, c))
        || (ab_d.abs() <= epsilon && on_segment(a, b, d))
        || (cd_a.abs() <= epsilon && on_segment(c, d, a))
        || (cd_b.abs() <= epsilon && on_segment(c, d, b))
}

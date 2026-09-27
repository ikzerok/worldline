use super::*;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(super) fn apply_layer(
    object: &mut Map<String, Value>,
    command: &Command,
) -> Result<(), EditError> {
    match command {
        Command::CreateLayer {
            layer_id,
            title,
            visible_default,
            locked,
            ..
        } => {
            validate_id(layer_id, "图层 ID")?;
            validate_text(title, "title")?;
            let layers = layers_mut(object)?;
            if layers.contains_key(layer_id) {
                return Err(EditError::InvalidSchema {
                    message: format!("图层 `{layer_id}` 已存在"),
                });
            }
            let mut layer = Map::new();
            layer.insert("title".into(), Value::String(title.clone()));
            layer.insert("visible_default".into(), Value::Bool(*visible_default));
            layer.insert("locked".into(), Value::Bool(*locked));
            layers.insert(layer_id.clone(), Value::Object(layer));
            let order = layer_order_mut(object)?;
            order.push(Value::String(layer_id.clone()));
        }
        Command::SetLayer {
            layer_id,
            title,
            visible_default,
            locked,
            layer_order,
            ..
        } => {
            let layer = layers_mut(object)?.get_mut(layer_id).ok_or_else(|| {
                EditError::MissingReference {
                    message: format!("图层 `{layer_id}` 不存在"),
                }
            })?;
            let layer = layer
                .as_object_mut()
                .ok_or_else(|| EditError::InvalidSchema {
                    message: format!("图层 `{layer_id}` 必须是对象"),
                })?;
            if let Some(title) = title {
                validate_text(title, "title")?;
                layer.insert("title".into(), Value::String(title.clone()));
            }
            if let Some(visible_default) = visible_default {
                layer.insert("visible_default".into(), Value::Bool(*visible_default));
            }
            if let Some(locked) = locked {
                layer.insert("locked".into(), Value::Bool(*locked));
            }
            if let Some(layer_order) = layer_order {
                validate_layer_order(object, layer_order)?;
                object.insert(
                    "layer_order".into(),
                    Value::Array(layer_order.iter().cloned().map(Value::String).collect()),
                );
            }
        }
        Command::DeleteLayer { layer_id, .. } => {
            let layer = object
                .get("layers")
                .and_then(Value::as_object)
                .and_then(|layers| layers.get(layer_id))
                .ok_or_else(|| EditError::MissingReference {
                    message: format!("图层 `{layer_id}` 不存在"),
                })?;
            if layer.get("locked").and_then(Value::as_bool) == Some(true) {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 已锁定"),
                });
            }
            let has_placements = object
                .get("placements")
                .and_then(Value::as_object)
                .is_some_and(|placements| {
                    placements.values().any(|placement| {
                        placement.get("layer_id").and_then(Value::as_str) == Some(layer_id)
                    })
                });
            if has_placements {
                return Err(EditError::ReadOnlyFeature {
                    message: format!("图层 `{layer_id}` 仍包含标记，请先移动或删除标记"),
                });
            }
            layers_mut(object)?.remove(layer_id);
            let order = layer_order_mut(object)?;
            order.retain(|value| value.as_str() != Some(layer_id));
        }
        _ => unreachable!("layer dispatcher only receives layer commands"),
    }
    Ok(())
}

fn layer_order_mut(object: &mut Map<String, Value>) -> Result<&mut Vec<Value>, EditError> {
    object
        .get_mut("layer_order")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 layer_order 必须是数组".into(),
        })
}

fn validate_layer_order(object: &Map<String, Value>, order: &[String]) -> Result<(), EditError> {
    let layers = object
        .get("layers")
        .and_then(Value::as_object)
        .ok_or_else(|| EditError::InvalidSchema {
            message: "地图 layers 必须是对象".into(),
        })?;
    let expected: BTreeSet<_> = layers.keys().cloned().collect();
    let actual: BTreeSet<_> = order.iter().cloned().collect();
    if expected != actual || actual.len() != order.len() {
        return Err(EditError::InvalidSchema {
            message: "layer_order 必须恰好包含每个图层一次".into(),
        });
    }
    Ok(())
}

use super::SceneError;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn fields(
    original: &Value,
    scene: &mut Value,
    sources: &BTreeMap<String, String>,
    maximum: usize,
) -> Result<(), SceneError> {
    let mut working = super::validate::serialized_size(scene, maximum)?;
    let mut sizes = BTreeMap::new();
    for source in sources.values() {
        let size = if let Some(size) = sizes.get(source) {
            *size
        } else {
            let value = original_node(original, source);
            let size = super::validate::serialized_size(&value, maximum)?;
            sizes.insert(source.clone(), size);
            size
        };
        working = working.saturating_add(size);
        if working > maximum.saturating_mul(2) {
            return Err(super::validate::limit("未知字段保全工作字节数"));
        }
    }
    let Some(nodes) = scene.get_mut("nodes").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    for (id, value) in nodes {
        let Some(source) = sources.get(id) else {
            continue;
        };
        let old = if let Some(id) = source.strip_prefix("scene:") {
            original
                .get("scene")
                .and_then(|s| s.get("nodes"))
                .and_then(|n| n.get(id))
        } else {
            source
                .strip_prefix("legacy:")
                .and_then(|id| original.get("placements").and_then(|p| p.get(id)))
        };
        if let Some(old_node) = old {
            if let (Some(before), Some(after)) =
                (old_node.get("target_ref"), value.get_mut("target_ref"))
            {
                reference_extras(before, after);
            }
            if let (Some(before), Some(after)) = (
                old_node.get("scope_refs").and_then(Value::as_array),
                value.get_mut("scope_refs").and_then(Value::as_array_mut),
            ) {
                for next in after {
                    if let Some(previous) = before.iter().find(|previous| {
                        previous.get("kind") == next.get("kind")
                            && previous.get("id") == next.get("id")
                    }) {
                        reference_extras(previous, next);
                    }
                }
            }
        }
        if let (Some(old), Some(new)) = (
            old.and_then(|n| n.get("geometry")),
            value.get_mut("geometry"),
        ) {
            merge_geometry(old, new)?;
        }
    }
    Ok(())
}
fn merge_geometry(old: &Value, new: &mut Value) -> Result<(), SceneError> {
    const KNOWN: &[&str] = &[
        "kind",
        "position",
        "points",
        "text",
        "font_size",
        "color",
        "children",
        "x",
        "y",
        "width",
        "height",
        "rx",
        "ry",
        "cx",
        "cy",
        "segments",
        "runs",
    ];
    let (Some(old), Some(new)) = (old.as_object(), new.as_object_mut()) else {
        return Ok(());
    };
    for (key, value) in old {
        if !KNOWN.contains(&key.as_str()) {
            new.insert(key.clone(), value.clone());
        }
    }
    if let (Some(before), Some(after)) = (
        old.get("segments").and_then(Value::as_array),
        new.get_mut("segments").and_then(Value::as_array_mut),
    ) {
        let known = [
            "kind",
            "to",
            "control1",
            "control2",
            "control",
            "rx",
            "ry",
            "rotation",
            "large_arc",
            "sweep",
        ];
        if before.len() != after.len()
            && before
                .iter()
                .filter_map(Value::as_object)
                .any(|s| s.keys().any(|k| !known.contains(&k.as_str())))
        {
            return Err(SceneError::new(
                "SCENE_STRUCTURE",
                "路径段含未知扩展，改变段数量无法保证扩展定位；请保留原结构",
            ));
        }
        for (a, b) in before.iter().zip(after) {
            if let (Some(a), Some(b)) = (a.as_object(), b.as_object_mut()) {
                for (key, value) in a {
                    if !known.contains(&key.as_str()) {
                        b.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }
    Ok(())
}

fn reference_extras(before: &Value, after: &mut Value) {
    if let (Some(before), Some(after)) = (before.as_object(), after.as_object_mut()) {
        for (key, value) in before {
            if key != "kind" && key != "id" {
                after.entry(key).or_insert_with(|| value.clone());
            }
        }
    }
}

fn original_node<'a>(original: &'a Value, source: &str) -> Option<&'a Value> {
    if let Some(id) = source.strip_prefix("scene:") {
        original
            .get("scene")
            .and_then(|s| s.get("nodes"))
            .and_then(|n| n.get(id))
    } else {
        source
            .strip_prefix("legacy:")
            .and_then(|id| original.get("placements").and_then(|n| n.get(id)))
    }
}

pub(super) fn duplicate_bytes(
    scene: &super::MapScene,
    original: &Value,
    sources: &BTreeMap<String, String>,
    ids: &[String],
    maximum: usize,
) -> Result<usize, SceneError> {
    let mut pending = ids.to_vec();
    let mut seen = std::collections::BTreeSet::new();
    let mut count = 0usize;
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let node = scene
            .nodes
            .get(&id)
            .ok_or_else(|| SceneError::new("SCENE_REFERENCE", "复制节点不存在"))?;
        let current = super::validate::serialized_size(node, maximum)?;
        let raw = sources
            .get(&id)
            .and_then(|source| original_node(original, source));
        let old_size = super::validate::serialized_size(&raw, maximum)?;
        count = count.saturating_add(current.max(old_size));
        if count > maximum {
            return Err(super::validate::limit("复制节点字节数"));
        }
        if let super::SceneGeometry::Group { children } = &node.geometry {
            pending.extend(children.iter().cloned());
        }
    }
    Ok(count)
}

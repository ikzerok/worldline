use super::{MapScene, SceneError, SceneGeometry};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn assert_unlocked(
    scene: &MapScene,
    root: &Map<String, Value>,
    id: &str,
) -> Result<(), SceneError> {
    let node = scene.nodes.get(id).ok_or_else(|| missing(id))?;
    if node.locked {
        return Err(SceneError::new("SCENE_LOCKED", "节点已锁定").at(id, "locked"));
    }
    unlocked_parent(scene, root, node.parent_id.as_deref(), &node.layer_id)
}
pub(super) fn unlocked_parent(
    scene: &MapScene,
    root: &Map<String, Value>,
    parent: Option<&str>,
    layer: &str,
) -> Result<(), SceneError> {
    let layer_info = root
        .get("layers")
        .and_then(|l| l.get(layer))
        .ok_or_else(|| error("目标图层不存在"))?;
    if layer_info.get("locked").and_then(Value::as_bool) == Some(true) {
        return Err(SceneError::new("SCENE_LOCKED", "图层已锁定"));
    }
    let mut current = parent;
    let mut seen = BTreeSet::new();
    while let Some(id) = current {
        if !seen.insert(id) {
            return Err(error("父层级循环"));
        }
        if seen.len() >= super::SceneLimits::default().max_depth {
            return Err(super::validate::limit("父层级深度"));
        }
        let node = scene.nodes.get(id).ok_or_else(|| missing(id))?;
        if node.locked {
            return Err(SceneError::new("SCENE_LOCKED", "祖先组已锁定").at(id, "locked"));
        }
        if node.layer_id != layer || !matches!(node.geometry, SceneGeometry::Group { .. }) {
            return Err(error("父节点不是同层组"));
        }
        current = node.parent_id.as_deref();
    }
    Ok(())
}
pub(super) fn unlocked_subtree(
    scene: &MapScene,
    root: &Map<String, Value>,
    id: &str,
) -> Result<(), SceneError> {
    assert_unlocked(scene, root, id)?;
    if let SceneGeometry::Group { children } = &scene.nodes[id].geometry {
        for child in children {
            unlocked_subtree(scene, root, child)?;
        }
    }
    Ok(())
}
pub(super) fn siblings<'a>(
    scene: &'a mut MapScene,
    parent: Option<&str>,
    layer: &str,
) -> Result<&'a mut Vec<String>, SceneError> {
    if let Some(parent) = parent {
        let node = scene.nodes.get_mut(parent).ok_or_else(|| missing(parent))?;
        if let SceneGeometry::Group { children } = &mut node.geometry {
            return Ok(children);
        }
        Err(error("父节点不是组"))
    } else {
        Ok(scene.root_order.entry(layer.into()).or_default())
    }
}
pub(super) fn detach(scene: &mut MapScene, id: &str) -> Result<usize, SceneError> {
    let node = scene.nodes.get(id).ok_or_else(|| missing(id))?;
    let parent = node.parent_id.clone();
    let layer = node.layer_id.clone();
    let order = siblings(scene, parent.as_deref(), &layer)?;
    let i = order
        .iter()
        .position(|v| v == id)
        .ok_or_else(|| error("节点不在父级顺序中"))?;
    order.remove(i);
    Ok(i)
}
pub(super) fn remove(scene: &mut MapScene, id: &str, sources: &mut BTreeMap<String, String>) {
    if let Some(node) = scene.nodes.remove(id) {
        if let SceneGeometry::Group { children } = node.geometry {
            for child in children {
                remove(scene, &child, sources);
            }
        }
    }
    sources.remove(id);
}
pub(super) fn selection(scene: &MapScene, ids: &[String]) -> Result<Vec<String>, SceneError> {
    let set: BTreeSet<_> = ids.iter().cloned().collect();
    if ids.is_empty() || set.len() != ids.len() {
        return Err(error("选择为空或含重复 ID"));
    }
    for id in ids {
        if !scene.nodes.contains_key(id) {
            return Err(missing(id));
        }
    }
    let mut out = Vec::new();
    fn walk(scene: &MapScene, id: &str, set: &BTreeSet<String>, out: &mut Vec<String>) {
        if set.contains(id) {
            out.push(id.into());
            return;
        }
        if let SceneGeometry::Group { children } = &scene.nodes[id].geometry {
            for child in children {
                walk(scene, child, set, out);
            }
        }
    }
    for id in scene.root_order.values().flatten() {
        walk(scene, id, &set, &mut out);
    }
    Ok(out)
}
pub(super) fn unique(
    scene: &MapScene,
    root: &Map<String, Value>,
    id: &str,
) -> Result<(), SceneError> {
    if !crate::workspace_documents::valid_id(id)
        || scene.nodes.contains_key(id)
        || root.get("placements").and_then(|p| p.get(id)).is_some()
    {
        return Err(error("节点 ID 无效或与现有节点/旧标记重复"));
    }
    Ok(())
}
pub(super) fn available_id(scene: &MapScene, root: &Map<String, Value>, prefix: &str) -> String {
    let mut id = prefix.to_string();
    let mut n = 1;
    while unique(scene, root, &id).is_err() {
        id = format!("{prefix}_{n}");
        n += 1;
    }
    id
}
pub(super) fn create_layer(
    root: &mut Map<String, Value>,
    id: &str,
    title: &str,
) -> Result<(), SceneError> {
    if !crate::workspace_documents::valid_id(id) {
        return Err(error("图层 ID 无效"));
    }
    let layers = root
        .get_mut("layers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| error("缺少图层集合"))?;
    if let Some(layer) = layers.get(id) {
        if layer.get("locked").and_then(Value::as_bool) == Some(true) {
            return Err(SceneError::new("SCENE_LOCKED", "目标导入图层已锁定"));
        }
        return Ok(());
    }
    layers.insert(
        id.into(),
        json!({"title":title,"visible_default":true,"locked":false}),
    );
    root.get_mut("layer_order")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| error("缺少图层顺序"))?
        .push(Value::String(id.into()));
    Ok(())
}
pub(super) fn error(message: &str) -> SceneError {
    SceneError::new("SCENE_STRUCTURE", message)
}
pub(super) fn missing(id: &str) -> SceneError {
    SceneError::new("SCENE_REFERENCE", "场景节点不存在").at(id, "id")
}
pub(super) fn boundary() -> SceneError {
    SceneError::new(
        "SCENE_COMPOSITING_BOUNDARY",
        "操作将丢失组透明度合成或viewport裁剪；请保留或移动完整边界组",
    )
}

/// 在克隆 ID 字符串前约束派生身份数据，不另设任意 ID 长度上限。
pub(super) fn identity_budget(count: usize, bytes: usize, copies: usize) -> Result<(), SceneError> {
    if count.saturating_mul(bytes).saturating_mul(copies)
        > super::SceneLimits::default().max_document_bytes
    {
        Err(super::validate::limit("派生节点身份字节数"))
    } else {
        Ok(())
    }
}
pub(super) fn subtree_count(scene: &MapScene, id: &str) -> usize {
    match &scene.nodes[id].geometry {
        SceneGeometry::Group { children } => {
            1 + children
                .iter()
                .map(|child| subtree_count(scene, child))
                .sum::<usize>()
        }
        _ => 1,
    }
}

pub(super) fn subtree_depth(scene: &MapScene, id: &str) -> usize {
    match &scene.nodes[id].geometry {
        SceneGeometry::Group { children } => {
            1 + children
                .iter()
                .map(|child| subtree_depth(scene, child))
                .max()
                .unwrap_or(0)
        }
        _ => 1,
    }
}

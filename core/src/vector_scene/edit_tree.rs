use super::structure::*;
use super::{Affine, MapScene, SceneError, SceneGeometry, SceneNode, SceneStyle};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn group(
    scene: &mut MapScene,
    root: &Map<String, Value>,
    group_id: &str,
    ids: &[String],
    name: &str,
) -> Result<(), SceneError> {
    unique(scene, root, group_id)?;
    let ids = selection(scene, ids)?;
    identity_budget(ids.len(), group_id.len(), 1)?;
    let first = scene.nodes[&ids[0]].clone();
    for id in &ids {
        assert_unlocked(scene, root, id)?;
        let node = &scene.nodes[id];
        if node.parent_id != first.parent_id || node.layer_id != first.layer_id {
            return Err(error("分组节点必须属于同一父级与图层"));
        }
    }
    let mut parent_depth = 0;
    let mut parent = first.parent_id.as_deref();
    while let Some(id) = parent {
        parent_depth += 1;
        parent = scene.nodes[id].parent_id.as_deref();
    }
    let depth = ids
        .iter()
        .map(|id| subtree_depth(scene, id))
        .max()
        .unwrap_or(0);
    if parent_depth + 1 + depth > super::SceneLimits::default().max_depth {
        return Err(super::validate::limit("分组层级深度"));
    }
    let order = siblings(scene, first.parent_id.as_deref(), &first.layer_id)?;
    let selected: BTreeSet<_> = ids.iter().collect();
    let positions: Vec<_> = order
        .iter()
        .enumerate()
        .filter(|(_, id)| selected.contains(id))
        .map(|(i, _)| i)
        .collect();
    let index = positions.last().unwrap() + 1 - ids.len();
    order.retain(|id| !selected.contains(id));
    order.insert(index, group_id.into());
    let mut group = SceneNode::new(
        group_id,
        &first.layer_id,
        SceneGeometry::Group {
            children: ids.clone(),
        },
    );
    group.parent_id = first.parent_id;
    group.name = name.into();
    for id in ids {
        scene.nodes.get_mut(&id).unwrap().parent_id = Some(group_id.into());
    }
    scene.nodes.insert(group_id.into(), group);
    Ok(())
}
pub(super) fn ungroup(
    scene: &mut MapScene,
    root: &Map<String, Value>,
    id: &str,
    sources: &mut BTreeMap<String, String>,
) -> Result<(), SceneError> {
    unlocked_subtree(scene, root, id)?;
    let group = scene.nodes[id].clone();
    let SceneGeometry::Group { children } = &group.geometry else {
        return Err(error("取消分组的目标不是组"));
    };
    if group.style.opacity.unwrap_or(1.0) != 1.0 || group.clip_rect.is_some() {
        return Err(boundary());
    }
    identity_budget(
        children.len(),
        group.parent_id.as_ref().map_or(0, String::len),
        1,
    )?;
    let index = detach(scene, id)?;
    for child in children {
        let node = scene.nodes.get_mut(child).unwrap();
        node.parent_id = group.parent_id.clone();
        node.transform = group.transform.then(node.transform);
        node.style = node.style.inherited(&group.style);
        node.visible &= group.visible;
    }
    let order = siblings(scene, group.parent_id.as_deref(), &group.layer_id)?;
    order.splice(index..index, children.clone());
    scene.nodes.remove(id);
    sources.remove(id);
    Ok(())
}
pub(super) fn world(scene: &MapScene, id: &str) -> Result<(Affine, SceneStyle, bool), SceneError> {
    let mut ancestors = Vec::new();
    let mut current = Some(id);
    while let Some(id) = current {
        let node = &scene.nodes[id];
        ancestors.push(node);
        current = node.parent_id.as_deref();
    }
    let mut matrix = Affine::IDENTITY;
    let mut style = SceneStyle::default();
    let mut visible = true;
    for node in ancestors.into_iter().rev() {
        matrix = matrix.then(node.transform);
        style = node.style.inherited(&style);
        visible &= node.visible;
    }
    Ok((matrix, style, visible))
}
pub(super) fn move_to_layer(
    scene: &mut MapScene,
    root: &Map<String, Value>,
    ids: &[String],
    layer: &str,
) -> Result<(), SceneError> {
    unlocked_parent(scene, root, None, layer)?;
    let ids = selection(scene, ids)?;
    identity_budget(
        ids.iter().map(|id| subtree_count(scene, id)).sum(),
        layer.len(),
        1,
    )?;
    for id in &ids {
        unlocked_subtree(scene, root, id)?;
        let mut parent = scene.nodes[id].parent_id.as_deref();
        while let Some(p) = parent {
            let n = &scene.nodes[p];
            if n.clip_rect.is_some() || n.style.opacity.unwrap_or(1.0) != 1.0 {
                return Err(boundary());
            }
            parent = n.parent_id.as_deref();
        }
    }
    fn change(scene: &mut MapScene, id: &str, layer: &str) {
        let node = scene.nodes.get_mut(id).unwrap();
        node.layer_id = layer.into();
        let children = if let SceneGeometry::Group { children } = &node.geometry {
            children.clone()
        } else {
            Vec::new()
        };
        for child in children {
            change(scene, &child, layer);
        }
    }
    for id in ids {
        let (matrix, style, visible) = world(scene, &id)?;
        detach(scene, &id)?;
        change(scene, &id, layer);
        let node = scene.nodes.get_mut(&id).unwrap();
        node.parent_id = None;
        node.transform = matrix;
        node.style = style;
        node.visible = visible;
        scene.root_order.entry(layer.into()).or_default().push(id);
    }
    Ok(())
}
pub(super) fn duplicate(
    scene: &mut MapScene,
    root: &Map<String, Value>,
    ids: &[String],
    prefix: &str,
    offset: [f64; 2],
    sources: &mut BTreeMap<String, String>,
) -> Result<(), SceneError> {
    if !crate::workspace_documents::valid_id(prefix) {
        return Err(error("复制 ID 前缀无效"));
    }
    super::validate::point(offset)?;
    let ids = selection(scene, ids)?;
    identity_budget(
        ids.iter().map(|id| subtree_count(scene, id)).sum(),
        prefix.len().saturating_add(24),
        3,
    )?;
    fn copy(
        scene: &mut MapScene,
        root: &Map<String, Value>,
        id: &str,
        parent: Option<String>,
        prefix: &str,
        count: &mut usize,
        sources: &mut BTreeMap<String, String>,
    ) -> String {
        let mut node = scene.nodes[id].clone();
        *count += 1;
        let new = available_id(scene, root, &format!("{prefix}_{count}"));
        node.id = new.clone();
        node.parent_id = parent;
        let children = if let SceneGeometry::Group { children } = &node.geometry {
            children.clone()
        } else {
            Vec::new()
        };
        scene.nodes.insert(new.clone(), node);
        let copied = children
            .iter()
            .map(|child| {
                copy(
                    scene,
                    root,
                    child,
                    Some(new.clone()),
                    prefix,
                    count,
                    sources,
                )
            })
            .collect();
        if let SceneGeometry::Group { children } = &mut scene.nodes.get_mut(&new).unwrap().geometry
        {
            *children = copied;
        }
        if let Some(source) = sources.get(id).cloned() {
            sources.insert(new.clone(), source);
        }
        new
    }
    let mut count = 0;
    for id in ids {
        assert_unlocked(scene, root, &id)?;
        let original = scene.nodes[&id].clone();
        let new = copy(
            scene,
            root,
            &id,
            original.parent_id.clone(),
            prefix,
            &mut count,
            sources,
        );
        let parent_matrix = if let Some(parent) = &original.parent_id {
            world(scene, parent)?.0
        } else {
            Affine::IDENTITY
        };
        let inverse = parent_matrix
            .inverse()
            .ok_or_else(|| SceneError::new("SCENE_NUMERIC", "父矩阵不可逆，不能安全偏移复制"))?;
        let local = inverse
            .then(Affine([1.0, 0.0, 0.0, 1.0, offset[0], offset[1]]))
            .then(parent_matrix);
        scene.nodes.get_mut(&new).unwrap().transform = local.then(original.transform);
        let order = siblings(scene, original.parent_id.as_deref(), &original.layer_id)?;
        let at = order.iter().position(|x| x == &id).unwrap() + 1;
        order.insert(at, new);
    }
    Ok(())
}

/// 非连续分组相对未选择兄弟的重排影响；只供预览明确告知。
pub(super) fn group_reorder_affected(scene: &MapScene, ids: &[String]) -> Vec<String> {
    let Some(first) = ids.first().and_then(|id| scene.nodes.get(id)) else {
        return Vec::new();
    };
    let order = if let Some(parent) = &first.parent_id {
        match scene.nodes.get(parent).map(|n| &n.geometry) {
            Some(SceneGeometry::Group { children }) => children,
            _ => return Vec::new(),
        }
    } else {
        match scene.root_order.get(&first.layer_id) {
            Some(order) => order,
            None => return Vec::new(),
        }
    };
    let selected: BTreeSet<_> = ids.iter().collect();
    let positions: Vec<_> = order
        .iter()
        .enumerate()
        .filter(|(_, id)| selected.contains(id))
        .map(|(i, _)| i)
        .collect();
    if positions.len() != ids.len() || positions.is_empty() {
        return Vec::new();
    }
    let first = positions[0];
    let last = *positions.last().unwrap();
    if last - first + 1 == ids.len() {
        Vec::new()
    } else {
        order[first..=last].to_vec()
    }
}

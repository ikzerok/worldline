use super::edit_tree::{duplicate, group, move_to_layer, ungroup};
use super::structure::*;
use super::{import_view_transform, MapScene, SceneError, SceneGeometry, SceneNode, SceneOp};
use crate::presentation::MapDocument;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn apply(
    scene: &mut MapScene,
    map: &MapDocument,
    root: &mut Map<String, Value>,
    op: &SceneOp,
    sources: &mut BTreeMap<String, String>,
) -> Result<(), SceneError> {
    match op {
        SceneOp::EnableScene => {}
        SceneOp::Insert { node, index } => {
            unique(scene, root, &node.id)?;
            unlocked_parent(scene, root, node.parent_id.as_deref(), &node.layer_id)?;
            if matches!(&node.geometry,SceneGeometry::Group{children} if !children.is_empty()) {
                return Err(error("新建组的 children 必须为空"));
            }
            let order = siblings(scene, node.parent_id.as_deref(), &node.layer_id)?;
            let i = index.unwrap_or(order.len());
            if i > order.len() {
                return Err(error("插入位置越界"));
            }
            order.insert(i, node.id.clone());
            scene.nodes.insert(node.id.clone(), node.clone());
            sources.remove(&node.id);
        }
        SceneOp::Update { node } => {
            let previous = scene.nodes.get(&node.id).ok_or_else(|| missing(&node.id))?;
            if previous.parent_id != node.parent_id || previous.layer_id != node.layer_id {
                return Err(error("Update 不允许隐式改变父节点或图层"));
            }
            if let SceneGeometry::Group { children } = &previous.geometry {
                if !matches!(&node.geometry,SceneGeometry::Group{children:new} if new==children) {
                    return Err(error("Update 不允许修改组结构"));
                }
            } else if matches!(node.geometry, SceneGeometry::Group { .. }) {
                return Err(error("Update 不允许把图形隐式变成组"));
            }
            let mut unlocked = previous.clone();
            unlocked.locked = false;
            if &unlocked == node {
                unlocked_parent(scene, root, node.parent_id.as_deref(), &node.layer_id)?;
            } else {
                assert_unlocked(scene, root, &node.id)?;
            }
            scene.nodes.insert(node.id.clone(), node.clone());
        }
        SceneOp::Delete { node_ids } => {
            let ids = selection(scene, node_ids)?;
            for id in &ids {
                unlocked_subtree(scene, root, id)?;
            }
            for id in ids {
                detach(scene, &id)?;
                remove(scene, &id, sources);
            }
        }
        SceneOp::Reorder {
            parent_id,
            layer_id,
            node_ids,
        } => {
            unlocked_parent(scene, root, parent_id.as_deref(), layer_id)?;
            let current = siblings(scene, parent_id.as_deref(), layer_id)?;
            if current.len() != node_ids.len()
                || current.iter().collect::<BTreeSet<_>>() != node_ids.iter().collect()
            {
                return Err(error("新顺序必须恰好包含原有兄弟节点"));
            }
            *current = node_ids.clone();
        }
        SceneOp::Group {
            group_id,
            node_ids,
            name,
        } => group(scene, root, group_id, node_ids, name)?,
        SceneOp::Ungroup { node_id } => ungroup(scene, root, node_id, sources)?,
        SceneOp::Duplicate {
            node_ids,
            id_prefix,
            offset,
        } => duplicate(scene, root, node_ids, id_prefix, *offset, sources)?,
        SceneOp::ImportScene {
            layer_id,
            title,
            scene: source,
            width,
            height,
        } => {
            create_layer(root, layer_id, title)?;
            super::validate_scene(source, &super::SceneLimits::default())?;
            let matrix = import_view_transform(source, *width, *height, scene)?;
            let prefix = available_id(scene, root, &format!("{layer_id}_svg"));
            identity_budget(
                source.nodes.len().saturating_add(1),
                prefix.len().saturating_add(24),
                4,
            )?;
            let mut mapping = BTreeMap::new();
            for (i, id) in source.nodes.keys().enumerate() {
                mapping.insert(
                    id.clone(),
                    available_id(scene, root, &format!("{prefix}_n{i}")),
                );
            }
            let children: Vec<_> = source
                .root_order
                .values()
                .flatten()
                .map(|id| mapping[id].clone())
                .collect();
            let mut wrapper = SceneNode::new(&prefix, layer_id, SceneGeometry::Group { children });
            wrapper.name = title.clone();
            wrapper.transform = matrix;
            for (id, node) in &source.nodes {
                let mut node = node.clone();
                node.id = mapping[id].clone();
                node.layer_id = layer_id.clone();
                node.parent_id = Some(
                    node.parent_id
                        .as_ref()
                        .map(|p| mapping[p].clone())
                        .unwrap_or_else(|| prefix.clone()),
                );
                if let SceneGeometry::Group { children } = &mut node.geometry {
                    for child in children {
                        *child = mapping[child].clone();
                    }
                }
                scene.nodes.insert(node.id.clone(), node);
            }
            scene.nodes.insert(prefix.clone(), wrapper);
            scene
                .root_order
                .entry(layer_id.clone())
                .or_default()
                .push(prefix);
        }
        SceneOp::ImportSvg { .. } => return Err(error("ImportSvg 必须先由核心规范化")),
        SceneOp::MoveToLayer { node_ids, layer_id } => {
            move_to_layer(scene, root, node_ids, layer_id)?
        }
        SceneOp::MigratePlacements { node_ids } => {
            if node_ids.iter().any(|id| {
                root.get("placements")
                    .and_then(Value::as_object)
                    .is_none_or(|p| !p.contains_key(id))
            }) {
                return Err(error("旧标记不存在或已在本批迁移"));
            }
            super::legacy::migrate(map, scene, root, node_ids)?;
            for id in node_ids {
                sources.insert(id.clone(), format!("legacy:{id}"));
            }
        }
    }
    Ok(())
}

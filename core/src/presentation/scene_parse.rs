use super::{map_error, map_warning, MapLayer, MapPlacement};
use crate::catalog::Catalog;
use crate::vector_scene::{validate_scene, MapScene, SceneLimits, SCENE_FEATURE};
use crate::Diagnostic;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[allow(clippy::too_many_arguments)]
pub(super) fn parse_scene(
    object: &Map<String, Value>,
    layers: &BTreeMap<String, MapLayer>,
    placements: &BTreeMap<String, MapPlacement>,
    catalog: &Catalog,
    map_ids: &BTreeSet<String>,
    options: crate::CompileOptions,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<MapScene>, ()> {
    let Some(value) = object.get("scene") else {
        return Ok(None);
    };
    let declared = object
        .get("required_features")
        .and_then(Value::as_array)
        .is_some_and(|features| features.iter().any(|v| v.as_str() == Some(SCENE_FEATURE)));
    if !declared {
        diagnostics.push(map_error(
            file,
            "MAP002",
            "scene 缺少 presentation.vector_scene.v1 必需能力声明",
        ));
        return Err(());
    }
    let scene: MapScene = serde_json::from_value(value.clone()).map_err(|error| {
        diagnostics.push(map_error(
            file,
            "MAP014",
            format!("scene 格式无效：{error}"),
        ));
    })?;
    validate_scene(&scene, &SceneLimits::default()).map_err(|error| {
        diagnostics.push(map_error(
            file,
            "MAP014",
            format!(
                "{}（节点 {:?}，字段 {:?}）",
                error, error.node_id, error.field
            ),
        ));
    })?;
    if crate::vector_scene::dash_feature_required(&scene)
        && !object
            .get("required_features")
            .and_then(Value::as_array)
            .is_some_and(|features| {
                features
                    .iter()
                    .any(|v| v.as_str() == Some(crate::vector_scene::SCENE_DASH_FEATURE))
            })
    {
        diagnostics.push(map_error(
            file,
            "MAP002",
            "虚线样式缺少 presentation.vector_stroke_dash.v1 必需能力声明",
        ));
        return Err(());
    }
    let mut valid = true;
    for layer in scene.root_order.keys() {
        if !layers.contains_key(layer) {
            diagnostics.push(map_error(
                file,
                "MAP005",
                format!("scene 图层 `{layer}` 不存在"),
            ));
            valid = false;
        }
    }
    for (id, node) in &scene.nodes {
        if placements.contains_key(id) {
            diagnostics.push(map_error(
                file,
                "MAP014",
                format!("场景节点 `{id}` 与旧标记 ID 冲突"),
            ));
            valid = false;
        }
        for target in node.target_ref.iter().chain(&node.scope_refs) {
            if target.id.is_empty() || !crate::catalog::is_target_kind(&target.kind, options) {
                diagnostics.push(map_error(
                    file,
                    "MAP007",
                    format!("scene 节点 `{id}` 的对象引用类型/ID无效"),
                ));
                valid = false;
            } else if catalog.object(target).is_none() {
                diagnostics.push(map_warning(
                    file,
                    "MAP007",
                    format!(
                        "scene 节点 `{id}` 的目标 {}:{} 未解析",
                        target.kind, target.id
                    ),
                ));
            }
        }
        if let Some(nav) = &node.navigation {
            if !crate::workspace_documents::valid_id(&nav.map_id) {
                diagnostics.push(map_error(
                    file,
                    "MAP009",
                    format!("scene 节点 `{id}` 的地图导航ID无效"),
                ));
                valid = false;
            } else if !map_ids.contains(&nav.map_id) {
                diagnostics.push(map_warning(
                    file,
                    "MAP009",
                    format!("scene 节点 `{id}` 的导航地图未注册"),
                ));
            }
        }
    }
    if valid {
        Ok(Some(scene))
    } else {
        Err(())
    }
}

//! 旧采样 placement 兼容事务：一次解析、候选构造和提交，避免逐图元重写整图。
use super::*;
use crate::presentation_commands::{self as commands, EditError, Revision};
use crate::project::Project;
use std::path::PathBuf;

/// 保留旧采样几何契约；新作者入口使用 preview_scene 与 vector_scene::SceneBatch。
pub fn apply(
    project: &mut Project,
    revision: &mut Revision,
    map_id: &str,
    layer_id: &str,
    source: &str,
    expected_revision: Revision,
    expected_documents: BTreeMap<PathBuf, String>,
) -> Result<usize, EditError> {
    let invalid = |message| EditError::InvalidGeometry { message };
    if *revision != expected_revision {
        return Err(EditError::StaleRevision {
            expected: expected_revision,
            actual: *revision,
        });
    }
    project
        .ensure_workspace_writable()
        .map_err(|message| EditError::ReadOnlyFeature { message })?;
    let path = commands::map_document_path(project, map_id)?;
    if !expected_documents.contains_key(&path) {
        return Err(EditError::StaleContent {
            message: "缺少地图文档基线".into(),
        });
    }
    for (path, expected) in &expected_documents {
        let document = project
            .authoring_document(path)
            .map_err(|message| EditError::MissingReference { message })?;
        if document.is_read_only() {
            return Err(EditError::ReadOnlyFeature {
                message: "展示文档是只读".into(),
            });
        }
        let actual = commands::document_hash(document.bytes());
        if &actual != expected {
            return Err(EditError::ExternalConflict {
                path: path.clone(),
                expected: expected.clone(),
                actual,
            });
        }
    }
    project
        .checkpoint_disk_baselines_match()
        .map_err(|message| EditError::StaleContent { message })?;
    if !crate::workspace_documents::valid_id(layer_id) {
        return Err(invalid("SVG 图层ID无效".into()));
    }
    let preview = preview(source).map_err(invalid)?;
    let before = project
        .authoring_document(&path)
        .map_err(|message| EditError::MissingReference { message })?
        .bytes();
    let mut document = crate::workspace_documents::parse_unique_json(before).map_err(invalid)?;
    let root = document
        .as_object_mut()
        .ok_or_else(|| invalid("地图必须是JSON对象".into()))?;
    let layers = root
        .get_mut("layers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("地图缺少layers".into()))?;
    if layers.contains_key(layer_id) {
        return Err(invalid("SVG目标图层已存在".into()));
    }
    layers.insert(
        layer_id.into(),
        json!({"title":"SVG 绘图","visible_default":true,"locked":false}),
    );
    root.get_mut("layer_order")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("地图缺少layer_order".into()))?
        .push(Value::String(layer_id.into()));
    let scene_ids: BTreeMap<String, Value> = root
        .get("scene")
        .and_then(|s| s.get("nodes"))
        .and_then(Value::as_object)
        .map(|nodes| nodes.keys().map(|id| (id.clone(), Value::Null)).collect())
        .unwrap_or_default();
    let placements = root
        .get_mut("placements")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid("地图缺少placements".into()))?;
    for (index, shape) in preview.shapes.iter().enumerate() {
        let id = format!("{layer_id}_{:04}", index + 1);
        if placements.contains_key(&id) || scene_ids.contains_key(&id) {
            return Err(invalid(format!("SVG图元ID已存在：{id}")));
        }
        placements.insert(
            id,
            json!({"layer_id":layer_id,"target_ref":null,"geometry":shape.geometry,
            "annotation":"SVG 图形","role":"illustration","style":shape.style}),
        );
    }
    if preview.shapes.iter().any(|shape| {
        matches!(
            shape.geometry,
            MapGeometry::Polyline { .. } | MapGeometry::Polygon { .. }
        )
    }) {
        let features = root
            .entry("required_features")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| invalid("required_features必须为数组".into()))?;
        let feature = "presentation.geometry.line_area.v1";
        if !features.iter().any(|v| v.as_str() == Some(feature)) {
            features.push(Value::String(feature.into()));
        }
    }
    let after = serde_json::to_vec_pretty(&document).map_err(|e| invalid(e.to_string()))?;
    let compiled = project.compile_current();
    let manifest = crate::workspace_documents::manifest_path(&project.root);
    let registry = crate::workspace_documents::parse_registry(
        &project.root,
        project
            .authoring_document(&manifest)
            .map_err(invalid)?
            .bytes(),
    );
    let parsed = crate::presentation::parse_map_document(
        &after,
        &path,
        map_id,
        &compiled.analysis.catalog,
        &registry.maps.keys().cloned().collect(),
        compiled.options,
    );
    if parsed.document.is_none() {
        return Err(invalid(
            parsed
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("；"),
        ));
    }
    project
        .set_authoring_document(&path, after)
        .map_err(|message| EditError::StorageFailure { message })?;
    *revision = revision.next_presentation();
    Ok(preview.shapes.len())
}

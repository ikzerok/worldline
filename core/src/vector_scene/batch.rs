use super::{MapScene, SceneBatch, SceneError, SceneLimits, SceneOp, SceneProgress};
use crate::catalog::TargetRef;
use crate::presentation_commands::{
    document_hash, map_document_path, CommandResult, DocumentChange, Revision, UndoRecord,
};
use crate::project::Project;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 只能由 core 预览产生；不提供 Deserialize，也不接受调用方候选字节。
#[derive(Clone, Debug, Serialize)]
pub struct ScenePlan {
    pub map_id: String,
    pub affected_nodes: Vec<String>,
    pub affected_refs: Vec<TargetRef>,
    pub diagnostics: Vec<crate::Diagnostic>,
    pub expected_revision: Revision,
    pub document_hash: String,
    pub after_hash: String,
    pub operation_count: usize,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    before: Vec<u8>,
    #[serde(skip)]
    after: Vec<u8>,
    #[serde(skip)]
    baseline: String,
    #[serde(skip)]
    normalized: SceneBatch,
    #[serde(skip)]
    summary_hash: String,
}
impl ScenePlan {
    pub fn document_path(&self) -> &Path {
        &self.path
    }
    pub fn document_before(&self) -> &[u8] {
        &self.before
    }
    pub fn document_after(&self) -> &[u8] {
        &self.after
    }
    pub fn normalized_batch(&self) -> &SceneBatch {
        &self.normalized
    }
}

pub fn preview_batch(
    project: &Project,
    revision: Revision,
    batch: SceneBatch,
) -> Result<ScenePlan, SceneError> {
    preview_batch_with_control(
        project,
        revision,
        batch,
        &SceneLimits::default(),
        &mut |_| true,
    )
}

pub fn preview_batch_with_control(
    project: &Project,
    revision: Revision,
    mut batch: SceneBatch,
    limits: &SceneLimits,
    progress: &mut dyn FnMut(SceneProgress) -> bool,
) -> Result<ScenePlan, SceneError> {
    super::validate::check_limits(limits)?;
    pulse(progress, "validate", 0, batch.operations.len())?;
    if batch.operations.is_empty() || batch.operations.len() > limits.max_operations {
        return Err(super::validate::limit("批量操作数"));
    }
    check(project, revision, &batch)?;
    let path = map_document_path(project, &batch.map_id)
        .map_err(|e| SceneError::new("SCENE_REFERENCE", e.to_string()))?;
    let document = project.authoring_document(&path).map_err(storage)?;
    if document.is_read_only() {
        return Err(SceneError::new("SCENE_FEATURE", "地图只读，不能编辑"));
    }
    let batch_bytes = super::validate::serialized_size(&batch, limits.max_document_bytes)?;
    let before = document.bytes().to_vec();
    let mut working_bytes = before.len().saturating_add(batch_bytes);
    if before.len() > limits.max_document_bytes {
        return Err(super::validate::limit("地图字节数"));
    }
    let mut value = crate::workspace_documents::parse_unique_json(&before)
        .map_err(|e| SceneError::new("SCENE_SCHEMA", e))?;
    let content = project.compile_current();
    let registry = registry(project)?;
    let map_ids = registry.maps.keys().cloned().collect();
    let original = crate::presentation::parse_map_document(
        &before,
        &path,
        &batch.map_id,
        &content.analysis.catalog,
        &map_ids,
        content.options,
    )
    .document
    .ok_or_else(|| SceneError::new("SCENE_SCHEMA", "地图结构无效，不能生成写入计划"))?;
    let root = value
        .as_object_mut()
        .ok_or_else(|| SceneError::new("SCENE_SCHEMA", "地图必须是JSON对象"))?;
    let mut scene = original.scene.clone();
    let mut sources: BTreeMap<String, String> = scene
        .as_ref()
        .into_iter()
        .flat_map(|s| s.nodes.keys())
        .map(|id| (id.clone(), format!("scene:{id}")))
        .collect();
    let mut reorder_affected = BTreeSet::new();
    let mut warnings = Vec::new();
    let operation_count = batch.operations.len();
    for (index, op) in batch.operations.iter_mut().enumerate() {
        pulse(progress, "plan", index, operation_count)?;
        if let SceneOp::ImportSvg {
            layer_id,
            title,
            source,
        } = op
        {
            let preview = super::svg_scene::preview_scene_with_control(source, limits, progress)?;
            *op = SceneOp::ImportScene {
                layer_id: layer_id.clone(),
                title: title.clone(),
                scene: preview.scene,
                width: preview.width,
                height: preview.height,
            };
        }
        if matches!(op, SceneOp::EnableScene) {
            if scene.is_none() {
                scene = Some(MapScene::new(
                    original.canvas.width as f64,
                    original.canvas.height as f64,
                ));
            }
            enable(root)?;
        }
        let Some(scene) = scene.as_mut() else {
            return Err(SceneError::new(
                "SCENE_FEATURE",
                "请先显式 EnableScene 升级此地图",
            ));
        };
        if let SceneOp::Group { node_ids, .. } = op {
            let affected = super::edit_tree::group_reorder_affected(scene, node_ids);
            if !affected.is_empty() {
                warnings.push(crate::Diagnostic::warning(
                    "SCENE_GROUP_REORDER",
                    &path.to_string_lossy(),
                    crate::Span::new(1, 1, 1),
                    format!(
                        "非连续分组将整体置于最高选中位置，叠放受影响节点：{}",
                        affected.join("、")
                    ),
                ));
                reorder_affected.extend(affected);
            }
        }
        if let SceneOp::Duplicate { node_ids, .. } = op {
            let bytes = super::preserve::duplicate_bytes(
                scene,
                &original.source,
                &sources,
                node_ids,
                limits.max_document_bytes,
            )?;
            working_bytes = working_bytes.saturating_add(bytes);
            if working_bytes > limits.max_document_bytes.saturating_mul(2) {
                return Err(super::validate::limit("批量复制工作字节数"));
            }
        }
        super::operations::apply(scene, &original, root, op, &mut sources).map_err(
            |mut error| {
                error.operation_index = u32::try_from(index).ok();
                error
            },
        )?;
        if scene.nodes.len() > limits.max_nodes {
            return Err(super::validate::limit("场景节点数"));
        }
    }
    let scene = scene.ok_or_else(|| SceneError::new("SCENE_FEATURE", "缺少scene"))?;
    pulse(
        progress,
        "validate",
        batch.operations.len(),
        batch.operations.len(),
    )?;
    super::validate_scene(&scene, limits)?;
    let mut affected_refs = BTreeSet::new();
    for node in scene.nodes.values() {
        if let Some(target) = &node.target_ref {
            let old = original
                .scene
                .as_ref()
                .and_then(|s| s.nodes.get(&node.id))
                .and_then(|n| n.target_ref.as_ref())
                .or_else(|| {
                    original
                        .placements
                        .get(&node.id)
                        .and_then(|p| p.target_ref.as_ref())
                });
            if old != Some(target) && content.analysis.catalog.object(target).is_none() {
                return Err(
                    SceneError::new("SCENE_REFERENCE", "新增或改变的对象引用未解析")
                        .at(&node.id, "target_ref"),
                );
            }
            affected_refs.insert(target.clone());
        }
    }
    let mut serialized = serde_json::to_value(&scene).map_err(|e| storage(e.to_string()))?;
    super::preserve::fields(
        &original.source,
        &mut serialized,
        &sources,
        limits.max_document_bytes,
    )?;
    root.insert("scene".into(), serialized);
    let after = serde_json::to_vec_pretty(&value).map_err(|e| storage(e.to_string()))?;
    if after.len() > limits.max_document_bytes {
        return Err(super::validate::limit("地图字节数"));
    }
    let mut candidate = crate::presentation::parse_map_document(
        &after,
        &path,
        &batch.map_id,
        &content.analysis.catalog,
        &map_ids,
        content.options,
    );
    if candidate.document.is_none() {
        return Err(SceneError::new(
            "SCENE_SCHEMA",
            candidate
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("；"),
        ));
    }
    if before == after {
        return Err(SceneError::new("SCENE_STRUCTURE", "批量操作没有产生变更"));
    }
    candidate.diagnostics.extend(warnings);
    let old = original.scene.as_ref().map(|s| &s.nodes);
    let mut affected_nodes: BTreeSet<_> = scene
        .nodes
        .iter()
        .filter(|(id, node)| old.and_then(|nodes| nodes.get(*id)) != Some(*node))
        .map(|(id, _)| id.clone())
        .chain(
            old.into_iter()
                .flat_map(|nodes| nodes.keys())
                .filter(|id| !scene.nodes.contains_key(*id))
                .cloned(),
        )
        .collect();
    affected_nodes.extend(reorder_affected);
    pulse(progress, "ready", 1, 1)?;
    let mut plan = ScenePlan {
        map_id: batch.map_id.clone(),
        affected_nodes: affected_nodes.into_iter().collect(),
        affected_refs: affected_refs.into_iter().collect(),
        diagnostics: candidate.diagnostics,
        expected_revision: revision,
        document_hash: document_hash(&before),
        after_hash: document_hash(&after),
        operation_count: batch.operations.len(),
        path,
        before,
        after,
        baseline: project.content_baseline(),
        normalized: batch,
        summary_hash: String::new(),
    };
    plan.summary_hash = summary_hash(&plan)?;
    Ok(plan)
}

pub fn apply_batch(
    project: &mut Project,
    revision: &mut Revision,
    plan: &ScenePlan,
) -> Result<CommandResult, SceneError> {
    apply_batch_with_control(project, revision, plan, &mut |_| true)
}

pub fn apply_batch_with_control(
    project: &mut Project,
    revision: &mut Revision,
    plan: &ScenePlan,
    progress: &mut dyn FnMut(SceneProgress) -> bool,
) -> Result<CommandResult, SceneError> {
    pulse(progress, "commit", 0, 1)?;
    if summary_hash(plan)? != plan.summary_hash {
        return Err(SceneError::new(
            "SCENE_STALE",
            "计划摘要已被修改，请重新预览",
        ));
    }
    check(project, *revision, &plan.normalized)?;
    if project.content_baseline() != plan.baseline
        || project
            .authoring_document(&plan.path)
            .map_err(storage)?
            .bytes()
            != plan.before
    {
        return Err(SceneError::new(
            "SCENE_STALE",
            "计划之后工程内容已改变，请重新预览",
        ));
    }
    let previous = *revision;
    let next = revision.next_presentation();
    project
        .set_authoring_document(&plan.path, plan.after.clone())
        .map_err(storage)?;
    *revision = next;
    // 提交后不可把取消当作失败；最终通知仅报告完成。
    progress(SceneProgress {
        stage: "committed".into(),
        completed: 1,
        total: 1,
    });
    Ok(CommandResult {
        new_revision: next,
        changed_files: vec![plan.path.clone()],
        affected_refs: plan.affected_refs.clone(),
        diagnostics: plan.diagnostics.clone(),
        undo_record: UndoRecord {
            base_revision: previous,
            applied_revision: next,
            changes: vec![DocumentChange {
                path: plan.path.clone(),
                before: plan.before.clone(),
                after: plan.after.clone(),
            }],
        },
    })
}

fn check(project: &Project, revision: Revision, batch: &SceneBatch) -> Result<(), SceneError> {
    project
        .ensure_workspace_writable()
        .map_err(|e| SceneError::new("SCENE_FEATURE", e))?;
    if batch.expected_revision != revision {
        return Err(SceneError::new("SCENE_STALE", "场景修订已过期"));
    }
    let path = map_document_path(project, &batch.map_id)
        .map_err(|e| SceneError::new("SCENE_REFERENCE", e.to_string()))?;
    if !batch.expected_documents.contains_key(&path) {
        return Err(SceneError::new("SCENE_STALE", "缺少地图文档hash基线"));
    }
    for (path, expected) in &batch.expected_documents {
        let document = project
            .authoring_document(path)
            .map_err(|e| SceneError::new("SCENE_CONFLICT", e))?;
        if document.is_read_only() {
            return Err(SceneError::new("SCENE_FEATURE", "计划涉及只读文档"));
        }
        if &document_hash(document.bytes()) != expected {
            return Err(SceneError::new("SCENE_STALE", "展示文档hash已改变"));
        }
    }
    project
        .checkpoint_disk_baselines_match()
        .map_err(|e| SceneError::new("SCENE_CONFLICT", e))
}
fn registry(project: &Project) -> Result<crate::workspace_documents::Registry, SceneError> {
    let path = crate::workspace_documents::manifest_path(&project.root);
    Ok(crate::workspace_documents::parse_registry(
        &project.root,
        project.authoring_document(&path).map_err(storage)?.bytes(),
    ))
}
fn enable(root: &mut Map<String, Value>) -> Result<(), SceneError> {
    let features = root
        .entry("required_features")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| SceneError::new("SCENE_SCHEMA", "required_features 必须为数组"))?;
    if !features
        .iter()
        .any(|v| v.as_str() == Some(super::SCENE_FEATURE))
    {
        features.push(Value::String(super::SCENE_FEATURE.into()));
    }
    Ok(())
}
pub(super) fn pulse(
    progress: &mut dyn FnMut(SceneProgress) -> bool,
    stage: &str,
    completed: usize,
    total: usize,
) -> Result<(), SceneError> {
    if progress(SceneProgress {
        stage: stage.into(),
        completed,
        total,
    }) {
        Ok(())
    } else {
        Err(SceneError::new("SCENE_CANCELLED", "操作已取消，未修改工程"))
    }
}
fn storage(message: String) -> SceneError {
    SceneError::new("SCENE_STORAGE", message)
}

pub(super) fn summary_hash<T: Serialize>(value: &T) -> Result<String, SceneError> {
    serde_json::to_vec(value)
        .map(|bytes| document_hash(&bytes))
        .map_err(|error| SceneError::new("SCENE_STORAGE", error.to_string()))
}

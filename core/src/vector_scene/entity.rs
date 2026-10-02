use super::{apply_batch, preview_batch, SceneBatch, SceneEntityRequest, SceneError, SceneOp};
use crate::catalog::TargetRef;
use crate::presentation_commands::{document_hash, map_document_path, Revision};
use crate::project::Project;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize)]
pub struct SceneSourceChange {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Serialize)]
pub struct SceneEntityPlan {
    pub target: TargetRef,
    pub map_id: String,
    pub node_id: String,
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub after_baseline: String,
    pub changed_files: Vec<PathBuf>,
    pub source_changes: Vec<SceneSourceChange>,
    #[serde(skip)]
    candidate: Project,
    #[serde(skip)]
    request: SceneEntityRequest,
    #[serde(skip)]
    summary_hash: String,
}
impl SceneEntityPlan {
    pub fn request(&self) -> &SceneEntityRequest {
        &self.request
    }
}
impl std::fmt::Debug for SceneEntityPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneEntityPlan")
            .field("target", &self.target)
            .field("map_id", &self.map_id)
            .field("node_id", &self.node_id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SceneEntityResult {
    pub target: TargetRef,
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
    pub new_baseline: String,
}

pub fn preview_entity_binding(
    project: &Project,
    revision: Revision,
    request: SceneEntityRequest,
) -> Result<SceneEntityPlan, SceneError> {
    if request.expected_revision != revision
        || request.expected_baseline != project.content_baseline()
    {
        return Err(SceneError::new("SCENE_STALE", "建档绑定预览基线已过期"));
    }
    project.ensure_workspace_writable().map_err(failed)?;
    project
        .checkpoint_disk_baselines_match()
        .map_err(|e| SceneError::new("SCENE_CONFLICT", e))?;
    let source =
        crate::authoring_intents::require_active_source(project, &request.path).map_err(failed)?;
    let path = map_document_path(project, &request.map_id).map_err(|e| failed(e.to_string()))?;
    if request.expected_documents.get(&path)
        != Some(&document_hash(
            project.authoring_document(&path).map_err(failed)?.bytes(),
        ))
    {
        return Err(SceneError::new("SCENE_STALE", "建档绑定地图hash不匹配"));
    }
    let mut candidate = project.clone();
    candidate
        .write_entity(&source, None, &request.draft)
        .map_err(failed)?;
    let content = candidate.compile_current();
    if let Some(error) = content
        .diagnostics
        .iter()
        .find(|d| d.severity == crate::Severity::Error)
    {
        return Err(failed(format!("{}：{}", error.code, error.message)));
    }
    let target = TargetRef::new("entity", &request.draft.id);
    if content.analysis.catalog.object(&target).is_none() {
        return Err(failed("新实体未进入活动目录".into()));
    }
    let index = candidate.map_index();
    let map = index
        .maps
        .get(&request.map_id)
        .ok_or_else(|| failed("地图不存在或无效".into()))?;
    let mut node = map
        .scene
        .as_ref()
        .and_then(|scene| scene.nodes.get(&request.node_id))
        .cloned()
        .ok_or_else(|| failed("绑定scene节点不存在".into()))?;
    node.target_ref = Some(target.clone());
    let scene_plan = preview_batch(
        &candidate,
        revision,
        SceneBatch {
            map_id: request.map_id.clone(),
            expected_revision: revision,
            expected_documents: request.expected_documents.clone(),
            operations: vec![SceneOp::Update { node }],
        },
    )?;
    let mut candidate_revision = revision;
    apply_batch(&mut candidate, &mut candidate_revision, &scene_plan)?;
    let mut changed_files = vec![source.clone(), path];
    changed_files.sort();
    changed_files.dedup();
    let source_changes = vec![SceneSourceChange {
        path: source.clone(),
        before: project.document(&source).map_err(failed)?.into(),
        after: candidate.document(&source).map_err(failed)?.into(),
    }];
    let mut plan = SceneEntityPlan {
        target,
        map_id: request.map_id.clone(),
        node_id: request.node_id.clone(),
        expected_revision: revision,
        expected_baseline: project.content_baseline(),
        after_baseline: candidate.content_baseline(),
        changed_files,
        source_changes,
        candidate,
        request,
        summary_hash: String::new(),
    };
    plan.summary_hash = super::batch::summary_hash(&plan)?;
    Ok(plan)
}

pub fn apply_entity_binding(
    project: &mut Project,
    revision: &mut Revision,
    plan: &SceneEntityPlan,
) -> Result<SceneEntityResult, SceneError> {
    if super::batch::summary_hash(plan)? != plan.summary_hash
        || *revision != plan.expected_revision
        || project.content_baseline() != plan.expected_baseline
    {
        return Err(SceneError::new(
            "SCENE_STALE",
            "建档绑定计划已过期，请保留输入重新预览",
        ));
    }
    project.ensure_workspace_writable().map_err(failed)?;
    project
        .checkpoint_disk_baselines_match()
        .map_err(|e| SceneError::new("SCENE_CONFLICT", e))?;
    if !project.restore(plan.candidate.clone()) {
        return Err(SceneError::new(
            "SCENE_STALE",
            "工程刷新代次变化，不能提交旧计划",
        ));
    }
    revision.content_generation = revision.content_generation.wrapping_add(1);
    *revision = revision.next_presentation();
    Ok(SceneEntityResult {
        target: plan.target.clone(),
        changed_files: plan.changed_files.clone(),
        new_revision: *revision,
        new_baseline: project.content_baseline(),
    })
}
fn failed(message: String) -> SceneError {
    SceneError::new("SCENE_REFERENCE", message)
}

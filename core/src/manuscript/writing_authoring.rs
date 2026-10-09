//! 正文缓冲关联资料：已有引用只改草稿，新资料以明确的全文草稿集合一次应用。
mod prepare;
use super::WritingBuffer;
use crate::authoring_intents::{IntentTarget, TextSelection};
use crate::capabilities::CapabilityEnablePlan;
use crate::catalog::TargetRef;
use crate::project::Project;
use serde::Serialize;
use std::path::PathBuf;

pub const MAX_WRITING_AUTHORING_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_WRITING_AUTHORING_CHANGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize)]
pub struct WritingAuthoringRequest {
    pub expected_baseline: String,
    pub source: TargetRef,
    pub generation: u64,
    pub selection: TextSelection,
    pub target: IntentTarget,
    /// 仅表示作者明确要求预览 1.9→1.10；提交仍须确认整个候选计划。
    pub enable_entities: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct WritingAuthoringBuffer {
    pub path: PathBuf,
    pub generation: u64,
    pub changed: bool,
    pub source_hash: String,
    pub original_hash: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WritingAuthoringChange {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: String,
    /// 此文件的全部未应用输入随复合事务应用，而不只是选区。
    pub includes_unapplied_draft: bool,
}

#[derive(Clone, Serialize)]
pub struct WritingAuthoringPlan {
    pub target: TargetRef,
    pub source: TargetRef,
    pub source_path: PathBuf,
    pub cursor_utf8: usize,
    pub link_start_utf8: usize,
    pub changes: Vec<WritingAuthoringChange>,
    pub included_buffers: Vec<WritingAuthoringBuffer>,
    pub migration: Option<CapabilityEnablePlan>,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: u64,
    pub can_apply: bool,
    pub plan_digest: String,
    request: WritingAuthoringRequest,
    root: PathBuf,
    refresh_generation: u64,
}

impl WritingAuthoringPlan {
    pub fn request(&self) -> &WritingAuthoringRequest {
        &self.request
    }
    pub fn creates_object(&self) -> bool {
        !matches!(self.request.target, IntentTarget::Existing(_))
    }
    pub fn changed_files(&self) -> Vec<PathBuf> {
        self.changes
            .iter()
            .map(|change| change.path.clone())
            .collect()
    }
}

impl Project {
    /// 只读取当前 Project 与明确涉及的缓冲，既不刷新也不应用/保存。
    pub fn preview_writing_authoring(
        &self,
        buffers: &[WritingBuffer],
        request: &WritingAuthoringRequest,
    ) -> Result<WritingAuthoringPlan, String> {
        prepare::prepare(self, buffers, request).map(|(_, plan)| plan)
    }

    /// 已有目标插入留在唯一草稿。不能借本入口单独创建资料或升级清单。
    pub fn insert_writing_reference(
        &self,
        buffer: &mut WritingBuffer,
        plan: &WritingAuthoringPlan,
    ) -> Result<(), String> {
        if plan.creates_object() {
            return Err("新资料需要明确应用这组关联草稿，不能只插入孤立链接".into());
        }
        let (candidate, current) =
            prepare::prepare(self, std::slice::from_ref(buffer), &plan.request)?;
        verify_plan(plan, &current)?;
        buffer.replace_source(candidate.document(buffer.path())?.to_owned());
        Ok(())
    }

    /// 新资料声明、所列全文草稿、链接和显式迁移在一个候选内整体提交。
    pub fn apply_writing_authoring(
        &mut self,
        buffers: &[WritingBuffer],
        plan: &WritingAuthoringPlan,
    ) -> Result<(), String> {
        if !plan.creates_object() {
            return Err("已有对象链接请先插入正文草稿，再明确应用正文".into());
        }
        let (candidate, current) = prepare::prepare(self, buffers, &plan.request)?;
        verify_plan(plan, &current)?;
        *self = candidate;
        Ok(())
    }
}

fn verify_plan(plan: &WritingAuthoringPlan, expected: &WritingAuthoringPlan) -> Result<(), String> {
    if !expected.can_apply {
        return Err("语言迁移候选含错误，全部关联草稿未应用".into());
    }
    if serde_json::to_vec(plan).map_err(|error| error.to_string())?
        != serde_json::to_vec(expected).map_err(|error| error.to_string())?
    {
        return Err("关联计划、选区或全文草稿已变化，请保留输入并重新预览".into());
    }
    Ok(())
}

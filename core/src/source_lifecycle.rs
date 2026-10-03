//! 单源码生命周期：纯预览、完整重校验、一次内存提交；不改变语言能力。
mod error;
mod plan;
mod proof;
mod registered;
pub(crate) mod resources;
pub(crate) mod safety;
mod source;
use crate::project::Project;
use crate::refactor::RefactorOccurrence;
pub use error::{SourceLifecycleFailure, SourceLifecycleFailureKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use SourceLifecycleFailure as Failure;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceLifecycleRequest {
    Create { path: PathBuf },
    Include { path: PathBuf },
    Move { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLifecycleChange {
    pub path: PathBuf,
    pub after_path: PathBuf,
    pub kind: String,
    pub occurrences: Vec<RefactorOccurrence>,
    #[serde(skip)]
    pub(super) before: Option<Vec<u8>>,
    #[serde(skip)]
    pub(super) after: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLifecycleResource {
    pub source: PathBuf,
    pub field: String,
    pub before_path: String,
    pub after_path: String,
    pub resolved_before: PathBuf,
    pub resolved_after: PathBuf,
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLifecyclePlan {
    pub request: SourceLifecycleRequest,
    pub content_baseline: String,
    pub plan_digest: String,
    pub changes: Vec<SourceLifecycleChange>,
    pub source_path: Option<PathBuf>,
    pub destination_path: Option<PathBuf>,
    pub membership: String,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: u64,
    pub entry_before: String,
    pub entry_after: String,
    pub load_order_before: Vec<String>,
    pub load_order_after: Vec<String>,
    pub resources: Vec<SourceLifecycleResource>,
}

impl Project {
    pub fn preview_source_lifecycle(
        &self,
        request: &SourceLifecycleRequest,
    ) -> Result<SourceLifecyclePlan, String> {
        self.preview_source_lifecycle_cancellable(request, || false)
    }

    pub fn preview_source_lifecycle_cancellable(
        &self,
        request: &SourceLifecycleRequest,
        cancelled: impl FnMut() -> bool,
    ) -> Result<SourceLifecyclePlan, String> {
        self.preview_source_lifecycle_cancellable_classified(request, cancelled)
            .map_err(|failure| failure.message)
    }

    pub fn preview_source_lifecycle_classified(
        &self,
        request: &SourceLifecycleRequest,
    ) -> Result<SourceLifecyclePlan, SourceLifecycleFailure> {
        self.preview_source_lifecycle_cancellable_classified(request, || false)
    }

    pub fn preview_source_lifecycle_cancellable_classified(
        &self,
        request: &SourceLifecycleRequest,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SourceLifecyclePlan, SourceLifecycleFailure> {
        plan::prepare(self, request, &mut cancelled).map(|(_, plan)| plan)
    }

    /// 不保存；调用方捕获一个 Project 快照即可将全部修改作为一次撤销。
    pub fn apply_source_lifecycle(
        &mut self,
        request: &SourceLifecycleRequest,
        plan_digest: &str,
    ) -> Result<SourceLifecyclePlan, String> {
        self.apply_source_lifecycle_cancellable(request, plan_digest, || false)
    }

    pub fn apply_source_lifecycle_cancellable(
        &mut self,
        request: &SourceLifecycleRequest,
        plan_digest: &str,
        cancelled: impl FnMut() -> bool,
    ) -> Result<SourceLifecyclePlan, String> {
        self.apply_source_lifecycle_cancellable_classified(request, plan_digest, cancelled)
            .map_err(|failure| failure.message)
    }

    pub fn apply_source_lifecycle_classified(
        &mut self,
        request: &SourceLifecycleRequest,
        plan_digest: &str,
    ) -> Result<SourceLifecyclePlan, SourceLifecycleFailure> {
        self.apply_source_lifecycle_cancellable_classified(request, plan_digest, || false)
    }

    pub fn apply_source_lifecycle_cancellable_classified(
        &mut self,
        request: &SourceLifecycleRequest,
        plan_digest: &str,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<SourceLifecyclePlan, SourceLifecycleFailure> {
        let (candidate, plan) = plan::prepare(self, request, &mut cancelled)?;
        if plan.plan_digest != plan_digest {
            return Err(Failure::changed(
                "源码组织预览已过期或摘要不符，工程未修改；请重新预览",
            ));
        }
        check_cancelled(&mut cancelled)?;
        safety::revalidate(self, &plan)?;
        *self = candidate;
        Ok(plan)
    }

    pub fn apply_source_lifecycle_plan(
        &mut self,
        plan: &SourceLifecyclePlan,
    ) -> Result<SourceLifecyclePlan, String> {
        self.apply_source_lifecycle_plan_classified(plan)
            .map_err(|failure| failure.message)
    }

    pub fn apply_source_lifecycle_plan_classified(
        &mut self,
        plan: &SourceLifecyclePlan,
    ) -> Result<SourceLifecyclePlan, SourceLifecycleFailure> {
        if self.content_baseline() != plan.content_baseline {
            return Err(Failure::changed(
                "源码组织计划或逐处预览已变化，工程未修改；请重新预览",
            ));
        }
        let (candidate, expected) = plan::prepare(self, &plan.request, &mut || false)?;
        if &expected != plan {
            return Err(Failure::changed(
                "源码组织计划或逐处预览已变化，工程未修改；请重新预览",
            ));
        }
        safety::revalidate(self, &expected)?;
        *self = candidate;
        Ok(expected)
    }
}

fn check_cancelled(cancelled: &mut impl FnMut() -> bool) -> Result<(), Failure> {
    if cancelled() {
        Err("源码组织已取消，工程未修改".into())
    } else {
        Ok(())
    }
}

fn digest(bytes: &[u8]) -> String {
    crate::presentation_commands::document_hash(bytes)
}

use resources::resource_bytes;

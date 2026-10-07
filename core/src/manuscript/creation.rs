//! 一次创建章节与正式来源；旧 ManuscriptCommand 保持纯编排。
mod guards;
mod prepare;
mod wire;

use super::*;
use crate::project::Project;
use serde::{Deserialize, Serialize};

pub use wire::parse_manuscript_chapter_create_request;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManuscriptChapterCreateRequest {
    pub schema_version: u32,
    pub expected_baseline: String,
    #[serde(deserialize_with = "wire::revision")]
    pub expected_revision: Revision,
    pub book: ManuscriptBookDestination,
    pub chapter: ManuscriptChapterDraft,
    pub source: ManuscriptChapterSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManuscriptBookDestination {
    Existing { id: String },
    New { id: String, title: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManuscriptChapterDraft {
    pub id: String,
    pub title: String,
    pub parent_section_id: Option<String>,
    pub after_sibling_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManuscriptChapterSource {
    Existing {
        #[serde(deserialize_with = "wire::target")]
        target: TargetRef,
    },
    NewEvent {
        id: String,
        destination: ManuscriptSourceDestination,
        storyline: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManuscriptSourceDestination {
    ExistingActiveSource { relative_path: PathBuf },
    NewActiveSource { relative_path: PathBuf },
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptChapterChange {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptChapterCreatePlan {
    pub schema_version: u32,
    pub workspace: PathBuf,
    pub baseline: String,
    pub revision: Revision,
    pub book_id: String,
    pub chapter_id: String,
    pub target: TargetRef,
    pub source_path: PathBuf,
    pub new_source: bool,
    pub manuscript_path: PathBuf,
    pub changed_files: Vec<PathBuf>,
    pub changes: Vec<ManuscriptChapterChange>,
    pub diagnostics: Vec<Diagnostic>,
    pub entry_before: String,
    pub entry_after: String,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: u64,
    pub can_apply: bool,
    pub plan_digest: String,
    #[serde(skip)]
    guard: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptChapterCreateResult {
    pub book_id: String,
    pub chapter_id: String,
    pub target: TargetRef,
    pub source_path: PathBuf,
    pub changed_files: Vec<PathBuf>,
    pub new_baseline: String,
    pub new_revision: Revision,
    pub plan: ManuscriptChapterCreatePlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManuscriptChapterCreateFailureCode {
    StaleBaseline,
    IdConflict,
    InvalidDestination,
    InvalidChapter,
    ReadOnly,
    SourceUnavailable,
    ExternalConflict,
    BudgetExceeded,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptChapterCreateFailure {
    pub code: ManuscriptChapterCreateFailureCode,
    pub message: String,
}
impl ManuscriptChapterCreateFailure {
    fn new(code: ManuscriptChapterCreateFailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for ManuscriptChapterCreateFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for ManuscriptChapterCreateFailure {}
type Failure = ManuscriptChapterCreateFailure;
use ManuscriptChapterCreateFailureCode as Code;

impl Project {
    pub fn preview_manuscript_chapter_create(
        &self,
        revision: Revision,
        request: &ManuscriptChapterCreateRequest,
    ) -> Result<ManuscriptChapterCreatePlan, Failure> {
        prepare::prepare(self, revision, request).map(|(_, plan)| plan)
    }

    /// 完整重建计划和最后重验后仅替换一次 Project；不保存。
    pub fn apply_manuscript_chapter_create(
        &mut self,
        revision: &mut Revision,
        request: &ManuscriptChapterCreateRequest,
        plan_digest: &str,
    ) -> Result<ManuscriptChapterCreateResult, Failure> {
        let (candidate, plan) = prepare::prepare(self, *revision, request)?;
        if plan.plan_digest != plan_digest || !plan.can_apply {
            return Err(Failure::new(
                Code::StaleBaseline,
                "新章预览已过期或摘要不符；请保留输入并重新预览",
            ));
        }
        if guards::workspace(self)? != plan.guard {
            return Err(Failure::new(
                Code::ExternalConflict,
                "提交前工作区库存或保存基线已变化，整笔未提交",
            ));
        }
        guards::destinations(self, request)?;
        for path in &plan.changed_files {
            crate::source_lifecycle::safety::writable_path(&self.root.join(path))
                .map_err(|message| Failure::new(Code::ReadOnly, message))?;
        }
        let mut new_revision = revision.next_presentation();
        if matches!(request.source, ManuscriptChapterSource::NewEvent { .. }) {
            new_revision.content_generation = new_revision.content_generation.wrapping_add(1);
        }
        let result = ManuscriptChapterCreateResult {
            book_id: plan.book_id.clone(),
            chapter_id: plan.chapter_id.clone(),
            target: plan.target.clone(),
            source_path: plan.source_path.clone(),
            changed_files: plan.changed_files.clone(),
            new_baseline: candidate.content_baseline(),
            new_revision,
            plan,
        };
        *self = candidate;
        *revision = new_revision;
        Ok(result)
    }
}

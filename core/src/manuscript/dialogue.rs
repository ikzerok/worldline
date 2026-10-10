//! 正式单语句台词作者投影与受保护计划；规范见 spec/dialogue-authoring.md。
mod continuation;
mod parts;
mod prepare;
mod projection;
mod protocol;
mod writer;

use super::{ReviewKind, ReviewSource, ReviewSpeaker, WritingAuthoringChange, WritingBuffer};
use crate::{capabilities::CapabilityEnablePlan, project::Project, TargetRef};
pub use protocol::{parse_dialogue_edit_request, parse_dialogue_target};
use serde::{Deserialize, Serialize};
use std::{ops::Range, path::PathBuf};

pub const MAX_DIALOGUE_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_DIALOGUE_CHANGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DialogueError {
    pub code: String,
    pub message: String,
}
impl DialogueError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for DialogueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for DialogueError {}
type Result<T> = std::result::Result<T, DialogueError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogueKind {
    Text,
    Say,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DialoguePart {
    Literal {
        text: String,
    },
    Expression {
        source: String,
    },
    Link {
        #[serde(deserialize_with = "protocol::target")]
        target: TargetRef,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueDraft {
    pub kind: DialogueKind,
    #[serde(default, deserialize_with = "protocol::optional_target")]
    pub speaker: Option<TargetRef>,
    #[serde(default)]
    pub direction: Option<String>,
    pub parts: Vec<DialoguePart>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DialogueMappedPart {
    pub part: DialoguePart,
    pub source_range: Option<Range<usize>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueStatement {
    pub id: String,
    pub kind: DialogueKind,
    pub source: ReviewSource,
    pub draft: DialogueDraft,
    pub parts: Vec<DialogueMappedPart>,
    pub glue: bool,
    pub tags: Vec<String>,
    pub localization_id: Option<String>,
    pub after_anchor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueInsertionAnchor {
    pub id: String,
    pub line: u32,
    pub byte_offset: usize,
    pub label: String,
    #[serde(skip)]
    indent: String,
    #[serde(skip)]
    newline: String,
    #[serde(skip)]
    prefix_newline: bool,
    #[serde(skip)]
    suffix_newline: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueRow {
    pub depth: usize,
    pub label: String,
    pub statement_id: Option<String>,
    pub source: Option<ReviewSource>,
    pub kind: ReviewKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueProjection {
    pub schema_version: u32,
    pub target: TargetRef,
    pub baseline: String,
    pub generation: u64,
    pub snapshot: String,
    pub statements: Vec<DialogueStatement>,
    pub anchors: Vec<DialogueInsertionAnchor>,
    pub rows: Vec<DialogueRow>,
    pub speakers: Vec<ReviewSpeaker>,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DialogueOperation {
    Update {
        statement_id: String,
        draft: DialogueDraft,
    },
    Insert {
        anchor_id: String,
        draft: DialogueDraft,
    },
    Delete {
        statement_id: String,
    },
    Convert {
        statement_id: String,
        to: DialogueKind,
        #[serde(default, deserialize_with = "protocol::optional_target")]
        speaker: Option<TargetRef>,
        #[serde(default)]
        allow_direction_loss: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueEditRequest {
    pub schema_version: u32,
    pub expected_baseline: String,
    #[serde(deserialize_with = "protocol::target")]
    pub target: TargetRef,
    pub generation: u64,
    pub operation: DialogueOperation,
    #[serde(default)]
    pub enable_language_1_11: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DialogueEditPlan {
    pub schema_version: u32,
    pub request: DialogueEditRequest,
    pub baseline: String,
    pub generation: u64,
    pub snapshot: String,
    pub source_path: PathBuf,
    pub range: Range<usize>,
    pub before: String,
    pub after: String,
    pub old_speaker: Option<TargetRef>,
    pub new_speaker: Option<TargetRef>,
    pub metadata_losses: Vec<String>,
    pub migration: Option<CapabilityEnablePlan>,
    pub changes: Vec<WritingAuthoringChange>,
    pub includes_unapplied_draft: bool,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: u64,
    pub fingerprint_comparison_reliable: bool,
    pub can_apply: bool,
    pub no_change: bool,
    pub plan_digest: String,
    #[serde(skip)]
    root: PathBuf,
    #[serde(skip)]
    refresh_generation: u64,
    workspace_guard: String,
    #[serde(skip)]
    continuation: Option<continuation::Witness>,
}

impl Project {
    pub fn project_dialogue_buffer(
        &self,
        buffer: &WritingBuffer,
        target: &TargetRef,
    ) -> Result<DialogueProjection> {
        prepare::guard(self, buffer)?;
        let mut candidate = self.clone();
        candidate
            .set_text(buffer.path(), buffer.source().into())
            .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?;
        let compiled = candidate.compile_current();
        projection::build(self, buffer, target, &compiled)
    }

    pub fn preview_dialogue_edit(
        &self,
        buffer: &WritingBuffer,
        request: &DialogueEditRequest,
    ) -> Result<DialogueEditPlan> {
        prepare::prepare(self, buffer, request).map(|(_, plan)| plan)
    }

    pub fn stage_dialogue_edit(
        &self,
        buffer: &mut WritingBuffer,
        plan: &DialogueEditPlan,
    ) -> Result<()> {
        let (candidate, current) = prepare::prepare(self, buffer, &plan.request)?;
        verify(plan, &current)?;
        if current.no_change {
            return Ok(());
        }
        if current.migration.is_some() {
            return Err(DialogueError::new(
                "MIGRATION_REQUIRED",
                "语言迁移与完整正文草稿必须一起明确应用，不能只插入台词",
            ));
        }
        buffer.replace_source(
            candidate
                .document(buffer.path())
                .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?
                .into(),
        );
        Ok(())
    }

    pub fn apply_dialogue_edit(
        &mut self,
        buffer: &WritingBuffer,
        plan: &DialogueEditPlan,
    ) -> Result<()> {
        let (candidate, current) = prepare::prepare(self, buffer, &plan.request)?;
        verify(plan, &current)?;
        if !current.no_change {
            *self = candidate;
        }
        Ok(())
    }
}

fn verify(plan: &DialogueEditPlan, current: &DialogueEditPlan) -> Result<()> {
    if !current.can_apply {
        return Err(DialogueError::new(
            "CONFIRMATION_REQUIRED",
            "此台词计划尚未满足明确确认或迁移条件，请保留输入并重新预览",
        ));
    }
    if plan.continuation != current.continuation
        || plan.root != current.root
        || plan.refresh_generation != current.refresh_generation
        || serde_json::to_vec(plan).ok() != serde_json::to_vec(current).ok()
    {
        return Err(DialogueError::new(
            "STALE_DRAFT",
            "台词计划或全文草稿已变化，请保留输入并重新预览",
        ));
    }
    Ok(())
}

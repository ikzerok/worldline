//! 与旧 TodoProjection 独立的只读批注检索及当前稿选区捕获。
use super::{capture_text_anchor, AnchorStatus, CommentAnchor, CommentDraft, CommentIndex};
use crate::{project::Project, Diagnostic};
use serde::{Deserialize, Serialize};
use std::{ops::Range, path::Path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentResolutionFilter {
    #[default]
    Open,
    All,
    Resolved,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentAnchorFilter {
    #[default]
    All,
    Attached,
    Detached,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentReviewFilter {
    pub resolution: CommentResolutionFilter,
    pub anchor: CommentAnchorFilter,
    pub text: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct CommentReviewItem {
    pub draft: CommentDraft,
    pub document: String,
    pub anchor_status: AnchorStatus,
    pub read_only: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct CommentReviewProjection {
    pub schema_version: u32,
    pub total: usize,
    pub unresolved: usize,
    pub matched: usize,
    pub items: Vec<CommentReviewItem>,
    pub diagnostics: Vec<Diagnostic>,
}
impl CommentIndex {
    pub fn review_projection(&self, filter: &CommentReviewFilter) -> CommentReviewProjection {
        let text = filter.text.trim().to_lowercase();
        let items: Vec<_> = self
            .comments
            .values()
            .filter(|comment| {
                (match filter.resolution {
                    CommentResolutionFilter::All => true,
                    CommentResolutionFilter::Open => !comment.draft.resolved,
                    CommentResolutionFilter::Resolved => comment.draft.resolved,
                }) && (match filter.anchor {
                    CommentAnchorFilter::All => true,
                    CommentAnchorFilter::Attached => {
                        comment.anchor_status == AnchorStatus::Attached
                    }
                    CommentAnchorFilter::Detached => {
                        comment.anchor_status == AnchorStatus::Detached
                    }
                }) && (text.is_empty()
                    || [
                        &comment.draft.id,
                        &comment.draft.author,
                        &comment.draft.body,
                    ]
                    .iter()
                    .any(|value| value.to_lowercase().contains(&text)))
            })
            .map(|comment| CommentReviewItem {
                draft: comment.draft.clone(),
                document: comment.path.to_string_lossy().into_owned(),
                anchor_status: comment.anchor_status,
                read_only: comment.read_only,
            })
            .collect();
        CommentReviewProjection {
            schema_version: 1,
            total: self.comments.len(),
            unresolved: self
                .comments
                .values()
                .filter(|comment| !comment.draft.resolved)
                .count(),
            matched: items.len(),
            items,
            diagnostics: self.diagnostics.clone(),
        }
    }
}

/// v1 锚点覆盖完整行；拒绝任何不同于当前工程的草稿/旧选区，绝不暗中应用。
pub fn capture_text_selection(
    project: &Project,
    path: &Path,
    expected_source: &str,
    range: Range<usize>,
) -> Result<CommentAnchor, String> {
    project.ensure_workspace_writable()?;
    let source = project.document(path)?;
    if source != expected_source {
        return Err(
            "选区来自未应用或已过期的正文；请先明确应用此文件，再重新选择。未建立旧版锚点。".into(),
        );
    }
    if range.start >= range.end || source.get(range.clone()).is_none() {
        return Err("请选择有效的非空正文范围；没有自动选择其他段落。".into());
    }
    let start_line = source[..range.start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let end_at = if source.as_bytes()[range.end - 1] == b'\n' {
        range.end - 1
    } else {
        range.end
    };
    let end_line = source[..end_at]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    capture_text_anchor(project, path, start_line as u32, end_line as u32)
}

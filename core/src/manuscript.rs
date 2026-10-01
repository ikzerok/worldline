//! 书稿编排的解析、来源投影、分页与基线保护写入。
//!
//! Project 只读取清单明确注册的书稿，并通过统一展示文档缓冲提交修改；
//! 书稿不修改源码语义或运行指纹。

mod commands;
mod index;
mod organization;
mod reading;
#[cfg(test)]
mod reading_tests;
mod source;
mod writing;

#[cfg(test)]
mod writing_tests;
use crate::catalog::TargetRef;
use crate::presentation_commands::Revision;
use crate::Diagnostic;
pub use reading::{reading_projection, ReadingPart, ReadingProjection};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
pub use writing::{WritingBlock, WritingBlockKind, WritingBuffer, WritingProjection};

pub use index::build_manuscript_index;

pub const MANUSCRIPT_SCHEMA_VERSION: u64 = 1;
pub const MANUSCRIPT_REQUIRED_FEATURE: &str = "presentation.manuscripts.v1";
pub const MAX_MANUSCRIPT_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptEntryKind {
    Section,
    Chapter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptReferenceStatus {
    Resolved,
    Missing,
    Unresolved,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptReferenceRole {
    Source,
    Perspective,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptSourceLocation {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ManuscriptTextStats {
    pub han_characters: u64,
    pub words: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptSource {
    pub status: ManuscriptReferenceStatus,
    pub location: Option<ManuscriptSourceLocation>,
    pub stats: Option<ManuscriptTextStats>,
}

/// 一个已识别的节点。未知字段留在 `ManuscriptIndex::source_document` 中，
/// 写入端必须对原文档做局部更新，不能将此投影整体写回。
#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptEntry {
    pub id: String,
    pub kind: ManuscriptEntryKind,
    pub parent_id: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub perspective: Option<TargetRef>,
    pub perspective_status: Option<ManuscriptReferenceStatus>,
    pub status: Option<String>,
    pub goal: Option<String>,
    pub target_ref: Option<TargetRef>,
    pub source: Option<ManuscriptSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptChapterProjection {
    pub id: String,
    pub title: String,
    /// 从根到直接父项的 section ID；不包含本章 ID。
    pub section_path: Vec<String>,
    pub summary: Option<String>,
    pub perspective: Option<TargetRef>,
    pub perspective_status: Option<ManuscriptReferenceStatus>,
    pub status: Option<String>,
    pub goal: Option<String>,
    pub target_ref: Option<TargetRef>,
    pub source: Option<ManuscriptSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptPage {
    pub manuscript_id: Option<String>,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub chapters: Vec<ManuscriptChapterProjection>,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptReference {
    pub file: String,
    pub manuscript_id: String,
    pub chapter_id: String,
    pub role: ManuscriptReferenceRole,
    pub target: TargetRef,
}

/// 只读索引。`source_bytes` 与 `source_document` 保留原文，包含未知字段；
/// 不支持的版本或调用方标记为只读时，不生成章节投影。
#[derive(Debug, Clone)]
pub struct ManuscriptIndex {
    pub id: Option<String>,
    pub title: Option<String>,
    pub entries: Vec<ManuscriptEntry>,
    pub diagnostics: Vec<Diagnostic>,
    pub read_only: bool,
    file: String,
    source_bytes: Vec<u8>,
    source_document: Option<Value>,
    chapter_order: Vec<(usize, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManuscriptEntryDraft {
    pub id: String,
    pub kind: ManuscriptEntryKind,
    pub parent_id: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub pov: Option<TargetRef>,
    pub status: Option<String>,
    pub goal: Option<String>,
    pub target_ref: Option<TargetRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManuscriptDraft {
    pub id: String,
    pub title: String,
    /// 数组顺序是相同父项下的阅读顺序。
    pub entries: Vec<ManuscriptEntryDraft>,
}

impl ManuscriptDraft {
    pub fn from_index(index: &ManuscriptIndex) -> Self {
        Self {
            id: index.id.clone().unwrap_or_default(),
            title: index.title.clone().unwrap_or_default(),
            entries: index
                .entries
                .iter()
                .map(|entry| ManuscriptEntryDraft {
                    id: entry.id.clone(),
                    kind: entry.kind,
                    parent_id: entry.parent_id.clone(),
                    title: entry.title.clone(),
                    summary: entry.summary.clone(),
                    pov: entry.perspective.clone(),
                    status: entry.status.clone(),
                    goal: entry.goal.clone(),
                    target_ref: entry.target_ref.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManuscriptCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    /// `None` 创建新书稿；已有书稿的稳定 ID 不能改名。
    pub original: Option<String>,
    pub draft: ManuscriptDraft,
}

#[derive(Debug, Clone)]
pub struct ManuscriptResult {
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
}

impl ManuscriptIndex {
    pub fn source_bytes(&self) -> &[u8] {
        &self.source_bytes
    }

    pub fn source_document(&self) -> Option<&Value> {
        self.source_document.as_ref()
    }

    /// 以稳定的树前序返回章节页。游标只对当前文档和分析快照有效。
    pub fn page(&self, offset: usize, requested_limit: usize) -> ManuscriptPage {
        let total = self.chapter_order.len();
        let limit = requested_limit.clamp(1, MAX_MANUSCRIPT_PAGE_SIZE);
        let start = offset.min(total);
        let end = start.saturating_add(limit).min(total);
        let chapters = self.chapter_order[start..end]
            .iter()
            .filter_map(|(entry_index, section_path)| {
                let entry = self.entries.get(*entry_index)?;
                Some(ManuscriptChapterProjection {
                    id: entry.id.clone(),
                    title: entry.title.clone(),
                    section_path: section_path.clone(),
                    summary: entry.summary.clone(),
                    perspective: entry.perspective.clone(),
                    perspective_status: entry.perspective_status,
                    status: entry.status.clone(),
                    goal: entry.goal.clone(),
                    target_ref: entry.target_ref.clone(),
                    source: entry.source.clone(),
                })
            })
            .collect();
        ManuscriptPage {
            manuscript_id: self.id.clone(),
            offset: start,
            limit,
            total,
            chapters,
            next_offset: (end < total).then_some(end),
        }
    }

    /// 返回源目标和 POV 的反向引用，不读取或修改 Project。
    pub fn references_to(&self, target: &TargetRef) -> Vec<ManuscriptReference> {
        let Some(manuscript_id) = self.id.as_ref() else {
            return Vec::new();
        };
        let mut references = Vec::new();
        for entry in &self.entries {
            if entry.kind != ManuscriptEntryKind::Chapter {
                continue;
            }
            if let Some(reference) = entry.target_ref.as_ref().filter(|reference| {
                crate::deletion_content_references::affected_by_deletion(reference, target)
            }) {
                references.push(ManuscriptReference {
                    file: self.file.clone(),
                    manuscript_id: manuscript_id.clone(),
                    chapter_id: entry.id.clone(),
                    role: ManuscriptReferenceRole::Source,
                    target: reference.clone(),
                });
            }
            if entry.perspective.as_ref() == Some(target) {
                references.push(ManuscriptReference {
                    file: self.file.clone(),
                    manuscript_id: manuscript_id.clone(),
                    chapter_id: entry.id.clone(),
                    role: ManuscriptReferenceRole::Perspective,
                    target: target.clone(),
                });
            }
        }
        references
    }

    fn references_to_targets_with_status(
        &self,
    ) -> Vec<(ManuscriptReference, ManuscriptReferenceStatus)> {
        let Some(manuscript_id) = self.id.as_ref() else {
            return Vec::new();
        };
        let mut references = Vec::new();
        for entry in &self.entries {
            if entry.kind != ManuscriptEntryKind::Chapter {
                continue;
            }
            if let (Some(target), Some(source)) = (&entry.target_ref, &entry.source) {
                references.push((
                    ManuscriptReference {
                        file: self.file.clone(),
                        manuscript_id: manuscript_id.clone(),
                        chapter_id: entry.id.clone(),
                        role: ManuscriptReferenceRole::Source,
                        target: target.clone(),
                    },
                    source.status,
                ));
            }
            if let (Some(target), Some(status)) = (&entry.perspective, entry.perspective_status) {
                references.push((
                    ManuscriptReference {
                        file: self.file.clone(),
                        manuscript_id: manuscript_id.clone(),
                        chapter_id: entry.id.clone(),
                        role: ManuscriptReferenceRole::Perspective,
                        target: target.clone(),
                    },
                    status,
                ));
            }
        }
        references
    }
}

//! 不可变、全书筛选后分页的只读查询；不授权任何导航或提交。
mod build;
mod content;
#[cfg(test)]
mod draft_tests;
mod hierarchy;
mod input;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod observation_tests;
pub use input::{
    parse_manuscript_query_drafts, parse_manuscript_query_request, MAX_MANUSCRIPT_QUERY_INPUT_BYTES,
};
mod key;
mod page;
#[cfg(test)]
mod tests;
use super::{ManuscriptDraft, ManuscriptEntry, ManuscriptIndex};
use crate::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MANUSCRIPT_QUERY_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManuscriptQueryDraft {
    pub expected_baseline: String,
    pub draft: ManuscriptDraft,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptQueryView {
    #[default]
    Chapters,
    Tree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManuscriptQueryRequest {
    pub schema_version: u64,
    pub manuscript_id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub pov: String,
    #[serde(default)]
    pub section_id: Option<String>,
    #[serde(default)]
    pub view: ManuscriptQueryView,
    #[serde(default)]
    pub collapsed: Vec<String>,
    #[serde(default)]
    pub selected_id: Option<String>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_limit() -> usize {
    100
}
impl Default for ManuscriptQueryRequest {
    fn default() -> Self {
        Self {
            schema_version: MANUSCRIPT_QUERY_SCHEMA_VERSION,
            manuscript_id: String::new(),
            text: String::new(),
            status: String::new(),
            pov: String::new(),
            section_id: None,
            view: ManuscriptQueryView::default(),
            collapsed: Vec::new(),
            selected_id: None,
            offset: 0,
            limit: default_limit(),
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptQuerySource {
    Applied,
    Draft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptQueryWritingInput {
    pub file: String,
    pub generation: u64,
    pub source_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptSectionPath {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptQueryRow {
    pub entry: ManuscriptEntry,
    pub section_path: Vec<ManuscriptSectionPath>,
    pub perspective_display: Option<String>,
    /// 全书安全恢复前序中的零基位置，不随筛选或分页改变。
    pub ordinal: usize,
    pub identity_ambiguous: bool,
    pub context_only: bool,
    pub path_complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptQueryPage {
    pub schema_version: u64,
    pub manuscript_id: String,
    pub snapshot_key: String,
    pub source: ManuscriptQuerySource,
    pub complete: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub recognized_chapters: usize,
    pub matching_chapters: usize,
    pub total_rows: usize,
    pub offset: usize,
    pub limit: usize,
    pub next_cursor: Option<String>,
    pub rows: Vec<ManuscriptQueryRow>,
    /// 未选择时为 None；缺失、歧义或被筛选时为 Some(false)。
    pub selection_matches: Option<bool>,
    pub selection_ambiguous: bool,
    /// 当前可见结果中所选稳定 ID 的零基位置；折叠隐藏时为 None。
    pub selected_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptQueryError {
    pub code: &'static str,
    pub message: String,
}
impl ManuscriptQueryError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for ManuscriptQueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}：{}", self.code, self.message)
    }
}
impl std::error::Error for ManuscriptQueryError {}

#[derive(Debug, Clone)]
struct SearchRow {
    row: ManuscriptQueryRow,
    parent: Option<usize>,
    subtree_end: usize,
    text: String,
    status: String,
    pov: String,
}
#[derive(Debug, Clone)]
struct SearchBook {
    rows: Vec<SearchRow>,
    identities: BTreeMap<String, Vec<usize>>,
}
#[derive(Debug, Clone)]
pub struct ManuscriptQuerySnapshot {
    key: String,
    pub(super) input_key: String,
    pub(super) content: content::QueryContent,
    pub(super) unique_writing_paths: bool,
    pub(super) writing_inputs: Vec<ManuscriptQueryWritingInput>,
    pub(super) fresh_observation: Result<String, String>,
    indices: BTreeMap<String, ManuscriptIndex>,
    applied_indices: BTreeMap<String, ManuscriptIndex>,
    books: BTreeMap<String, SearchBook>,
    draft_ids: BTreeSet<String>,
    writing_draft: bool,
    diagnostics: Vec<Diagnostic>,
    complete: bool,
}
impl ManuscriptQuerySnapshot {
    pub(crate) fn compiled(&self) -> &crate::CompileResult {
        &self.content.0
    }

    pub fn key(&self) -> &str {
        &self.key
    }
    /// 工作区和当前正文编译诊断；各书稿文档诊断由其查询页追加。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    /// 整个快照是否可完整确认；查询页另给该书稿的完整状态。
    pub fn complete(&self) -> bool {
        self.complete
            && self.indices.values().all(|index| {
                !index.read_only
                    && index
                        .diagnostics
                        .iter()
                        .all(|item| item.severity != crate::Severity::Error)
            })
            && self
                .books
                .values()
                .all(|book| book.rows.iter().all(|row| row.row.path_complete))
    }
    /// 歧义身份仅可展示，不能任选其中一项作为作者正文位置。
    pub fn entry_is_ambiguous(&self, manuscript_id: &str, entry_id: &str) -> bool {
        self.indices
            .get(manuscript_id)
            .and_then(|index| index.original_id_counts.get(entry_id))
            .is_some_and(|count| *count > 1)
    }
    /// 原编排或显式编排草稿，正文来源均来自本快照的 WritingBuffer。
    pub fn indices(&self) -> &BTreeMap<String, ManuscriptIndex> {
        &self.indices
    }
    /// 应用编排原稿；与 indices 共用当前正文快照，不另编译旧正文。
    pub fn applied_indices(&self) -> &BTreeMap<String, ManuscriptIndex> {
        &self.applied_indices
    }
}

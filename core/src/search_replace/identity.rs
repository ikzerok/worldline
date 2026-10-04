use super::{SearchMatch, SearchRequest};
use crate::{manuscript::WritingBuffer, project::Project};
use std::{ops::Range, path::PathBuf, sync::Arc};

/// 仅 core 能生成的匹配身份；所有命中共享精确快照，不逐处复制全文。
#[derive(Clone, PartialEq, Eq)]
pub struct SearchMatchIdentity {
    pub(super) snapshot: Arc<SearchSnapshot>,
    pub(super) path: PathBuf,
    pub(super) range: Range<usize>,
}

impl std::fmt::Debug for SearchMatchIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchMatchIdentity")
            .field("path", &self.path)
            .field("range", &self.range)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SearchSnapshot {
    request: SearchRequest,
    root: PathBuf,
    entry: PathBuf,
    generation: u64,
    documents: Vec<(PathBuf, String, bool)>,
    authoring: Vec<(PathBuf, Vec<u8>, bool, bool)>,
    pub(super) drafts: Vec<DraftIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct DraftIdentity {
    path: PathBuf,
    baseline: String,
    generation: u64,
    source: String,
}

impl SearchSnapshot {
    pub(super) fn capture(
        project: &Project,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
    ) -> Self {
        let mut request = request.clone();
        // 替换文字属于计划，不属于查找身份。
        request.replacement.clear();
        let mut drafts: Vec<_> = drafts
            .iter()
            .filter(|d| d.is_changed() || d.generation() != 0)
            .map(DraftIdentity::from)
            .collect();
        drafts.sort();
        drafts.dedup();
        Self {
            request,
            root: project.root.clone(),
            entry: project.entry.clone(),
            generation: project.search_refresh_generation(),
            documents: project
                .documents
                .iter()
                .map(|(path, d)| (path.clone(), d.text.clone(), d.is_deleted()))
                .collect(),
            authoring: project
                .authoring_documents
                .iter()
                .map(|(path, d)| {
                    (
                        path.clone(),
                        d.bytes().to_vec(),
                        d.is_deleted(),
                        d.is_read_only(),
                    )
                })
                .collect(),
            drafts,
        }
    }

    pub(super) fn accepts_buffer(&self, buffer: &WritingBuffer) -> bool {
        if !buffer.is_changed() && buffer.generation() == 0 {
            return !self.drafts.iter().any(|d| d.path == buffer.path());
        }
        self.drafts.contains(&DraftIdentity::from(buffer))
    }
}

impl From<&WritingBuffer> for DraftIdentity {
    fn from(draft: &WritingBuffer) -> Self {
        Self {
            path: draft.path().to_owned(),
            baseline: draft.baseline().to_owned(),
            generation: draft.generation(),
            source: draft.source().to_owned(),
        }
    }
}

/// 元数据显示也须核对；旧范围、改造的保护标记或伪造强调不能授权写入。
pub(super) fn same_hit(a: &SearchMatch, b: &SearchMatch) -> bool {
    a.path == b.path
        && a.range == b.range
        && a.line == b.line
        && a.column == b.column
        && a.preview == b.preview
        && a.replaceable == b.replaceable
        && a.draft == b.draft
        && a.context == b.context
}

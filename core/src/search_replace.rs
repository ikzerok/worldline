//! 当前工程与唯一正文草稿的字面查找、保护 token 和显式原子替换。
mod context;
mod identity;
mod matching;
mod planning;
mod ranges;
use crate::{manuscript::WritingBuffer, project::Project};
pub use context::SearchContext;
pub use identity::SearchMatchIdentity;
pub use matching::literal_matches;
pub use planning::reconcile_search_selection;
use identity::SearchSnapshot;
use std::{collections::BTreeMap, ops::Range, path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self { case_sensitive: true, whole_word: false }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchScope {
    #[default]
    Prose,
    Source,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchFile {
    pub path: PathBuf,
    pub range: Option<Range<usize>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub query: String,
    pub replacement: String,
    pub options: SearchOptions,
    pub scope: SearchScope,
    pub files: Vec<SearchFile>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub line: u32,
    pub column: u32,
    /// 有界真实片段；强调与省略状态由 context 提供。
    pub preview: String,
    pub replaceable: bool,
    pub draft: bool,
    /// 纯跳转位置可为 None，不能作为替换选择。
    pub context: Option<SearchContext>,
    pub identity: Option<SearchMatchIdentity>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceChange {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
    pub count: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceOccurrence {
    pub path: PathBuf,
    pub before_range: Range<usize>,
    pub after_range: Range<usize>,
    pub before_context: SearchContext,
    pub after_context: SearchContext,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacePlan {
    pub changes: Vec<ReplaceChange>,
    pub hits: Vec<SearchMatch>,
    pub occurrences: Vec<ReplaceOccurrence>,
    request: SearchRequest,
    snapshot: Arc<SearchSnapshot>,
    selection: Option<Vec<SearchMatch>>,
}
impl ReplacePlan {
    pub fn request(&self) -> &SearchRequest { &self.request }
}
impl Project {
    pub fn search_drafts(
        &self,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
    ) -> Result<Vec<SearchMatch>, String> {
        let sources = self.search_sources(drafts, false)?;
        let snapshot = Arc::new(SearchSnapshot::capture(self, request, drafts));
        self.search_matches(request, drafts, &sources, &snapshot)
    }

    fn search_matches(
        &self,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
        sources: &BTreeMap<PathBuf, String>,
        snapshot: &Arc<SearchSnapshot>,
    ) -> Result<Vec<SearchMatch>, String> {
        let mut hits = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for file in &request.files {
            let path = if file.path.is_absolute() {
                file.path.clone()
            } else { self.root.join(&file.path) };
            crate::file_access::within(&self.root, &path)?;
            if !seen.insert(path.clone()) { return Err("搜索范围包含重复文件".into()); }
            let source = sources.get(&path).ok_or("搜索文件未载入")?;
            let selected = file.range.clone().unwrap_or(0..source.len());
            if source.get(selected.clone()).is_none() { return Err("搜索选区已失效".into()); }
            let mut prose = ranges::prose_ranges(source, self.compile_options());
            prose.sort_by_key(|range| range.start);
            let contexts = context::ContextIndex::new(source);
            for matched in matching::literal_match_iter(source, &request.query, request.options) {
                let range = matched.range;
                if range.start < selected.start || range.end > selected.end { continue; }
                let prose_index = prose.partition_point(|p| p.start <= range.start);
                let replaceable = prose_index.checked_sub(1).is_some_and(|index| prose[index].end >= range.end);
                if request.scope == SearchScope::Prose && !replaceable { continue; }
                if hits.len() == 10000 { return Err("命中超过10000处，请缩小范围".into()); }
                let context = contexts.context(range.clone());
                hits.push(SearchMatch {
                    path: path.clone(),
                    identity: Some(SearchMatchIdentity { snapshot: snapshot.clone(), path: path.clone(), range: range.clone() }),
                    range: range.clone(),
                    line: matched.line,
                    column: matched.column,
                    preview: context.text.clone(),
                    replaceable,
                    draft: drafts.iter().any(|d| d.path() == path && d.is_changed()),
                    context: Some(context),
                });
            }
        }
        Ok(hits)
    }

    /// 保持旧入口语义：全部命中一起预览，任一保护项拒绝整批。
    pub fn preview_search_replace(
        &self,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
    ) -> Result<ReplacePlan, String> {
        self.build_replace_plan(request, drafts, None, None)
    }

    /// 仅替换 core 搜索结果中明确选中的离散命中，不按新偏移重定位。
    pub fn preview_search_replace_selected(
        &self,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
        selected: &[SearchMatch],
    ) -> Result<ReplacePlan, String> {
        self.build_replace_plan(request, drafts, Some(selected), None)
    }

    /// 跨文件显式应用计划中的当前稿；一次内存事务，不保存。
    pub fn apply_search_replace(
        &mut self,
        plan: &ReplacePlan,
        drafts: &[WritingBuffer],
    ) -> Result<(), String> {
        self.validate_search_replace(plan, drafts)?;
        let mut candidate = self.clone();
        for change in &plan.changes { candidate.set_text(&change.path, change.after.clone())?; }
        *self = candidate;
        Ok(())
    }

    /// 当前稿仅返回安全候选，由调用方替换唯一 WritingBuffer 并保留撤销快照。
    pub fn replace_search_draft(
        &self,
        plan: &ReplacePlan,
        drafts: &[WritingBuffer],
        buffer: &WritingBuffer,
    ) -> Result<WritingBuffer, String> {
        self.validate_search_replace(plan, drafts)?;
        if plan.changes.len() != 1 || plan.changes[0].path != buffer.path()
            || plan.changes[0].before != buffer.source() || !plan.snapshot.accepts_buffer(buffer)
        { return Err("当前稿替换范围或代次不匹配".into()); }
        let mut next = buffer.clone();
        next.replace_source(plan.changes[0].after.clone());
        Ok(next)
    }

    fn validate_search_replace(&self, plan: &ReplacePlan, drafts: &[WritingBuffer]) -> Result<(), String> {
        let selected = plan.selection.as_deref();
        let rebuilt = self.build_replace_plan(&plan.request, drafts, selected, Some(&plan.snapshot))?;
        if rebuilt != *plan { return Err("替换预览已过期或被改动，请重新预览".into()); }
        Ok(())
    }

    fn search_sources(&self, drafts: &[WritingBuffer], strict: bool) -> Result<BTreeMap<PathBuf, String>, String> {
        let mut sources: BTreeMap<_, _> = self.documents.iter().filter(|(_, d)| !d.is_deleted())
            .map(|(p, d)| (p.clone(), d.text.clone())).collect();
        let mut unique = BTreeMap::new();
        let baseline = self.content_baseline();
        for draft in drafts.iter().filter(|draft| draft.is_changed()) {
            crate::file_access::within(&self.root, draft.path())?;
            self.document(draft.path())?;
            if strict && draft.baseline() != baseline { return Err("正文草稿基线已过期，替换未应用".into()); }
            if let Some(previous) = unique.insert(draft.path().to_owned(), draft.source()) {
                if previous != draft.source() { return Err("同源文件存在冲突草稿，不能自动选择版本".into()); }
            }
            sources.insert(draft.path().to_owned(), draft.source().to_owned());
        }
        Ok(sources)
    }
}

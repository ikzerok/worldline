//! 当前工程与唯一正文草稿的字面查找、保护 token 和显式原子替换。
mod matching;
mod ranges;
use crate::{manuscript::WritingBuffer, project::Project};
pub use matching::literal_matches;
use std::{collections::BTreeMap, ops::Range, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: true,
            whole_word: false,
        }
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
    pub preview: String,
    pub replaceable: bool,
    pub draft: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceChange {
    pub path: PathBuf,
    pub before: String,
    pub after: String,
    pub count: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacePlan {
    pub changes: Vec<ReplaceChange>,
    pub hits: Vec<SearchMatch>,
    request: SearchRequest,
    baseline: String,
    drafts: Vec<(PathBuf, String, u64, String)>,
}
impl ReplacePlan {
    pub fn request(&self) -> &SearchRequest {
        &self.request
    }
}
impl Project {
    pub fn search_drafts(
        &self,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
    ) -> Result<Vec<SearchMatch>, String> {
        let sources = self.search_sources(drafts, false)?;
        let mut hits = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for file in &request.files {
            let path = if file.path.is_absolute() {
                file.path.clone()
            } else {
                self.root.join(&file.path)
            };
            crate::file_access::within(&self.root, &path)?;
            if !seen.insert(path.clone()) {
                return Err("搜索范围包含重复文件".into());
            }
            let source = sources.get(&path).ok_or("搜索文件未载入")?;
            let selected = file.range.clone().unwrap_or(0..source.len());
            if source.get(selected.clone()).is_none() {
                return Err("搜索选区已失效".into());
            }
            let prose = ranges::prose_ranges(source, self.compile_options());
            for range in literal_matches(source, &request.query, request.options) {
                if range.start < selected.start || range.end > selected.end {
                    continue;
                }
                let replaceable = prose
                    .iter()
                    .any(|p| p.start <= range.start && p.end >= range.end);
                if request.scope == SearchScope::Prose && !replaceable {
                    continue;
                }
                let prefix = &source[..range.start];
                let line = prefix.bytes().filter(|b| *b == b'\n').count();
                hits.push(SearchMatch {
                    path: path.clone(),
                    range,
                    line: line as u32 + 1,
                    column: prefix.rsplit('\n').next().unwrap_or("").chars().count() as u32 + 1,
                    preview: source.lines().nth(line).unwrap_or("").into(),
                    replaceable,
                    draft: drafts.iter().any(|d| d.path() == path && d.is_changed()),
                });
                if hits.len() > 10000 {
                    return Err("命中超过10000处，请缩小范围".into());
                }
            }
        }
        Ok(hits)
    }
    pub fn preview_search_replace(
        &self,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
    ) -> Result<ReplacePlan, String> {
        self.ensure_workspace_writable()?;
        self.checkpoint_disk_baselines_match()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("保存事务冲突未解决".into());
        }
        if request.query.is_empty() {
            return Err("请输入非空查找文字".into());
        }
        if request.replacement.contains(['\n', '\r', '\\', '"']) {
            return Err("替换不得插入换行、引号或转义；请在编辑器手动修改".into());
        }
        let sources = self.search_sources(drafts, true)?;
        let hits = self.search_drafts(request, drafts)?;
        if hits.iter().any(|hit| !hit.replaceable) {
            return Err("命中包含声明、引用或保护 token；请缩小到正文范围".into());
        }
        let mut changes = Vec::new();
        for (path, before) in sources {
            let selected: Vec<_> = hits.iter().filter(|hit| hit.path == path).collect();
            if selected.is_empty() {
                continue;
            }
            let mut after = before.clone();
            for hit in selected.iter().rev() {
                after.replace_range(hit.range.clone(), &request.replacement);
            }
            if ranges::protected_signature(&before, self.compile_options())
                != ranges::protected_signature(&after, self.compile_options())
            {
                return Err("替换会改变正文结构或保护 token，原稿未改动".into());
            }
            changes.push(ReplaceChange {
                path,
                before,
                after,
                count: selected.len(),
            });
        }
        let mut identities: Vec<_> = drafts
            .iter()
            .map(|d| {
                (
                    d.path().to_owned(),
                    d.baseline().to_owned(),
                    d.generation(),
                    d.source().to_owned(),
                )
            })
            .collect();
        identities.sort();
        identities.dedup();
        Ok(ReplacePlan {
            changes,
            hits,
            request: request.clone(),
            baseline: self.content_baseline(),
            drafts: identities,
        })
    }
    /// 跨文件显式应用所有选中命中的当前稿；一次内存事务，不保存。
    pub fn apply_search_replace(
        &mut self,
        plan: &ReplacePlan,
        drafts: &[WritingBuffer],
    ) -> Result<(), String> {
        self.validate_search_replace(plan, drafts)?;
        let mut candidate = self.clone();
        for change in &plan.changes {
            candidate.set_text(&change.path, change.after.clone())?;
        }
        *self = candidate;
        Ok(())
    }
    /// 当前稿替换只返回安全候选，调用方替换唯一 WritingBuffer 并保留撤销快照。
    pub fn replace_search_draft(
        &self,
        plan: &ReplacePlan,
        drafts: &[WritingBuffer],
        buffer: &WritingBuffer,
    ) -> Result<WritingBuffer, String> {
        self.validate_search_replace(plan, drafts)?;
        if plan.changes.len() != 1
            || plan.changes[0].path != buffer.path()
            || plan.changes[0].before != buffer.source()
        {
            return Err("当前稿替换范围不匹配".into());
        }
        let mut next = buffer.clone();
        next.replace_source(plan.changes[0].after.clone());
        Ok(next)
    }
    fn validate_search_replace(
        &self,
        plan: &ReplacePlan,
        drafts: &[WritingBuffer],
    ) -> Result<(), String> {
        if self.preview_search_replace(&plan.request, drafts)? != *plan {
            return Err("替换预览已过期，请重新预览".into());
        }
        Ok(())
    }
    fn search_sources(
        &self,
        drafts: &[WritingBuffer],
        strict: bool,
    ) -> Result<BTreeMap<PathBuf, String>, String> {
        let mut sources: BTreeMap<_, _> = self
            .documents
            .iter()
            .filter(|(_, d)| !d.is_deleted())
            .map(|(p, d)| (p.clone(), d.text.clone()))
            .collect();
        let mut unique = BTreeMap::new();
        for draft in drafts.iter().filter(|draft| draft.is_changed()) {
            crate::file_access::within(&self.root, draft.path())?;
            self.document(draft.path())?;
            if strict && draft.baseline() != self.content_baseline() {
                return Err("正文草稿基线已过期，替换未应用".into());
            }
            if let Some(previous) = unique.insert(draft.path().to_owned(), draft.source()) {
                if previous != draft.source() {
                    return Err("同源文件存在冲突草稿，不能自动选择版本".into());
                }
            }
            sources.insert(draft.path().to_owned(), draft.source().to_owned());
        }
        Ok(sources)
    }
}

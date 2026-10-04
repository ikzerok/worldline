use super::{context, identity, ranges, ReplaceChange, ReplaceOccurrence, ReplacePlan, SearchMatch, SearchRequest, SearchSnapshot};
use crate::{manuscript::WritingBuffer, project::Project};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

impl Project {
    pub(super) fn build_replace_plan(
        &self,
        request: &SearchRequest,
        drafts: &[WritingBuffer],
        selected: Option<&[SearchMatch]>,
        expected: Option<&Arc<SearchSnapshot>>,
    ) -> Result<ReplacePlan, String> {
        self.ensure_workspace_writable()?;
        self.checkpoint_disk_baselines_match()?;
        if !self.recovery_conflicts().is_empty() { return Err("保存事务冲突未解决".into()); }
        if request.query.is_empty() { return Err("请输入非空查找文字".into()); }
        if request.replacement.contains(['\n', '\r', '\\', '"']) {
            return Err("替换不得插入换行、引号或转义；请在编辑器手动修改".into());
        }
        if selected.is_some_and(|hits| hits.is_empty()) { return Err("请明确选择至少一处命中".into()); }
        let sources = self.search_sources(drafts, true)?;
        let current = SearchSnapshot::capture(self, request, drafts);
        let first_identity = selected.and_then(|hits| hits.first()).and_then(|hit| hit.identity.as_ref());
        let existing = expected.or_else(|| first_identity.map(|id| &id.snapshot));
        let snapshot = match existing {
            Some(snapshot) if **snapshot == current => snapshot.clone(),
            Some(_) => return Err("搜索选择或替换预览已过期，请重新查找".into()),
            None => Arc::new(current),
        };
        let mut hits = self.search_matches(request, drafts, &sources, &snapshot)?;
        if let Some(selected) = selected {
            hits = reconcile_search_selection(&hits, selected)?;
        }
        if hits.iter().any(|hit| !hit.replaceable) {
            return Err("命中包含声明、引用或保护 token；请只选择可替换正文".into());
        }
        let mut changes = Vec::new();
        let mut occurrences_by_key = BTreeMap::new();
        let mut by_path = BTreeMap::<_, Vec<_>>::new();
        for hit in &hits { by_path.entry(hit.path.clone()).or_default().push(hit); }
        for (path, selected) in by_path {
            crate::source_lifecycle::safety::writable_path(&path)?;
            let before = sources.get(&path).ok_or("替换来源未载入")?;
            let mut after = String::with_capacity(before.len());
            let mut previous = 0;
            let mut after_ranges = Vec::new();
            for hit in &selected {
                after.push_str(&before[previous..hit.range.start]);
                let start = after.len();
                after.push_str(&request.replacement);
                after_ranges.push(start..after.len());
                previous = hit.range.end;
            }
            after.push_str(&before[previous..]);
            if ranges::protected_signature(before, self.compile_options())
                != ranges::protected_signature(&after, self.compile_options()) {
                return Err("替换会改变正文结构或保护 token，原稿未改动".into());
            }
            let before_contexts = context::ContextIndex::new(before);
            let after_contexts = context::ContextIndex::new(&after);
            for (hit, after_range) in selected.iter().zip(after_ranges) {
                occurrences_by_key.insert((path.clone(), hit.range.start, hit.range.end), ReplaceOccurrence {
                    path: path.clone(),
                    before_range: hit.range.clone(),
                    before_context: before_contexts.context(hit.range.clone()),
                    after_context: after_contexts.context(after_range.clone()),
                    after_range,
                });
            }
            changes.push(ReplaceChange { path, before: before.clone(), after, count: selected.len() });
        }
        let occurrences = hits.iter().map(|hit| {
            occurrences_by_key.remove(&(hit.path.clone(), hit.range.start, hit.range.end))
                .expect("每个选中命中均有逐处预览")
        }).collect();
        Ok(ReplacePlan {
            changes, occurrences, selection: selected.map(|_| hits.clone()), hits,
            request: request.clone(), snapshot,
        })
    }
}

/// 将旧选择批量对齐到本次 core 查找结果；每份精确快照仅比较一次。
///
/// 不删除失效项、不产生写权限；预览/应用仍须重新核对当前 Project 与草稿。
/// 纯导航合成位置不受理，空选择返回空集合。
pub fn reconcile_search_selection(
    hits: &[SearchMatch],
    selected: &[SearchMatch],
) -> Result<Vec<SearchMatch>, String> {
    if hits.len() > 10000 || selected.len() > 10000 {
        return Err("命中超过10000处，请缩小范围".into());
    }
    if selected.is_empty() { return Ok(Vec::new()); }
    let snapshot = &hits.first().and_then(|hit| hit.identity.as_ref())
        .ok_or("当前查找结果没有有效命中身份")?.snapshot;
    let mut current_snapshots = BTreeSet::from([Arc::as_ptr(snapshot)]);
    for hit in hits {
        let identity = hit.identity.as_ref().ok_or("当前查找结果混入只读导航位置")?;
        if identity.path != hit.path || identity.range != hit.range {
            return Err("当前查找结果的身份与位置不一致".into());
        }
        if current_snapshots.insert(Arc::as_ptr(&identity.snapshot)) && *identity.snapshot != **snapshot {
            return Err("当前查找结果混入不同快照".into());
        }
    }
    let candidates: BTreeMap<_, _> = hits.iter().map(|hit| {
        ((hit.path.clone(), hit.range.start, hit.range.end), hit)
    }).collect();
    if candidates.len() != hits.len() { return Err("当前查找结果包含重复命中".into()); }
    let mut checked_snapshots = current_snapshots;
    let mut chosen = BTreeSet::new();
    for hit in selected {
        let identity = hit.identity.as_ref().ok_or("只读导航位置没有可替换的搜索身份")?;
        if checked_snapshots.insert(Arc::as_ptr(&identity.snapshot)) && *identity.snapshot != **snapshot {
            return Err("选中项来自不同或过期的搜索，请重新选择".into());
        }
        if identity.path != hit.path || identity.range != hit.range {
            return Err("选中命中的身份与位置不一致".into());
        }
        let key = (hit.path.clone(), hit.range.start, hit.range.end);
        if !chosen.insert(key.clone()) { return Err("搜索选择包含重复命中".into()); }
        let candidate = candidates.get(&key).ok_or("选中位置不再是当前查询的命中")?;
        if !identity::same_hit(hit, candidate) { return Err("选中命中的预览或保护状态被改动".into()); }
    }
    Ok(hits.iter().filter(|hit| chosen.contains(&(hit.path.clone(), hit.range.start, hit.range.end))).cloned().collect())
}

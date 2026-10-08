use super::*;
use crate::manuscript::{ManuscriptQueryDraft, ManuscriptQueryView, WritingBuffer};
use crate::project::Project;
use std::collections::BTreeSet;

impl ManuscriptDeliverySnapshot {
    pub fn new(
        query: Arc<ManuscriptQuerySnapshot>,
        request: &ManuscriptDeliveryRequest,
    ) -> Result<Self, ManuscriptDeliveryError> {
        request.validate()?;
        if !query.unique_writing_paths {
            return Err(ManuscriptDeliveryError::new(
                "DUPLICATE_DRAFT",
                "交付要求每个源文件只有一个正文草稿；重复项未被去重",
            ));
        }
        if request
            .expected_snapshot_key
            .as_ref()
            .is_some_and(|key| key != query.key())
        {
            return Err(ManuscriptDeliveryError::new(
                "STALE_SNAPSHOT",
                "筛选快照已变化，请重新核对范围",
            ));
        }
        let mut normalized = request.clone();
        normalized.query.view = ManuscriptQueryView::Chapters;
        normalized.query.collapsed.clear();
        normalized.query.selected_id = None;
        normalized.query.offset = 0;
        normalized.query.cursor = None;
        normalized.query.limit = super::super::MAX_MANUSCRIPT_PAGE_SIZE;
        normalized.expected_snapshot_key = Some(query.key().into());
        let selected: Option<BTreeSet<&str>> = normalized
            .chapter_ids
            .as_ref()
            .map(|ids| ids.iter().map(String::as_str).collect());
        if selected
            .as_ref()
            .zip(normalized.chapter_ids.as_ref())
            .is_some_and(|(set, ids)| set.len() != ids.len())
        {
            return Err(ManuscriptDeliveryError::new(
                "DUPLICATE_SELECTION",
                "章节选择含重复 ID；未静默去重",
            ));
        }
        let first = query.query(&normalized.query)?;
        if selected.is_none() && first.matching_chapters > normalized.limits.chapters {
            return Err(ManuscriptDeliveryError::limit(
                "完整匹配范围超过交付专属4096章预算；请明确缩小筛选范围",
            ));
        }
        let mut rows = Vec::new();
        let mut metadata_bytes = 0usize;
        let mut found = BTreeSet::new();
        let mut page = first.clone();
        loop {
            for row in page.rows {
                if selected
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(row.entry.id.as_str()))
                {
                    continue;
                }
                if selected.is_some() && row.identity_ambiguous {
                    return Err(ManuscriptDeliveryError::new(
                        "AMBIGUOUS_SELECTION",
                        "所选章节 ID 有歧义，不能任选替身",
                    ));
                }
                found.insert(row.entry.id.clone());
                metadata_bytes = metadata_bytes
                    .saturating_add(serialized_size(&row, normalized.limits.scope_bytes)?);
                if metadata_bytes > normalized.limits.scope_bytes
                    || rows.len() >= normalized.limits.chapters
                {
                    return Err(ManuscriptDeliveryError::limit(
                        "完整范围身份/分节路径超过交付预算；未截断选择",
                    ));
                }
                rows.push(row);
            }
            let Some(cursor) = page.next_cursor else {
                break;
            };
            let mut next = normalized.query.clone();
            next.cursor = Some(cursor);
            page = query.query(&next)?;
        }
        if selected
            .as_ref()
            .is_some_and(|ids| ids.iter().any(|id| !found.contains(*id)))
        {
            return Err(ManuscriptDeliveryError::new(
                "SELECTION_OUTSIDE_SCOPE",
                "所选章不存在或不在当前筛选内；未扩大范围",
            ));
        }
        let sources: BTreeSet<_> = rows
            .iter()
            .filter_map(|row| row.entry.target_ref.as_ref())
            .collect();
        let source_occurrences = rows
            .iter()
            .filter(|row| row.entry.target_ref.is_some())
            .count();
        let scope = ManuscriptDeliveryScope {
            schema_version: MANUSCRIPT_DELIVERY_SCHEMA_VERSION,
            snapshot_key: query.key().into(),
            title: query
                .indices()
                .get(&normalized.query.manuscript_id)
                .and_then(|index| index.title.clone())
                .unwrap_or_default(),
            source: first.source,
            writing_inputs: query.writing_inputs.clone(),
            recognized_chapters: first.recognized_chapters,
            matching_chapters: first.matching_chapters,
            selected_occurrences: rows.len(),
            unique_sources: sources.len(),
            repeated_source_occurrences: source_occurrences.saturating_sub(sources.len()),
            complete: first.complete
                && rows
                    .iter()
                    .all(|row| !row.identity_ambiguous && row.path_complete),
            diagnostics: first.diagnostics,
            chapters: rows,
            request: normalized,
        };
        serialized_size(&scope, scope.request.limits.scope_bytes)?;
        Ok(Self { query, scope })
    }
}

impl Project {
    /// 明确生成/来源/交付动作的只读IO；浏览器仅读取当前授权挂载快照。
    /// 普通缓存命中/idle帧不得调用。本方法不证明附件内容字节未变，也不导出附件。
    pub fn verify_manuscript_delivery_observation(
        &self,
        snapshot: &ManuscriptQuerySnapshot,
    ) -> Result<(), ManuscriptDeliveryError> {
        self.verify_review_navigation()
            .map_err(|message| ManuscriptDeliveryError::new("STALE_OBSERVATION", message))?;
        let observed = self.problems_observation_key().map_err(|error| {
            ManuscriptDeliveryError::new("STALE_OBSERVATION", error.to_string())
        })?;
        if snapshot.fresh_observation.as_ref().ok() != Some(&observed) {
            return Err(ManuscriptDeliveryError::new(
                "STALE_OBSERVATION",
                "外部附件库存或可读性已变化；请刷新并重新生成，没有交付旧材料",
            ));
        }
        Ok(())
    }
    /// 只在明确生成动作使用；后续所有页和章节共享此一次编译。
    pub fn manuscript_delivery_snapshot(
        &self,
        buffers: &[WritingBuffer],
        drafts: &[ManuscriptQueryDraft],
        request: &ManuscriptDeliveryRequest,
    ) -> Result<ManuscriptDeliverySnapshot, ManuscriptDeliveryError> {
        let snapshot = Arc::new(self.manuscript_query_snapshot(buffers, drafts)?);
        ManuscriptDeliverySnapshot::new(snapshot, request)
    }

    /// 交付时核对全部当前输入及磁盘观察；不应用、不保存、不读回缺失源。
    pub fn validate_manuscript_delivery(
        &self,
        buffers: &[WritingBuffer],
        drafts: &[ManuscriptQueryDraft],
        report: &ManuscriptDeliveryReport,
    ) -> Result<(), ManuscriptDeliveryError> {
        if !report.complete || report.markdown.is_none() {
            return Err(ManuscriptDeliveryError::new(
                "INCOMPLETE_DELIVERY",
                "审稿范围或章节不完整，不能交付残稿",
            ));
        }
        let changed: Vec<_> = buffers
            .iter()
            .filter(|buffer| buffer.is_changed())
            .collect();
        if self.manuscript_query_key_refs(changed, drafts) != report.query.input_key {
            return Err(ManuscriptDeliveryError::new(
                "STALE_SNAPSHOT",
                "正文、编排或完整工程观察已变化，请重新生成",
            ));
        }
        self.verify_manuscript_delivery_observation(&report.query)
    }
}

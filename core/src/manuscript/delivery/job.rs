use super::*;
use crate::manuscript::{
    review_projection_with_snapshot, ManuscriptReferenceStatus, ReviewSnapshot,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptDeliveryProgress {
    pub completed: usize,
    pub total: usize,
    pub nodes: usize,
    pub review_bytes: usize,
    pub cancelled: bool,
    pub finished: bool,
}

pub struct ManuscriptDeliveryJob {
    snapshot: ManuscriptDeliverySnapshot,
    review_snapshot: Result<ReviewSnapshot, ManuscriptDeliveryError>,
    chapters: Vec<ManuscriptDeliveryChapter>,
    cache: BTreeMap<crate::TargetRef, (Arc<ReviewProjection>, usize)>,
    usage: ManuscriptDeliveryUsage,
    markdown: String,
    cancelled: bool,
    finished: bool,
}
impl ManuscriptDeliveryJob {
    pub fn new(snapshot: ManuscriptDeliverySnapshot) -> Result<Self, ManuscriptDeliveryError> {
        let review_snapshot = ReviewSnapshot::new(&snapshot.query.content.0)
            .map_err(|error| ManuscriptDeliveryError::new(&error.code, error.message));
        let markdown = markdown::header(&snapshot.scope)?;
        Ok(Self {
            snapshot,
            review_snapshot,
            chapters: Vec::new(),
            cache: BTreeMap::new(),
            usage: Default::default(),
            markdown,
            cancelled: false,
            finished: false,
        })
    }
    pub fn progress(&self) -> ManuscriptDeliveryProgress {
        ManuscriptDeliveryProgress {
            completed: if self.finished {
                self.snapshot.scope.selected_occurrences
            } else {
                self.chapters.len()
            },
            total: self.snapshot.scope.selected_occurrences,
            nodes: self.usage.nodes,
            review_bytes: self.usage.review_bytes,
            cancelled: self.cancelled,
            finished: self.finished,
        }
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.chapters.clear();
        self.cache.clear();
        self.markdown.clear();
    }
    /// 一次最多处理请求数量（1–100）的出现；单章仍受既有 ReviewProjection 硬预算。
    pub fn advance(
        &mut self,
        chapters: usize,
    ) -> Result<Option<ManuscriptDeliveryReport>, ManuscriptDeliveryError> {
        if self.cancelled {
            return Err(ManuscriptDeliveryError::new(
                "CANCELLED",
                "审稿生成已取消，没有交付产物",
            ));
        }
        if self.finished {
            return Err(ManuscriptDeliveryError::new(
                "FINISHED",
                "此生成作业已经结束",
            ));
        }
        let end = self
            .chapters
            .len()
            .saturating_add(chapters.clamp(1, 100))
            .min(self.snapshot.scope.chapters.len());
        while self.chapters.len() < end {
            let occurrence = self.chapters.len();
            let outcome = self.review_chapter(occurrence);
            let (review, review_bytes, error) = match outcome {
                Ok((review, bytes)) => {
                    self.usage.nodes = self.usage.nodes.saturating_add(review.node_count);
                    self.usage.review_bytes = self.usage.review_bytes.saturating_add(bytes);
                    if self.usage.nodes > self.snapshot.scope.request.limits.nodes
                        || self.usage.review_bytes > self.snapshot.scope.request.limits.review_bytes
                    {
                        self.cancel();
                        return Err(ManuscriptDeliveryError::limit(
                            "全部出现次数的审稿累计预算已满；重复源也计数，没有交付残稿",
                        ));
                    }
                    if let Err(error) = markdown::append_chapter(
                        &mut self.markdown,
                        &self.snapshot.scope,
                        occurrence,
                        &review,
                    ) {
                        self.cancel();
                        return Err(error);
                    }
                    (Some(review), bytes, None)
                }
                Err(error) => (None, 0, Some(error)),
            };
            self.chapters.push(ManuscriptDeliveryChapter {
                occurrence,
                review_bytes,
                review,
                error,
            });
        }
        if self.chapters.len() != self.snapshot.scope.chapters.len() {
            return Ok(None);
        }
        self.finished = true;
        let complete = self.snapshot.scope.complete
            && self
                .chapters
                .iter()
                .all(|chapter| chapter.review.is_some() && chapter.error.is_none());
        self.usage.markdown_bytes = if complete { self.markdown.len() } else { 0 };
        let report = ManuscriptDeliveryReport {
            scope: self.snapshot.scope.clone(),
            chapters: std::mem::take(&mut self.chapters),
            complete,
            usage: self.usage.clone(),
            markdown: complete.then(|| std::mem::take(&mut self.markdown)),
            query: self.snapshot.query.clone(),
        };
        serialized_size(&report, MAX_MANUSCRIPT_DELIVERY_RESPONSE_BYTES - 8192)?;
        Ok(Some(report))
    }
    fn review_chapter(
        &mut self,
        occurrence: usize,
    ) -> Result<(Arc<ReviewProjection>, usize), ManuscriptDeliveryError> {
        let row = &self.snapshot.scope.chapters[occurrence];
        if row.identity_ambiguous || !row.path_complete {
            return Err(ManuscriptDeliveryError::new(
                "AMBIGUOUS_CHAPTER",
                "章身份或分节路径未完整确认",
            ));
        }
        let target =
            row.entry.target_ref.as_ref().ok_or_else(|| {
                ManuscriptDeliveryError::new("MISSING_SOURCE", "章节没有正文来源")
            })?;
        if row
            .entry
            .source
            .as_ref()
            .is_none_or(|source| source.status != ManuscriptReferenceStatus::Resolved)
        {
            return Err(ManuscriptDeliveryError::new(
                "UNRESOLVED_SOURCE",
                "章节正文来源缺失、无效或无法确认；未按零字处理",
            ));
        }
        if let Some(cached) = self.cache.get(target) {
            return Ok(cached.clone());
        }
        let snapshot = self.review_snapshot.as_ref().map_err(Clone::clone)?;
        let review =
            review_projection_with_snapshot(&self.snapshot.query.content.0, target, snapshot)
                .map_err(|error| ManuscriptDeliveryError::new(&error.code, error.message))?;
        let bytes = serialized_size(&review, self.snapshot.scope.request.limits.review_bytes)?;
        let value = (Arc::new(review), bytes);
        self.cache.insert(target.clone(), value.clone());
        Ok(value)
    }
}

pub fn generate_manuscript_delivery(
    snapshot: ManuscriptDeliverySnapshot,
    progress: &mut dyn FnMut(&ManuscriptDeliveryProgress) -> bool,
) -> Result<ManuscriptDeliveryReport, ManuscriptDeliveryError> {
    let mut job = ManuscriptDeliveryJob::new(snapshot)?;
    loop {
        if !progress(&job.progress()) {
            job.cancel();
        }
        if let Some(report) = job.advance(1)? {
            return Ok(report);
        }
    }
}

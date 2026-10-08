//! 单书稿完整范围的作者私密审稿材料；不复用读者发布授权。
mod job;
mod markdown;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod performance_tests;
mod scope;
#[cfg(test)]
mod tests;

use super::{
    ManuscriptQueryRequest, ManuscriptQueryRow, ManuscriptQuerySnapshot, ManuscriptQuerySource,
    ReviewProjection,
};
use crate::Diagnostic;
pub use job::{generate_manuscript_delivery, ManuscriptDeliveryJob, ManuscriptDeliveryProgress};
#[cfg(not(target_arch = "wasm32"))]
pub use native::write_manuscript_markdown_new;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MANUSCRIPT_DELIVERY_SCHEMA_VERSION: u64 = 1;
pub const MAX_MANUSCRIPT_DELIVERY_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ManuscriptDeliveryLimits {
    pub chapters: usize,
    pub nodes: usize,
    pub scope_bytes: usize,
    pub review_bytes: usize,
    pub markdown_bytes: usize,
}
impl Default for ManuscriptDeliveryLimits {
    fn default() -> Self {
        Self {
            chapters: 4096,
            nodes: 100_000,
            scope_bytes: 4 * 1024 * 1024,
            review_bytes: 16 * 1024 * 1024,
            markdown_bytes: 8 * 1024 * 1024,
        }
    }
}
impl ManuscriptDeliveryLimits {
    fn validate(&self) -> Result<(), ManuscriptDeliveryError> {
        let max = Self::default();
        if [
            (self.chapters, max.chapters),
            (self.nodes, max.nodes),
            (self.scope_bytes, max.scope_bytes),
            (self.review_bytes, max.review_bytes),
            (self.markdown_bytes, max.markdown_bytes),
        ]
        .into_iter()
        .any(|(value, ceiling)| value == 0 || value > ceiling)
        {
            return Err(ManuscriptDeliveryError::new(
                "INVALID_LIMIT",
                "交付预算必须为正且不超过4096章、100000节点、4MiB范围、16MiB审稿、8MiB Markdown；未自动钳制",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManuscriptDeliveryRequest {
    pub schema_version: u64,
    pub query: ManuscriptQueryRequest,
    #[serde(default)]
    pub chapter_ids: Option<Vec<String>>,
    #[serde(default)]
    pub expected_snapshot_key: Option<String>,
    #[serde(default)]
    pub limits: ManuscriptDeliveryLimits,
}
impl ManuscriptDeliveryRequest {
    pub fn new(query: ManuscriptQueryRequest) -> Self {
        Self {
            schema_version: MANUSCRIPT_DELIVERY_SCHEMA_VERSION,
            query,
            chapter_ids: None,
            expected_snapshot_key: None,
            limits: Default::default(),
        }
    }
    pub fn validate(&self) -> Result<(), ManuscriptDeliveryError> {
        if self.schema_version != MANUSCRIPT_DELIVERY_SCHEMA_VERSION {
            return Err(ManuscriptDeliveryError::new(
                "UNSUPPORTED_VERSION",
                "不支持的书稿交付版本",
            ));
        }
        self.query
            .validate()
            .map_err(ManuscriptDeliveryError::from)?;
        self.limits.validate()?;
        if self.chapter_ids.as_ref().is_some_and(|ids| {
            ids.len() > self.limits.chapters || ids.iter().any(|id| id.is_empty() || id.len() > 256)
        }) || self
            .expected_snapshot_key
            .as_ref()
            .is_some_and(|key| key.len() > 128)
        {
            return Err(ManuscriptDeliveryError::limit(
                "选择身份或快照标识超过输入预算",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptDeliveryError {
    pub code: String,
    pub message: String,
}
impl ManuscriptDeliveryError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    fn limit(message: impl Into<String>) -> Self {
        Self::new("BUDGET_EXCEEDED", message)
    }
}
impl From<super::ManuscriptQueryError> for ManuscriptDeliveryError {
    fn from(error: super::ManuscriptQueryError) -> Self {
        Self::new(error.code, error.message)
    }
}
impl std::fmt::Display for ManuscriptDeliveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.code, self.message)
    }
}
impl std::error::Error for ManuscriptDeliveryError {}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptDeliveryScope {
    pub schema_version: u64,
    pub snapshot_key: String,
    pub request: ManuscriptDeliveryRequest,
    pub title: String,
    pub source: ManuscriptQuerySource,
    pub writing_inputs: Vec<super::ManuscriptQueryWritingInput>,
    pub recognized_chapters: usize,
    pub matching_chapters: usize,
    pub selected_occurrences: usize,
    pub unique_sources: usize,
    pub repeated_source_occurrences: usize,
    pub complete: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub chapters: Vec<ManuscriptQueryRow>,
}

#[derive(Debug, Clone)]
pub struct ManuscriptDeliverySnapshot {
    query: Arc<ManuscriptQuerySnapshot>,
    scope: ManuscriptDeliveryScope,
}
impl ManuscriptDeliverySnapshot {
    pub fn scope(&self) -> &ManuscriptDeliveryScope {
        &self.scope
    }
    pub fn query_snapshot(&self) -> &Arc<ManuscriptQuerySnapshot> {
        &self.query
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptDeliveryChapter {
    pub occurrence: usize,
    /// 此出现的完整ReviewProjection序列化字节数；重复源也逐次计量，错误项为0。
    pub review_bytes: usize,
    #[serde(serialize_with = "serialize_review")]
    pub review: Option<Arc<ReviewProjection>>,
    pub error: Option<ManuscriptDeliveryError>,
}
fn serialize_review<S: serde::Serializer>(
    review: &Option<Arc<ReviewProjection>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    review.as_deref().serialize(serializer)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ManuscriptDeliveryUsage {
    pub nodes: usize,
    pub review_bytes: usize,
    pub markdown_bytes: usize,
}

/// 只能由同快照作业创建；外部 JSON 无法反序列化为交付或来源权限。
#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptDeliveryReport {
    scope: ManuscriptDeliveryScope,
    chapters: Vec<ManuscriptDeliveryChapter>,
    complete: bool,
    usage: ManuscriptDeliveryUsage,
    markdown: Option<String>,
    #[serde(skip)]
    query: Arc<ManuscriptQuerySnapshot>,
}
impl ManuscriptDeliveryReport {
    pub fn scope(&self) -> &ManuscriptDeliveryScope {
        &self.scope
    }
    pub fn chapters(&self) -> &[ManuscriptDeliveryChapter] {
        &self.chapters
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn usage(&self) -> &ManuscriptDeliveryUsage {
        &self.usage
    }
    pub fn markdown(&self) -> Option<&str> {
        self.markdown.as_deref()
    }
    pub fn matches_snapshot(&self, snapshot: &ManuscriptQuerySnapshot) -> bool {
        self.scope.snapshot_key == snapshot.key()
    }
    pub fn contains_review(&self, review: &Arc<ReviewProjection>) -> bool {
        self.chapters.iter().any(|chapter| {
            chapter
                .review
                .as_ref()
                .is_some_and(|value| Arc::ptr_eq(value, review))
        })
    }
}

pub fn parse_manuscript_delivery_request(raw: &str) -> Result<ManuscriptDeliveryRequest, String> {
    if raw.len() > super::MAX_MANUSCRIPT_QUERY_INPUT_BYTES {
        return Err("书稿交付请求超过4MiB预算".into());
    }
    let value = crate::parse_unique_json(raw.as_bytes())?;
    let request: ManuscriptDeliveryRequest =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

pub(super) fn serialized_size(
    value: &impl Serialize,
    limit: usize,
) -> Result<usize, ManuscriptDeliveryError> {
    struct Counter {
        remaining: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.remaining = self
                .remaining
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("delivery_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { remaining: limit };
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| ManuscriptDeliveryError::limit("审稿材料超过字节预算；未截断交付"))?;
    Ok(limit - counter.remaining)
}

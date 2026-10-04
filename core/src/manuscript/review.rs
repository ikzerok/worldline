//! 静态全分支作者审稿：正式 AST、来源与完整快照，不触碰 runtime。
mod build;
mod source;
#[cfg(test)]
mod tests;
use crate::{CompileOptions, CompileResult, TargetRef};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const MAX_REVIEW_JSON_BYTES: usize = 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 10_000;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewError {
    pub code: String,
    pub message: String,
}
impl ReviewError {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    fn limit() -> Self {
        Self::new("review_limit", "审稿超过规模预算；请按章节分别审阅")
    }
    fn source() -> Self {
        Self::new(
            "source_unavailable",
            "无法证明审稿原文来源，未生成可导航投影",
        )
    }
}
impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for ReviewError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    Text,
    Say,
    If,
    Branch,
    ChoiceGroup,
    Choice,
    Scene,
    Call,
    Return,
    Divert,
    Structure,
    Description,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewPart {
    pub text: String,
    pub target: Option<TargetRef>,
    pub dynamic: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewSpeaker {
    pub display: String,
    pub target: TargetRef,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewSource {
    pub target: TargetRef,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub byte_start: usize,
    pub byte_end: usize,
    pub excerpt: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewNode {
    pub kind: ReviewKind,
    pub label: String,
    pub parts: Vec<ReviewPart>,
    pub children: Vec<ReviewNode>,
    pub source: Option<ReviewSource>,
    pub speaker: Option<ReviewSpeaker>,
    pub condition: Option<String>,
    pub enable: Option<String>,
    pub once: bool,
    pub disabled_reason: Option<String>,
    pub target: Option<TargetRef>,
    pub glue: bool,
    pub end_label: Option<String>,
}
impl ReviewNode {
    fn new(kind: ReviewKind, label: &str, source: Option<ReviewSource>) -> Self {
        Self {
            kind,
            label: label.into(),
            source,
            parts: Vec::new(),
            children: Vec::new(),
            speaker: None,
            condition: None,
            enable: None,
            once: false,
            disabled_reason: None,
            target: None,
            glue: false,
            end_label: None,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct ReviewProjection {
    pub schema_version: u32,
    pub target: TargetRef,
    pub snapshot: String,
    pub complete: bool,
    pub node_count: usize,
    pub nodes: Vec<ReviewNode>,
    #[serde(skip)]
    sources: std::sync::Arc<BTreeMap<PathBuf, String>>,
    #[serde(skip)]
    options: CompileOptions,
    #[serde(skip)]
    locations: Vec<ReviewSource>,
}

/// 一次编译的完整原文共享绑定；整书各章只拷贝 Arc。
#[derive(Debug, Clone)]
pub struct ReviewSnapshot {
    sources: std::sync::Arc<BTreeMap<PathBuf, String>>,
    options: CompileOptions,
    marker: String,
}
impl ReviewSnapshot {
    pub fn new(result: &CompileResult) -> Result<Self, ReviewError> {
        if result.has_errors() {
            return Err(ReviewError::new(
                "compile_failed",
                "当前稿件含编译错误，不能作为可信审稿",
            ));
        }
        let bytes = result
            .sources
            .values()
            .try_fold(0usize, |sum, text| sum.checked_add(text.len()));
        if bytes.is_none_or(|bytes| bytes > MAX_SOURCE_BYTES) {
            return Err(ReviewError::limit());
        }
        Ok(Self {
            sources: std::sync::Arc::new(result.sources.clone()),
            options: result.options,
            marker: source::snapshot(result),
        })
    }
}

/// 所选目标的完整静态投影。任何失败都不交付截断成功结果。
pub fn review_projection(
    result: &CompileResult,
    target: &TargetRef,
) -> Result<ReviewProjection, ReviewError> {
    let snapshot = ReviewSnapshot::new(result)?;
    review_projection_with_snapshot(result, target, &snapshot)
}

pub fn review_projection_with_snapshot(
    result: &CompileResult,
    target: &TargetRef,
    snapshot: &ReviewSnapshot,
) -> Result<ReviewProjection, ReviewError> {
    if result.has_errors()
        || result.options != snapshot.options
        || result.sources != *snapshot.sources
    {
        return Err(ReviewError::new("stale_review", "审稿源快照已过期"));
    }
    let mut builder = build::Builder::new(result, target);
    let nodes = builder.target()?;
    let projection = ReviewProjection {
        schema_version: 1,
        target: target.clone(),
        snapshot: snapshot.marker.clone(),
        complete: true,
        node_count: builder.count,
        nodes,
        sources: snapshot.sources.clone(),
        options: result.options,
        locations: builder.locations,
    };
    let mut counter = ByteBudget(MAX_REVIEW_JSON_BYTES);
    serde_json::to_writer(&mut counter, &projection).map_err(|_| ReviewError::limit())?;
    Ok(projection)
}

/// 必须传入最新全部当前稿编译结果；不刷新、应用或保存工程。
pub fn validate_review_source(
    result: &CompileResult,
    review: &ReviewProjection,
    location: &ReviewSource,
) -> Result<(), ReviewError> {
    if result.has_errors() || result.options != review.options || result.sources != *review.sources
    {
        return Err(ReviewError::new(
            "stale_review",
            "审稿已过期，请先刷新当前稿；原输入完整保留",
        ));
    }
    if location.target != review.target || !review.locations.contains(location) {
        return Err(ReviewError::source());
    }
    let text = result
        .sources
        .get(&PathBuf::from(&location.file))
        .ok_or_else(ReviewError::source)?;
    if text.get(location.byte_start..location.byte_end) != Some(location.excerpt.as_str()) {
        return Err(ReviewError::source());
    }
    Ok(())
}

struct ByteBudget(usize);
impl std::io::Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("review_limit"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

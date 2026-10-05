//! 当前单文件的正式语法结构投影；契约见 spec/source-outline.md。
use serde::Serialize;
use std::{ops::Range, path::PathBuf};
mod collect;
mod project;
mod ranges;

pub const MAX_SOURCE_OUTLINE_BYTES: usize = 512 * 1024;
pub const MAX_SOURCE_OUTLINE_LINES: usize = 16_384;
pub const MAX_SOURCE_OUTLINE_LINE_BYTES: usize = 16_384;
pub const MAX_SOURCE_OUTLINE_ENTRIES: usize = 4_096;
pub const MAX_SOURCE_OUTLINE_DEPTH: usize = 64;
pub const MAX_SOURCE_OUTLINE_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceOutlineStatus {
    Ready,
    SyntaxInvalid,
    BudgetExceeded,
    Inactive,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceOutlineEntry {
    pub occurrence: usize,
    pub parent: Option<usize>,
    pub depth: usize,
    pub kind: String,
    pub id: String,
    pub display: String,
    pub entity_type: Option<String>,
    pub line: u32,
    pub header: Range<usize>,
    pub body: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceOutline {
    pub path: PathBuf,
    pub status: SourceOutlineStatus,
    pub message: Option<String>,
    pub entries: Vec<SourceOutlineEntry>,
    #[serde(skip)]
    pub(crate) stamp: Stamp,
    #[serde(skip)]
    pub(crate) statements: Vec<Range<usize>>,
    #[serde(skip)]
    pub(crate) comments: Vec<Range<usize>>,
    #[serde(skip)]
    pub(crate) non_boundaries: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub source: (usize, u64),
    pub baseline: String,
    pub generation: u64,
    pub options: crate::CompileOptions,
}

impl SourceOutline {
    /// 只校验精确正文版本；磁盘、工程代次仍须由跳转守卫检查。
    pub fn matches_source(&self, source: &str) -> bool {
        self.stamp.source == signature(source)
    }

    /// 当前光标的最深语法归属，不代表故事运行位置。
    pub fn current_item(&self, byte_offset: usize) -> Option<&SourceOutlineEntry> {
        if self.status != SourceOutlineStatus::Ready
            || self.non_boundaries.binary_search(&byte_offset).is_ok()
            || self
                .comments
                .iter()
                .any(|range| range.contains(&byte_offset))
            || !self
                .statements
                .iter()
                .any(|range| range.contains(&byte_offset))
        {
            return None;
        }
        self.entries
            .iter()
            .filter(|entry| entry.body.contains(&byte_offset))
            .max_by_key(|entry| entry.depth)
    }
}

pub(crate) fn signature(source: &str) -> (usize, u64) {
    let hash = source.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    (source.len(), hash)
}

/// Count exact serialized bytes without allocating another serialized copy.
fn output_within_budget(outline: &SourceOutline, limit: usize) -> bool {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("结构输出超过预算"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Counter(limit), outline).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serialized_output_limit_is_exact_and_inclusive() {
        let outline = SourceOutline {
            path: PathBuf::from("世界🙂.wl"),
            status: SourceOutlineStatus::Ready,
            message: None,
            entries: Vec::new(),
            statements: Vec::new(),
            comments: Vec::new(),
            non_boundaries: Vec::new(),
            stamp: Stamp {
                source: signature(""),
                baseline: String::new(),
                generation: 0,
                options: crate::CompileOptions::default(),
            },
        };
        let bytes = serde_json::to_vec(&outline).unwrap().len();
        assert!(output_within_budget(&outline, bytes));
        assert!(!output_within_budget(&outline, bytes - 1));
    }
}

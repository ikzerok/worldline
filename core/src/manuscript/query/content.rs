//! 查询与连续审稿共用一次编译；Clone 仅复制 Arc。
use crate::CompileResult;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct QueryContent(pub(crate) Arc<CompileResult>);

impl std::fmt::Debug for QueryContent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueryContent")
            .field("source_count", &self.0.sources.len())
            .field("diagnostics", &self.0.diagnostics.len())
            .finish_non_exhaustive()
    }
}

use serde::{Deserialize, Serialize};

/// 当前阶段的计数；回调返回 false 时取消，未发布目标不得留下部分站点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderExportProgress {
    pub phase: String,
    pub completed: usize,
    pub total: usize,
}

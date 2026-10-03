//! 展示用结构化回执；已知守卫显式赋类，旧底层错误保守保留为无法证明。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLifecycleFailureKind {
    SourceChanged,
    IllegalPath,
    SemanticChange,
    UnableToProve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLifecycleFailure {
    pub kind: SourceLifecycleFailureKind,
    pub message: String,
}

impl SourceLifecycleFailure {
    pub(crate) fn changed(message: impl Into<String>) -> Self {
        Self {
            kind: SourceLifecycleFailureKind::SourceChanged,
            message: message.into(),
        }
    }
    pub(crate) fn path(message: impl Into<String>) -> Self {
        Self {
            kind: SourceLifecycleFailureKind::IllegalPath,
            message: message.into(),
        }
    }
    pub(crate) fn semantic(message: impl Into<String>) -> Self {
        Self {
            kind: SourceLifecycleFailureKind::SemanticChange,
            message: message.into(),
        }
    }
}
impl From<String> for SourceLifecycleFailure {
    fn from(message: String) -> Self {
        Self {
            kind: SourceLifecycleFailureKind::UnableToProve,
            message,
        }
    }
}
impl From<&str> for SourceLifecycleFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl std::fmt::Display for SourceLifecycleFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for SourceLifecycleFailure {}

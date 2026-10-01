//! 显式启用既有语言和资料能力；契约见 spec/language-versions.md。
mod catalog;
mod manifest;
mod preview;
mod transaction;

use crate::{Diagnostic, LanguageVersion};
pub use catalog::{
    feature_capabilities, language_capabilities, FeatureCapability, LanguageCapability,
};
use serde::Serialize;
use std::path::PathBuf;

/// 只追加明确选择的能力，不删除、降级或改写其他清单字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityEnableRequest {
    pub target_language: LanguageVersion,
    pub enable_features: Vec<String>,
    pub expected_baseline: String,
}

/// 正式 lexer 对同一物理行的分类改变；不是语义等价证明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityKeywordChange {
    pub file: String,
    pub line: u32,
    pub source: String,
    pub before_kind: String,
    pub after_kind: String,
}

/// 只读全稿预览。含错误的候选可以展示，但不可提交。
#[derive(Debug, Clone, Serialize)]
pub struct CapabilityEnablePlan {
    pub request: CapabilityEnableRequest,
    pub current_language: LanguageVersion,
    pub target_language: LanguageVersion,
    pub required_features_before: Vec<String>,
    pub required_features_after: Vec<String>,
    pub added_features: Vec<String>,
    pub diagnostics_before: Vec<Diagnostic>,
    pub diagnostics_after: Vec<Diagnostic>,
    pub new_diagnostics: Vec<Diagnostic>,
    pub keyword_changes: Vec<CapabilityKeywordChange>,
    pub runtime_fingerprint_before: u64,
    pub runtime_fingerprint_after: u64,
    pub fingerprint_comparison_reliable: bool,
    pub compatibility_notes: Vec<String>,
    pub manifest_changed: bool,
    pub can_apply: bool,
    #[serde(skip)]
    root: PathBuf,
    #[serde(skip)]
    manifest_before: Option<Vec<u8>>,
    #[serde(skip)]
    manifest_after: Vec<u8>,
}

impl CapabilityEnablePlan {
    /// 候选清单原始字节只读提供，不允许 UI 重编码后提交。
    /// 原工程无清单且没有选中变化时为空，不表示创建空文件。
    pub fn manifest_bytes_after(&self) -> &[u8] {
        &self.manifest_after
    }

    pub fn manifest_bytes_before(&self) -> Option<&[u8]> {
        self.manifest_before.as_deref()
    }
}

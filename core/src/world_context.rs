//! 同一编译快照上的有界世界对象上下文；不建立第二份正典索引。
use crate::{CatalogObject, Diagnostic, RelationDirection, RelationQueryDirection, TargetRef};
use serde::{Deserialize, Serialize};

mod collect;
mod executable;
pub use executable::{ExecutableContextIndex, ExecutableContextRole};
mod project;
mod query;

pub const EXECUTABLE_CONTEXT_CAPABILITY: &str = "authoring.executable_context.v1";
pub const WORLD_CONTEXT_CAPABILITY: &str = "authoring.world_context.v1";
pub const TEMPORAL_EXPLANATIONS_CAPABILITY: &str = "authoring.temporal_explanations.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldContextKind {
    FormalRelation,
    LegacyCharacterRelation,
    PropertyReference,
    EventParticipation,
    ExplicitBodyLink,
    TextMention,
    RuleCall,
    FragmentCall,
    GlobalRead,
    GlobalWrite,
}
impl WorldContextKind {
    pub fn is_executable(self) -> bool {
        matches!(
            self,
            Self::RuleCall | Self::FragmentCall | Self::GlobalRead | Self::GlobalWrite
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldContextOptions {
    pub depth: u8,
    pub direction: RelationQueryDirection,
    pub kinds: Vec<WorldContextKind>,
    pub include_text_mentions: bool,
    pub include_executable: bool,
    pub max_nodes: usize,
    pub max_records: usize,
    pub max_candidates: usize,
    pub expected_snapshot: Option<String>,
}
impl Default for WorldContextOptions {
    fn default() -> Self {
        Self {
            depth: 1,
            direction: RelationQueryDirection::Both,
            kinds: Vec::new(),
            include_text_mentions: false,
            include_executable: false,
            max_nodes: 250,
            max_records: 500,
            max_candidates: 10_000,
            expected_snapshot: None,
        }
    }
}
impl WorldContextOptions {
    pub fn executable_enabled(&self) -> bool {
        self.include_executable || self.kinds.iter().any(|kind| kind.is_executable())
    }
    pub fn validate(&self) -> Result<(), WorldContextError> {
        if !matches!(self.depth, 1 | 2)
            || !(1..=250).contains(&self.max_nodes)
            || self.max_records > 500
            || !(1..=100_000).contains(&self.max_candidates)
        {
            return Err(WorldContextError::InvalidOptions);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldContextIdentity {
    PersistentRelation,
    SnapshotOccurrence,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldContextPrecision {
    Line,
    Column,
}
#[derive(Debug, Clone, Serialize)]
pub struct WorldContextSource {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    pub precision: WorldContextPrecision,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorldContextProvenance {
    FormalRelation {
        relation_id: String,
        relation_type: String,
        scope_refs: Vec<TargetRef>,
    },
    LegacyCharacterRelation {
        occurrence: u32,
    },
    PropertyReference {
        property: String,
    },
    EventParticipation {
        event: String,
    },
    ExplicitBodyLink {
        label: String,
    },
    TextMention {
        preview: String,
    },
    Executable {
        context: ExecutableContextRole,
        occurrence: usize,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct WorldContextRecord {
    pub id: String,
    pub identity: WorldContextIdentity,
    pub kind: WorldContextKind,
    pub from_ref: TargetRef,
    pub to_ref: TargetRef,
    pub direction: RelationDirection,
    pub role: String,
    pub provenance: WorldContextProvenance,
    pub source: WorldContextSource,
}
#[derive(Debug, Clone, Serialize)]
pub struct WorldContextNode {
    pub target: TargetRef,
    pub display: String,
    pub depth: u8,
    pub exists: bool,
    pub file: Option<String>,
    pub line: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldContextLimit {
    InvalidSource,
    SourceConflict,
    CandidateBudget,
    NodeLimit,
    RecordLimit,
    ExecutableIndexBudget,
    SourceUnavailable,
}
#[derive(Debug, Clone, Serialize)]
pub struct WorldContextResult {
    pub schema_version: u32,
    pub target: TargetRef,
    pub object: CatalogObject,
    pub snapshot: String,
    pub content_baseline: Option<String>,
    pub nodes: Vec<WorldContextNode>,
    pub records: Vec<WorldContextRecord>,
    pub total: Option<usize>,
    pub returned: usize,
    pub complete: bool,
    pub truncated: bool,
    pub reasons: Vec<WorldContextLimit>,
    pub diagnostics: Vec<Diagnostic>,
}
impl WorldContextResult {
    /// 标明已知源码冲突；资料仍来自本结果的缓冲快照，不将冲突伪装成范围截断。
    pub fn mark_source_conflict(&mut self) {
        self.complete = false;
        if !self.reasons.contains(&WorldContextLimit::SourceConflict) {
            self.reasons.push(WorldContextLimit::SourceConflict);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldContextError {
    UnknownTarget(TargetRef),
    InvalidOptions,
    StaleSnapshot,
    Cancelled,
    SourceUnavailable(String),
}
impl WorldContextError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownTarget(_) => "UNKNOWN_TARGET",
            Self::InvalidOptions => "INVALID_OPTIONS",
            Self::StaleSnapshot => "STALE_SNAPSHOT",
            Self::Cancelled => "CANCELLED",
            Self::SourceUnavailable(_) => "SOURCE_UNAVAILABLE",
        }
    }
}
impl std::fmt::Display for WorldContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTarget(target) => write!(f, "对象不存在 {}:{}", target.kind, target.id),
            Self::InvalidOptions => write!(f, "上下文选项超出范围，请收窄深度或预算"),
            Self::StaleSnapshot => write!(f, "源码快照已变化，请重新查询当前对象"),
            Self::Cancelled => write!(f, "上下文查询已取消"),
            Self::SourceUnavailable(message) => write!(f, "无法确认源码冲突状态：{message}"),
        }
    }
}
impl std::error::Error for WorldContextError {}

/// 长度分帧的 FNV 内容摘要，仅用于失效检查，不是安全签名。
fn digest(parts: impl IntoIterator<Item = impl AsRef<[u8]>>) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for part in parts {
        let bytes = part.as_ref();
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

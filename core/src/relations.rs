//! 语言 1.10 的独立语义关系、旧人物关系投影与局部邻接查询。
//!
//! 关系只属于作者资料目录：它们不生成执行图、不改变运行状态，也不进入
//! `fingerprint_program`。旧 `character` 块中的 `relation` 仍由旧语法解析，
//! 这里仅为它生成可读的临时句柄。

use crate::ast::PropertyValue;
use crate::catalog::TargetRef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod catalog;
mod editor;
mod query;
mod source;

pub(crate) use catalog::collect;
pub use query::expand_period_scope_refs;
pub(crate) use query::relation_matches_scope;

/// 关系类型的方向。`Undirected` 只影响读取投影；源码仍只保存一条关系。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationDirection {
    #[default]
    Directed,
    Undirected,
}

/// 目录中的稳定关系类型身份与显示资料。
#[derive(Debug, Clone, Serialize)]
pub struct RelationTypeInfo {
    pub id: String,
    pub display: String,
    pub inverse_display: Option<String>,
    pub direction: RelationDirection,
    pub from_kind: Option<String>,
    pub to_kind: Option<String>,
    pub file: String,
    pub line: u32,
}

/// 目录中的独立关系实例。
#[derive(Debug, Clone, Serialize)]
pub struct SemanticRelationInfo {
    pub id: String,
    pub relation_type: String,
    pub from_ref: TargetRef,
    pub to_ref: TargetRef,
    pub description: String,
    pub source_note: Option<String>,
    pub scope_refs: Vec<TargetRef>,
    pub properties: BTreeMap<String, PropertyValue>,
    pub file: String,
    pub line: u32,
}

/// 旧 `CharacterRelation` 的临时显示身份。
///
/// `occurrence` 仅用于在同一源码对象的重复关系中区分行；它不是可持久化
/// 的关系 ID，也不能写入地图、批注或共享展示文档。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct LegacyRelationHandle {
    pub source: TargetRef,
    pub target: TargetRef,
    pub label: String,
    pub occurrence: u32,
    pub file: String,
    pub line: u32,
}

impl LegacyRelationHandle {
    pub fn is_persistent(&self) -> bool {
        false
    }

    pub fn stable_id(&self) -> Option<&str> {
        None
    }
}

/// 旧人物关系的兼容投影。`handle` 的字段会随着源码编辑重新计算。
#[derive(Debug, Clone, Serialize)]
pub struct LegacyRelationInfo {
    pub handle: LegacyRelationHandle,
}

/// 邻接查询方向筛选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelationQueryDirection {
    Outgoing,
    Incoming,
    #[default]
    Both,
}

/// 受限局部关系查询选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationQueryOptions {
    /// 同一快照和筛选下，跳过确定性遍历已经返回的关系数。
    #[serde(default)]
    pub offset: usize,
    /// 只允许 1 或 2；0 会被规范化为 1。
    pub depth: u8,
    pub relation_type: Option<String>,
    /// 多类型 OR 筛选；与单类型条件及方向条件取 AND。
    #[serde(default)]
    pub relation_types: Vec<String>,
    #[serde(default)]
    pub scope_refs: Vec<TargetRef>,
    #[serde(default)]
    pub include_unscoped: bool,
    pub direction: RelationQueryDirection,
    pub max_nodes: usize,
    pub max_edges: usize,
}

impl Default for RelationQueryOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            depth: 1,
            relation_type: None,
            relation_types: Vec::new(),
            scope_refs: Vec::new(),
            include_unscoped: false,
            direction: RelationQueryDirection::Both,
            max_nodes: 250,
            max_edges: 500,
        }
    }
}

impl RelationQueryOptions {
    pub fn bounded(mut self) -> Self {
        self.depth = self.depth.clamp(1, 2);
        // 起点始终是结果的一部分；即使调用方传入 0，也要返回一个封闭结果。
        self.max_nodes = self.max_nodes.clamp(1, 250);
        self.max_edges = self.max_edges.min(500);
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryNode {
    #[serde(rename = "ref")]
    pub target: TargetRef,
    pub depth: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryEdge {
    pub id: String,
    pub relation_type: String,
    pub from_ref: TargetRef,
    pub to_ref: TargetRef,
    pub label: String,
    pub direction: RelationDirection,
    pub source_note: Option<String>,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryContinuation {
    pub offset: usize,
    pub target: TargetRef,
    pub depth: u8,
    pub relation_type: Option<String>,
    /// 多类型 OR 筛选；与单类型条件及方向条件取 AND。
    #[serde(default)]
    pub relation_types: Vec<String>,
    #[serde(default)]
    pub scope_refs: Vec<TargetRef>,
    #[serde(default)]
    pub include_unscoped: bool,
    pub direction: RelationQueryDirection,
    /// 因上限未展开的实际边界对象；调用方可从这些对象继续请求更窄结果。
    pub frontier: Vec<TargetRef>,
}

/// 查询结果带明确版本和截断状态，供 CLI/RPC/编辑器直接消费。
#[derive(Debug, Clone, Serialize)]
pub struct RelationQueryResult {
    pub schema_version: u32,
    pub target: TargetRef,
    pub depth: u8,
    pub nodes: Vec<RelationQueryNode>,
    pub edges: Vec<RelationQueryEdge>,
    pub truncated: bool,
    pub continuation: Option<RelationQueryContinuation>,
}

/// 关系类型的结构编辑输入。
#[derive(Debug, Clone, Default)]
pub struct RelationTypeDraft {
    pub id: String,
    pub display: String,
    pub inverse_display: Option<String>,
    pub direction: RelationDirection,
    pub from_kind: Option<String>,
    pub to_kind: Option<String>,
}

/// 关系实例的结构编辑输入。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RelationDraft {
    pub id: String,
    pub relation_type: String,
    pub from: TargetRef,
    pub to: TargetRef,
    pub description: String,
    pub source_note: Option<String>,
    pub scope_refs: Vec<TargetRef>,
    pub properties: Vec<(String, PropertyValue)>,
}

/// 旧人物关系提升时使用的简化资料输入。
#[derive(Debug, Clone, Default)]
pub struct LegacyRelationPromotionDraft {
    pub relation_id: String,
    pub relation_type: String,
    pub description: String,
    pub source_note: Option<String>,
}

/// 关系提升的预览结果。预览期间不改 Project。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelationPromotionPreview {
    pub handle: LegacyRelationHandle,
    /// 预览时的内容基线；提交必须与当前工程完全一致。
    pub content_baseline: String,
    /// 预览使用的完整关系草稿，包含 scope 与 properties。
    pub draft: RelationDraft,
    pub relation_id: String,
    pub relation_type: String,
    pub description: String,
    pub source_note: Option<String>,
    pub before_fingerprint: u64,
    pub after_fingerprint: u64,
    pub fingerprint_changed: bool,
}

impl RelationPromotionPreview {
    fn draft(&self) -> RelationDraft {
        self.draft.clone()
    }
}

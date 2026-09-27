use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::ast::{PropertyValue, ValueKind};
use crate::diagnostic::Span;
use crate::graph::{AnchorDecl, RelationGraph};
// ---------------------------------------------------------------------------
// 符号表
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct VarInfo {
    pub kind: Option<ValueKind>,
    pub is_const: bool,
    pub decl_file: String,
    pub decl_span: Span,
    pub read: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorylineInfo {
    pub display: String,
    /// 是否来自显式 storyline 声明(否则为事件归属的隐式线)。
    pub declared: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CharacterInfo {
    pub display: String,
    pub decl_file: String,
    pub decl_span: Span,
    pub properties: BTreeMap<String, PropertyValue>,
    pub relations: Vec<CharacterRelationInfo>,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CharacterRelationInfo {
    pub target: String,
    pub label: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldInfo {
    pub id: String,
    pub display: String,
    pub description: String,
    pub properties: BTreeMap<String, PropertyValue>,
    pub file: String,
    pub line: u32,
}

/// 节点定位:事件索引 + 从事件根到该节点的场景叶名路径。
#[derive(Debug, Clone, Serialize)]
pub struct NodePath {
    pub event: usize,
    /// 场景叶名序列;空 = 事件本身。
    pub scenes: Vec<String>,
}

impl NodePath {
    pub fn full_name(&self, event_name: &str) -> String {
        if self.scenes.is_empty() {
            event_name.to_string()
        } else {
            format!("{}.{}", event_name, self.scenes.join("."))
        }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Symbols {
    /// 事件名 → 路径。
    pub events: HashMap<String, NodePath>,
    /// 场景全名("event.scene[.scene]") → 路径。
    pub scenes: HashMap<String, NodePath>,
    /// 事件名列表(按声明序)。
    pub event_order: Vec<String>,
    pub vars: HashMap<String, VarInfo>,
    /// 故事线(id → 信息)与声明序。
    pub storylines: HashMap<String, StorylineInfo>,
    pub storyline_order: Vec<String>,
    /// 角色(id → 信息)与声明序。
    pub characters: HashMap<String, CharacterInfo>,
    pub character_order: Vec<String>,
}

impl Symbols {
    /// 跃迁目标解析:当前事件内的场景(叶名或全名)优先,其次全局事件。
    pub fn resolve_target(&self, target: &str, current_event: Option<&str>) -> Option<NodePath> {
        if let Some(ev) = current_event {
            if let Some(p) = self.scenes.get(target) {
                if p.event == self.events.get(ev).map(|e| e.event).unwrap_or(usize::MAX) {
                    return Some(p.clone());
                }
            }
            let prefix = format!("{ev}.");
            for (name, path) in &self.scenes {
                if let Some(rest) = name.strip_prefix(&prefix) {
                    if rest == target || rest.split('.').next() == Some(target) {
                        return Some(path.clone());
                    }
                }
            }
        }
        self.events.get(target).cloned()
    }

    /// visits(x) 解析:全局事件、全名场景、唯一叶名场景。
    pub fn resolve_node(&self, name: &str) -> Option<NodePath> {
        if let Some(p) = self.events.get(name).or_else(|| self.scenes.get(name)) {
            return Some(p.clone());
        }
        let mut hits: Vec<&NodePath> = self
            .scenes
            .iter()
            .filter(|(n, _)| n.rsplit('.').next() == Some(name))
            .map(|(_, p)| p)
            .collect();
        if hits.len() == 1 {
            Some(hits.remove(0).clone())
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 统计
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Stats {
    pub events: u32,
    pub scenes: u32,
    pub choices: u32,
    pub words: u32,
    pub storylines: u32,
    pub characters: u32,
    pub entities: u32,
}

// ---------------------------------------------------------------------------
// 分析产物
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Analysis {
    pub catalog: crate::catalog::Catalog,
    pub timeline: crate::timeline::Timeline,
    pub world: Option<WorldInfo>,
    pub symbols: Symbols,
    pub graph: RelationGraph,
    pub stats: Stats,
    /// 源码中的锚点声明(按源码序)。
    pub anchors: Vec<AnchorDecl>,
    /// 程序内容指纹:存档兼容性校验用。
    pub fingerprint: u64,
}

impl Clone for Analysis {
    fn clone(&self) -> Self {
        Analysis {
            catalog: self.catalog.clone(),
            timeline: self.timeline.clone(),
            world: self.world.clone(),
            symbols: self.symbols.clone(),
            graph: self.graph.clone(),
            stats: self.stats,
            anchors: self.anchors.clone(),
            fingerprint: self.fingerprint,
        }
    }
}

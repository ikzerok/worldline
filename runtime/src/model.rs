//! 运行时公开数据模型与持久化载荷。

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Value {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Value::Num(_) => "数值",
            Value::Str(_) => "字符串",
            Value::Bool(_) => "布尔",
        }
    }

    /// 展示形式:数值去掉多余的 `.0`。
    pub fn display(&self) -> String {
        match self {
            Value::Num(number) => format_number(*number),
            Value::Str(text) => text.clone(),
            Value::Bool(value) => value.to_string(),
        }
    }
}

fn format_number(number: f64) -> String {
    if number.fract() == 0.0 && number.abs() < 1e15 {
        format!("{}", number as i64)
    } else {
        format!("{number}")
    }
}

/// 运行时输出流(规范 semantics.md §5)。
/// 机器视图(规范 agent-protocol.md §2.4):{"type":"text",…} / {"type":"ended"}。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Output {
    /// 一行文本;`new_line=false` 表示粘接(不换行)。
    Text {
        content: String,
        new_line: bool,
        tags: Vec<String>,
    },
    /// 故事结束(`-> END` 或执行到末尾)。
    Ended,
}

/// 暂停时的可选选择。
#[derive(Debug, Clone, Serialize)]
pub struct ChoiceView {
    pub label: String,
    /// 源码行(编辑器跳转用)。
    pub line: u32,
    /// 组内偏移。
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunError {
    pub message: String,
    pub node: Option<String>,
    pub line: Option<u32>,
}

impl RunError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        RunError {
            message: message.into(),
            node: None,
            line: None,
        }
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.node, self.line) {
            (Some(node), Some(line)) => {
                write!(formatter, "{}(节点 {node},第 {line} 行)", self.message)
            }
            (Some(node), None) => write!(formatter, "{}(节点 {node})", self.message),
            _ => write!(formatter, "{}", self.message),
        }
    }
}

impl std::error::Error for RunError {}

/// 内联帧来源(存档重建用)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) enum FrameSrc {
    /// if 语句(块内语句下标,分支下标)。
    IfBranch { stmt: usize, branch: usize },
    /// 选择体(组内选择语句下标)。
    ChoiceBody { stmt: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FrameSave {
    pub node: Option<String>,
    pub idx: usize,
    pub src: Option<FrameSrc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorKind {
    /// 手动 `anchor` 语句。
    Manual,
    /// 漂流 `->>`。
    Drift,
    /// 主线变动 `to`。
    Shift,
    /// 权限授予。
    Grant,
    /// 权限吊销。
    Revoke,
    /// 人物登场。
    Meet,
    /// 人物离场。
    Part,
}

impl AnchorKind {
    pub fn label(&self) -> &'static str {
        match self {
            AnchorKind::Manual => "锚点",
            AnchorKind::Drift => "漂流",
            AnchorKind::Shift => "主线变动",
            AnchorKind::Grant => "权限授予",
            AnchorKind::Revoke => "权限吊销",
            AnchorKind::Meet => "人物登场",
            AnchorKind::Part => "人物离场",
        }
    }
}

/// 一条锚点记录(只增不减,全量入存档)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorRecord {
    pub kind: AnchorKind,
    pub name: String,
    pub note: Option<String>,
    /// 自动记录的对象:目标节点 / 故事线 / 权限 / 角色。
    pub detail: Option<String>,
    /// 发生时所在节点。
    pub node: Option<String>,
    /// 发生后的当前故事线。
    pub storyline: String,
    pub turn: u32,
}

/// 一次实际发生的状态替换；同值替换也保留，按发生顺序入档。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateRecord {
    #[serde(default = "default_change_kind")]
    pub kind: worldline_core::ast::ChangeKind,
    pub state: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub event: Option<String>,
    pub node: Option<String>,
    pub note: Option<String>,
    pub turn: u32,
}

fn default_change_kind() -> worldline_core::ast::ChangeKind {
    worldline_core::ast::ChangeKind::Become
}

#[derive(Serialize, Deserialize)]
pub(super) struct SaveState {
    pub fingerprint: u64,
    pub vars: HashMap<String, Value>,
    pub visits: HashMap<String, u32>,
    pub turns: u32,
    pub taken_once: Vec<String>,
    pub frames: Vec<FrameSave>,
    pub glue_pending: bool,
    pub rng: u64,
    #[serde(default)]
    pub storyline: String,
    #[serde(default, skip_serializing)]
    pub perms: Option<Vec<String>>,
    #[serde(default)]
    pub met: Vec<String>,
    #[serde(default)]
    pub anchors: Vec<AnchorRecord>,
    #[serde(default)]
    pub states: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub state_history: Vec<StateRecord>,
}

#[cfg(test)]
mod tests {
    use super::{AnchorKind, Value};

    #[test]
    fn value_display_removes_redundant_decimal_part() {
        assert_eq!(Value::Num(42.0).display(), "42");
        assert_eq!(Value::Num(0.25).display(), "0.25");
        assert_eq!(Value::Bool(true).display(), "true");
    }

    #[test]
    fn anchor_kind_labels_are_stable_chinese_terms() {
        assert_eq!(AnchorKind::Manual.label(), "锚点");
        assert_eq!(AnchorKind::Drift.label(), "漂流");
        assert_eq!(AnchorKind::Grant.label(), "权限授予");
    }
}

//! worldline AST —— 语法规范见 `worldline/spec/syntax.md`。

/// 源文件内的位置:1-based 行号 + 文件内诊断所需的行首列偏移由各节点自带。
#[derive(Debug, Clone, Copy)]
pub struct Loc {
    pub line: u32,
    pub column: u32,
}

impl Loc {
    pub fn new(line: u32, column: u32) -> Self {
        Loc { line, column }
    }
}

/// 文本中的内插片段:字面量(已解码转义)或表达式。
#[derive(Debug, Clone)]
pub enum TextPart {
    Str(String),
    Expr(Expr),
    Link(crate::navigation::InlineLink),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Neq => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Var {
        name: String,
        loc: Loc,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// 内建调用:visits / turns / rnd。其余函数名在分析期报错。
    Call {
        name: String,
        args: Vec<Expr>,
        loc: Loc,
    },
}

impl Expr {
    /// 分析期可静态确定的类型。
    pub fn static_kind(&self) -> Option<ValueKind> {
        match self {
            Expr::Num(_) => Some(ValueKind::Num),
            Expr::Str(_) => Some(ValueKind::Str),
            Expr::Bool(_) => Some(ValueKind::Bool),
            Expr::Var { .. } | Expr::Call { .. } => None, // 由符号表补全
            Expr::Unary { op, expr } => match op {
                UnOp::Neg => expr.static_kind().filter(|k| *k == ValueKind::Num),
                UnOp::Not => expr.static_kind().filter(|k| *k == ValueKind::Bool),
            },
            Expr::Binary { op, lhs, rhs } => {
                let (l, r) = (lhs.static_kind()?, rhs.static_kind()?);
                if l != r {
                    return None;
                }
                match op {
                    BinOp::Add if l == ValueKind::Num || l == ValueKind::Str => Some(l),
                    BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod if l == ValueKind::Num => {
                        Some(ValueKind::Num)
                    }
                    BinOp::Eq | BinOp::Neq => Some(ValueKind::Bool),
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge if l == ValueKind::Num => {
                        Some(ValueKind::Bool)
                    }
                    BinOp::And | BinOp::Or if l == ValueKind::Bool => Some(ValueKind::Bool),
                    _ => None,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Num,
    Str,
    Bool,
}

impl ValueKind {
    pub fn label(&self) -> &'static str {
        match self {
            ValueKind::Num => "数值",
            ValueKind::Str => "字符串",
            ValueKind::Bool => "布尔",
        }
    }
}

/// 跃迁目标:命名节点或 END。命名目标在分析期解析并校验。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DivertTarget {
    Node(String),
    End,
}

#[derive(Debug, Clone)]
pub struct TextStmt {
    pub parts: Vec<TextPart>,
    /// 行尾 `~`:与下一行文本粘接。
    pub glue: bool,
    /// 行尾 `#标签`:元数据,不输出。
    pub tags: Vec<String>,
    pub loc: Loc,
}

// ---------------------------------------------------------------------------
// v1.5:故事线 / 角色 / 效果 / 锚点(2026-04 草案收编)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StorylineDecl {
    pub name: String,
    pub display: Option<String>,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct CharacterDecl {
    pub name: String,
    pub display: Option<String>,
    pub loc: Loc,
    pub file: String,
    pub properties: Vec<Property>,
    pub relations: Vec<CharacterRelation>,
}

/// 1.10 通用实体作者资料。实体不会成为事件、场景或运行时状态，
/// 仅由分析目录和结构编辑 API 消费。
#[derive(Debug, Clone)]
pub struct EntityDecl {
    pub name: String,
    pub entity_type: String,
    pub display: Option<String>,
    pub description: String,
    pub properties: Vec<Property>,
    pub file: String,
    pub loc: Loc,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    Str(String),
    Num(f64),
    Bool(bool),
}

#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub value: PropertyValue,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct CharacterRelation {
    pub target: String,
    pub label: String,
    pub loc: Loc,
}

/// 1.10 语义关系类型。关系类型是作者资料，不是执行动作。
#[derive(Debug, Clone)]
pub struct RelationTypeDecl {
    pub name: String,
    pub display: Option<String>,
    pub inverse_display: Option<String>,
    pub direction: crate::relations::RelationDirection,
    pub from_kind: Option<String>,
    pub to_kind: Option<String>,
    pub file: String,
    pub loc: Loc,
}

/// 1.10 语义关系实例。端点按完整 TargetRef 保存，不能按显示名合并。
#[derive(Debug, Clone)]
pub struct RelationDef {
    pub id: String,
    pub relation_type: String,
    pub from: crate::catalog::TargetRef,
    pub to: crate::catalog::TargetRef,
    pub description: String,
    pub source_note: Option<String>,
    pub scope_refs: Vec<crate::catalog::TargetRef>,
    pub properties: Vec<Property>,
    pub file: String,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct WorldDecl {
    pub name: String,
    pub display: Option<String>,
    pub description: String,
    pub properties: Vec<Property>,
    pub file: String,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct PeriodDecl {
    pub parent: Option<String>,
    pub name: String,
    pub display: Option<String>,
    pub file: String,
    pub loc: Loc,
}

/// 效果生效时机:进入节点 / 节点自然完成。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EffectWhen {
    Enter,
    Done,
    Exit,
}

impl EffectWhen {
    pub fn label(&self) -> &'static str {
        match self {
            EffectWhen::Enter => "进入时",
            EffectWhen::Done => "完成时",
            EffectWhen::Exit => "离开时",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChangeKind {
    Become,
    AddTags,
    RemoveTags,
    Grant,
    Revoke,
    Meet,
    Part,
    To,
}

impl ChangeKind {
    pub fn label(&self) -> &'static str {
        match self {
            ChangeKind::Become => "状态变更",
            ChangeKind::AddTags => "增加状态标签",
            ChangeKind::RemoveTags => "移除状态标签",
            ChangeKind::Grant => "权限授予",
            ChangeKind::Revoke => "权限吊销",
            ChangeKind::Meet => "人物登场",
            ChangeKind::Part => "人物离场",
            ChangeKind::To => "主线变动",
        }
    }
}

/// 效果动作 / 内联变动共用的语义单元。
#[derive(Debug, Clone)]
pub struct Change {
    pub tags: Vec<String>,
    pub kind: ChangeKind,
    /// 权限 id(Grant/Revoke)或角色 id(Meet/Part)。
    pub id: String,
    /// 叙事文本记录(`as "…"`),None 表示主故事线变动之外无记录需求。
    pub note: Option<String>,
    /// To 动作的目标故事线;其余动作此字段无效。
    pub to_storyline: Option<String>,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct EffectBlock {
    pub when: EffectWhen,
    pub cond: Option<Expr>,
    pub actions: Vec<Change>,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct AnchorStmt {
    /// 锚点名(字符串字面量,可中文)。
    pub name: String,
    pub note: Option<String>,
    pub loc: Loc,
}

/// 内联变动语句:grant / revoke / meet / part(不含 to)。
#[derive(Debug, Clone)]
pub struct ChangeStmt {
    pub change: Change,
}

#[derive(Debug, Clone)]
pub struct ChoiceStmt {
    /// 标签内插片段(求值前)。
    pub label: Vec<TextPart>,
    /// 标签原文,用于重复检测与关系图边标签。
    pub label_raw: String,
    pub once: bool,
    pub cond: Option<Expr>,
    pub body: Vec<Stmt>,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    /// (条件, 块);else 分支条件为 None 且必为最后一项。
    pub branches: Vec<(Option<Expr>, Vec<Stmt>)>,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct DivertStmt {
    pub target: DivertTarget,
    /// `->>` 漂流:切换故事线并记录锚点。
    pub drift: bool,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub name: String,
    pub expr: Expr,
    pub is_const: bool,
    pub loc: Loc,
    /// 声明所在源文件(诊断关联用)。
    pub file: String,
}

#[derive(Debug, Clone)]
pub struct SetStmt {
    pub name: String,
    pub expr: Expr,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub struct SceneStmt {
    pub name: String,
    pub body: Vec<Stmt>,
    pub loc: Loc,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Text(TextStmt),
    Choice(ChoiceStmt),
    If(IfStmt),
    Divert(DivertStmt),
    Let(LetStmt),
    Set(SetStmt),
    Scene(SceneStmt),
    /// 内联变动(grant/revoke/meet/part)。
    Change(ChangeStmt),
    /// 叙事锚点。
    Anchor(AnchorStmt),
    /// 效果块(仅事件体顶层合法,提取进 Event.effects;其余位置为残留)。
    Effect(EffectBlock),
}

#[derive(Debug, Clone)]
pub struct Event {
    pub name: String,
    pub body: Vec<Stmt>,
    pub loc: Loc,
    // v1.5 头部子句
    /// 事件简述(`as "…"`)。
    pub summary: Option<String>,
    /// 显式时间线序号,不影响执行流。
    pub order: Option<u32>,
    pub period: Option<String>,
    pub predecessors: Vec<String>,
    /// 人物字段(`with`,引用角色声明)。
    pub characters: Vec<String>,
    /// 准入权限(`perm`)。
    pub perm: Option<String>,
    /// 前置节点条件(`after`)。
    pub after: Option<Expr>,
    /// 所属故事线(解析期归属;无 storyline 块时为 "main")。
    pub storyline: String,
    /// 效果块(从事件体顶层提取)。
    pub effects: Vec<EffectBlock>,
}

/// 编译产物:程序结构(include 已在源文本级合并)。
#[derive(Debug, Clone, Default)]
pub struct Program {
    /// 旧权限与身份状态的兼容映射；不保存运行状态。
    pub permission_migration: Option<crate::migration::PermissionMigration>,
    pub catalog: Vec<crate::catalog::CatalogDecl>,
    /// 参与编译的文件路径,0 为主文件。
    pub files: Vec<String>,
    /// 全局 let/const 声明(顶层与块内,均为全局)。
    pub lets: Vec<LetStmt>,
    /// 故事线声明(同名多块按声明序重复出现,分析期合并)。
    pub storylines: Vec<StorylineDecl>,
    /// 角色声明。
    pub characters: Vec<CharacterDecl>,
    /// 1.10 实体作者资料；不参与事件执行和运行指纹。
    pub entities: Vec<EntityDecl>,
    /// 1.10 语义关系类型与实例；不进入事件执行和运行指纹。
    pub relation_types: Vec<RelationTypeDecl>,
    pub relations: Vec<RelationDef>,
    pub worlds: Vec<WorldDecl>,
    pub periods: Vec<PeriodDecl>,
    pub events: Vec<Event>,
    /// 与 events 平行:每个事件声明的源文件。
    pub event_files: Vec<String>,
    /// 入口事件 = 主文件第一个事件。
    pub entry: String,
}

impl Program {
    /// 求解事件索引。
    pub fn event_index(&self, name: &str) -> Option<usize> {
        self.events.iter().position(|e| e.name == name)
    }
}

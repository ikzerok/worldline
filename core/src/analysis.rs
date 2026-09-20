//! 语义分析:唯一的分析真相 —— 规范见 `spec/diagnostics.md` 与 `spec/relations.md`。
//! 职责:符号收集、引用解析、类型检查、流分析(可达性/终止性)、关系图。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::analysis_helpers::{
    expr_kind_static, expr_loc, first_divert, nodes_on_cycles, terminates,
};
use crate::ast::*;
use crate::diagnostic::{sort_diagnostics, Diagnostic, Span};
pub use crate::fingerprint::fingerprint_program;
use crate::graph::{AnchorDecl, EdgeKind, GraphEdge, GraphNode, RelationGraph};

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

// ---------------------------------------------------------------------------
// 分析器
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    program: &'a Program,
    symbols: Symbols,
    diags: Vec<Diagnostic>,
    node_ids: HashMap<String, u32>,
    graph_nodes: Vec<GraphNode>,
    graph_edges: Vec<GraphEdge>,
    anchors: Vec<AnchorDecl>,
    cur_file: String,
}

/// 节点执行上下文(引用解析与诊断归属)。
struct NodeCtx {
    event: usize,
    /// 当前节点全名(事件名或最深场景全名)。
    node_name: String,
}

pub fn analyze(program: &Program, parse_diags: Vec<Diagnostic>) -> (Analysis, Vec<Diagnostic>) {
    let mut diags = parse_diags;
    let mut ctx = Ctx {
        program,
        symbols: Symbols::default(),
        diags: Vec::new(),
        node_ids: HashMap::new(),
        graph_nodes: Vec::new(),
        graph_edges: Vec::new(),
        anchors: Vec::new(),
        cur_file: program.files.first().cloned().unwrap_or_default(),
    };

    ctx.collect_decl_symbols();
    ctx.collect_nodes();
    ctx.collect_vars();
    ctx.walk_all();
    ctx.flow_analysis();
    let world =
        crate::analysis_metadata::analyze_metadata(program, &mut ctx.symbols, &mut ctx.diags);

    let stats = Stats {
        events: ctx.graph_nodes.iter().filter(|n| n.is_event).count() as u32,
        scenes: ctx.graph_nodes.iter().filter(|n| !n.is_event).count() as u32,
        choices: ctx.graph_nodes.iter().map(|n| n.choice_count).sum(),
        words: ctx.graph_nodes.iter().map(|n| n.word_count).sum(),
        storylines: ctx.symbols.storyline_order.len() as u32,
        characters: ctx.symbols.character_order.len() as u32,
        entities: program.entities.len() as u32,
    };
    let entry = ctx.node_ids.get(&program.entry).copied().unwrap_or(0);
    let depth = ctx.compute_depth(entry);
    let storyline_order: Vec<(String, String)> = ctx
        .symbols
        .storyline_order
        .iter()
        .map(|id| {
            let display = ctx
                .symbols
                .storylines
                .get(id)
                .map(|s| s.display.clone())
                .unwrap_or_else(|| id.clone());
            (id.clone(), display)
        })
        .collect();

    let timeline = crate::timeline::analyze(program, &mut ctx.diags);
    let mut graph = RelationGraph {
        nodes: ctx.graph_nodes,
        ids: ctx.node_ids,
        edges: ctx.graph_edges,
        entry,
        depth,
        storyline_order,
    };
    crate::relation_context::populate(program, &ctx.symbols, &mut graph);
    let catalog = crate::catalog::analyze(program, &ctx.symbols, &graph, &mut ctx.diags);
    let mut all = std::mem::take(&mut ctx.diags);
    all.append(&mut diags);
    sort_diagnostics(&mut all);

    let analysis = Analysis {
        catalog,
        timeline,
        world,
        symbols: ctx.symbols,
        graph,
        stats,
        anchors: ctx.anchors,
        fingerprint: fingerprint_program(program),
    };
    (analysis, all)
}

impl<'a> Ctx<'a> {
    /// 故事线/角色声明收集(重复角色报 A104;故事线同名合并)。
    fn collect_decl_symbols(&mut self) {
        for sl in &self.program.storylines {
            if sl.name.is_empty() || self.symbols.storylines.contains_key(&sl.name) {
                continue;
            }
            self.symbols.storylines.insert(
                sl.name.clone(),
                StorylineInfo {
                    display: sl.display.clone().unwrap_or_else(|| sl.name.clone()),
                    declared: true,
                },
            );
            self.symbols.storyline_order.push(sl.name.clone());
        }
        for ch in &self.program.characters {
            if ch.name.is_empty() {
                continue;
            }
            if let Some(prev) = self.symbols.characters.get(&ch.name) {
                let (pf, ps) = (prev.decl_file.clone(), prev.decl_span);
                self.diags.push(
                    Diagnostic::error(
                        "A104",
                        &ch.file,
                        Span::new(ch.loc.line, ch.loc.column, ch.name.chars().count() as u32),
                        format!("角色 `{}` 重复定义", ch.name),
                    )
                    .with_related(&pf, ps),
                );
                continue;
            }
            self.symbols.characters.insert(
                ch.name.clone(),
                CharacterInfo {
                    display: ch.display.clone().unwrap_or_else(|| ch.name.clone()),
                    decl_file: ch.file.clone(),
                    decl_span: Span::new(
                        ch.loc.line,
                        ch.loc.column,
                        ch.name.chars().count() as u32,
                    ),
                    properties: BTreeMap::new(),
                    relations: Vec::new(),
                    events: Vec::new(),
                },
            );
            self.symbols.character_order.push(ch.name.clone());
        }
    }

    /// 故事线归属注册:显式声明优先,事件隐式归属补录。
    fn register_storyline(&mut self, id: &str) {
        if id.is_empty() || self.symbols.storylines.contains_key(id) {
            return;
        }
        self.symbols.storylines.insert(
            id.to_string(),
            StorylineInfo {
                display: id.to_string(),
                declared: false,
            },
        );
        self.symbols.storyline_order.push(id.to_string());
    }

    fn add_node(&mut self, node: GraphNode) {
        if self.node_ids.contains_key(&node.name) {
            return; // 重复符号另行报错
        }
        let id = self.graph_nodes.len() as u32;
        self.node_ids.insert(node.name.clone(), id);
        self.graph_nodes.push(node);
    }

    fn add_edge(
        &mut self,
        from: &str,
        to: &str,
        kind: EdgeKind,
        label: Option<String>,
        file: &str,
        line: u32,
    ) {
        let (Some(&f), Some(&t)) = (self.node_ids.get(from), self.node_ids.get(to)) else {
            return; // 目标未知的边由引用检查负责报告
        };
        self.graph_edges.push(GraphEdge {
            contexts: Vec::new(),
            target_requirement: None,
            from: f,
            to: t,
            kind,
            label,
            file: file.to_string(),
            line,
        });
    }

    /// 第一遍:收集事件与场景符号(重复定义报 A104),分配故事线序号。
    fn collect_nodes(&mut self) {
        let mut seq_of: HashMap<String, u32> = HashMap::new();
        for idx in 0..self.program.events.len() {
            let name = self.program.events[idx].name.clone();
            let loc = self.program.events[idx].loc;
            let file = self
                .program
                .event_files
                .get(idx)
                .cloned()
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            if let Some(prev) = self.symbols.events.get(&name) {
                let prev = prev.clone();
                let prev_file = self
                    .program
                    .event_files
                    .get(prev.event)
                    .cloned()
                    .unwrap_or_default();
                let prev_loc = self.program.events[prev.event].loc;
                self.diags.push(
                    Diagnostic::error(
                        "A104",
                        &file,
                        Span::new(loc.line, loc.column, name.chars().count() as u32),
                        format!("事件 `{name}` 重复定义"),
                    )
                    .with_related(
                        &prev_file,
                        Span::new(prev_loc.line, prev_loc.column, name.chars().count() as u32),
                    ),
                );
                continue;
            }
            let storyline = self.program.events[idx].storyline.clone();
            self.register_storyline(&storyline);
            let seq = {
                let n = seq_of.entry(storyline.clone()).or_insert(0);
                *n += 1;
                *n
            };
            // with 角色引用校验(A208)
            let characters = self.program.events[idx].characters.clone();
            for c in &characters {
                if !c.is_empty() && !self.symbols.characters.contains_key(c) {
                    self.diags.push(Diagnostic::error(
                        "A208",
                        &file,
                        Span::new(loc.line, loc.column, c.chars().count() as u32),
                        format!("事件 `{name}` 的 with 引用了未定义角色 `{c}`"),
                    ));
                }
            }
            self.symbols.events.insert(
                name.clone(),
                NodePath {
                    event: idx,
                    scenes: vec![],
                },
            );
            self.symbols.event_order.push(name.clone());
            let body = self.program.events[idx].body.clone();
            let (ch, w) = count_choices_words(&body);
            let summary = self.program.events[idx].summary.clone();
            let perm = self.program.events[idx].perm.clone();
            self.add_node(GraphNode {
                name: name.clone(),
                is_event: true,
                file: file.clone(),
                line: loc.line,
                choice_count: ch,
                word_count: w,
                storyline: storyline.clone(),
                seq: self.program.events[idx].order.unwrap_or(seq),
                summary,
                characters,
                perm,
            });
            self.collect_scenes(idx, &name, &body, &file, &storyline);
        }
    }

    fn collect_scenes(
        &mut self,
        event_idx: usize,
        parent_full: &str,
        body: &[Stmt],
        file: &str,
        storyline: &str,
    ) {
        for stmt in body {
            let Stmt::Scene(s) = stmt else { continue };
            let full = format!("{parent_full}.{}", s.name);
            let (ch, w) = count_choices_words(&s.body);
            if self.node_ids.contains_key(&full) {
                self.diags.push(Diagnostic::error(
                    "A104",
                    file,
                    Span::new(s.loc.line, s.loc.column, s.name.chars().count() as u32),
                    format!("场景 `{full}` 重复定义"),
                ));
            } else {
                let scenes: Vec<String> = full.split('.').skip(1).map(str::to_string).collect();
                self.add_node(GraphNode {
                    name: full.clone(),
                    is_event: false,
                    file: file.to_string(),
                    line: s.loc.line,
                    choice_count: ch,
                    word_count: w,
                    storyline: storyline.to_string(),
                    seq: 0,
                    summary: None,
                    characters: Vec::new(),
                    perm: None,
                });
                self.symbols.scenes.insert(
                    full.clone(),
                    NodePath {
                        event: event_idx,
                        scenes,
                    },
                );
            }
            self.collect_scenes(event_idx, &full, &s.body, file, storyline);
        }
    }

    /// 第二遍:收集全局变量(类型由初始化表达式静态推断)。
    fn collect_vars(&mut self) {
        for l in &self.program.lets {
            if l.name.is_empty() {
                continue;
            }
            if let Some(prev) = self.symbols.vars.get(&l.name) {
                let (pf, ps) = (prev.decl_file.clone(), prev.decl_span);
                self.diags.push(
                    Diagnostic::error(
                        "A104",
                        &l.file,
                        Span::new(l.loc.line, l.loc.column, l.name.chars().count() as u32),
                        format!("变量 `{}` 重复定义", l.name),
                    )
                    .with_related(&pf, ps),
                );
                continue;
            }
            self.symbols.vars.insert(
                l.name.clone(),
                VarInfo {
                    kind: expr_kind_static(&l.expr, &self.symbols),
                    is_const: l.is_const,
                    decl_file: l.file.clone(),
                    decl_span: Span::new(l.loc.line, l.loc.column, l.name.chars().count() as u32),
                    read: false,
                },
            );
        }
    }

    /// 第三遍:遍历所有事件体,做引用/类型检查并产出图边。
    fn walk_all(&mut self) {
        for idx in 0..self.program.events.len() {
            let name = self.program.events[idx].name.clone();
            if self.symbols.events.get(&name).map(|p| p.event) != Some(idx) {
                continue; // 重复定义的后者跳过
            }
            self.cur_file = self
                .program
                .event_files
                .get(idx)
                .cloned()
                .unwrap_or_default();
            let node = NodeCtx {
                event: idx,
                node_name: name.clone(),
            };
            // after 前置条件类型检查
            if let Some(after) = &self.program.events[idx].after {
                self.check_expr(after, Some(ValueKind::Bool));
            }
            // 效果块:条件与动作校验
            let effects = self.program.events[idx].effects.clone();
            for fx in &effects {
                if let Some(cond) = &fx.cond {
                    self.check_expr(cond, Some(ValueKind::Bool));
                }
                for a in &fx.actions {
                    self.check_change(a);
                }
            }
            let body = self.program.events[idx].body.clone();
            self.walk_block(&body, &node, 0);
        }
    }

    /// 变动动作校验:角色引用(A208)与故事线引用(A210)。
    fn check_change(&mut self, a: &Change) {
        match a.kind {
            ChangeKind::Meet | ChangeKind::Part => {
                if !a.id.is_empty() && !self.symbols.characters.contains_key(&a.id) {
                    self.diags.push(Diagnostic::error(
                        "A208",
                        &self.cur_file,
                        Span::new(a.loc.line, a.loc.column, a.id.chars().count() as u32),
                        format!("{}引用了未定义角色 `{}`", a.kind.label(), a.id),
                    ));
                }
            }
            ChangeKind::To => {
                let Some(sl) = &a.to_storyline else { return };
                if !sl.is_empty() && !self.symbols.storylines.contains_key(sl) {
                    self.diags.push(Diagnostic::error(
                        "A210",
                        &self.cur_file,
                        Span::new(a.loc.line, a.loc.column, sl.chars().count() as u32),
                        format!("主线变动的目标故事线 `{sl}` 不存在"),
                    ));
                }
            }
            ChangeKind::Grant
            | ChangeKind::Revoke
            | ChangeKind::Become
            | ChangeKind::AddTags
            | ChangeKind::RemoveTags => {}
        }
    }

    fn walk_block(&mut self, stmts: &[Stmt], node: &NodeCtx, depth: u32) {
        let mut i = 0;
        while i < stmts.len() {
            match &stmts[i] {
                Stmt::Text(t) => {
                    for p in &t.parts {
                        if let TextPart::Expr(e) = p {
                            self.check_expr(e, None);
                        }
                    }
                }
                Stmt::Divert(d) => {
                    self.check_divert(d, node, depth);
                }
                Stmt::Choice(first) => {
                    // 选择组:连续 Choice 语句构成一组
                    let mut j = i;
                    let mut labels: HashSet<String> = HashSet::new();
                    let mut all_cond = true;
                    while let Some(Stmt::Choice(c)) = stmts.get(j) {
                        let label = c.label_raw.trim().to_string();
                        if !labels.insert(label.clone()) {
                            self.diags.push(Diagnostic::hint(
                                "A207",
                                &self.cur_file,
                                Span::new(c.loc.line, c.loc.column, 6),
                                format!("同一选择组内标签重复:`{label}`"),
                            ));
                        }
                        if let Some(cond) = &c.cond {
                            self.check_expr(cond, Some(ValueKind::Bool));
                        } else {
                            all_cond = false;
                        }
                        for p in &c.label {
                            if let TextPart::Expr(e) = p {
                                self.check_expr(e, None);
                            }
                        }
                        // Choice 边:选择体内预序首个跃迁
                        if let Some(DivertTarget::Node(t)) = first_divert(&c.body) {
                            let from = node.node_name.clone();
                            let file = self.cur_file.clone();
                            let line = c.loc.line;
                            let label = shorten_label(&c.label_raw);
                            self.add_edge(&from, t, EdgeKind::Choice, Some(label), &file, line);
                        }
                        self.walk_block(&c.body, node, depth + 1);
                        j += 1;
                    }
                    if all_cond && j > i {
                        self.diags.push(Diagnostic::warning(
                            "A203",
                            &self.cur_file,
                            Span::new(first.loc.line, first.loc.column, 6),
                            "选择组内所有分支都带条件:全部不满足时将直接穿过本组(fallback 落穿)",
                        ));
                    }
                    i = j;
                    continue;
                }
                Stmt::If(s) => {
                    for (cond, body) in &s.branches {
                        if let Some(c) = cond {
                            self.check_expr(c, Some(ValueKind::Bool));
                        }
                        self.walk_block(body, node, depth + 1);
                    }
                }
                Stmt::Let(l) => {
                    self.check_expr(&l.expr, None);
                }
                Stmt::Set(s) => {
                    let expected = match self.symbols.vars.get(&s.name) {
                        Some(v) => {
                            if v.is_const {
                                self.diags.push(Diagnostic::error(
                                    "A106",
                                    &self.cur_file,
                                    Span::new(
                                        s.loc.line,
                                        s.loc.column,
                                        s.name.chars().count() as u32,
                                    ),
                                    format!("不能对常量 `{}` 赋值", s.name),
                                ));
                            }
                            v.kind
                        }
                        None => {
                            self.diags.push(Diagnostic::error(
                                "A102",
                                &self.cur_file,
                                Span::new(s.loc.line, s.loc.column, s.name.chars().count() as u32),
                                format!("`set` 的目标 `{}` 未声明(需要先 let)", s.name),
                            ));
                            None
                        }
                    };
                    self.check_expr(&s.expr, expected);
                }
                Stmt::Scene(s) => {
                    let inner = NodeCtx {
                        event: node.event,
                        node_name: format!("{}.{}", node.node_name, s.name),
                    };
                    let file = self.cur_file.clone();
                    let line = s.loc.line;
                    let from = node.node_name.clone();
                    let to = inner.node_name.clone();
                    self.add_edge(&from, &to, EdgeKind::Enter, None, &file, line);
                    self.walk_block(&s.body, &inner, depth);
                }
                Stmt::Change(c) => {
                    self.check_change(&c.change);
                }
                Stmt::Anchor(a) => {
                    self.anchors.push(AnchorDecl {
                        node: node.node_name.clone(),
                        name: a.name.clone(),
                        note: a.note.clone(),
                        file: self.cur_file.clone(),
                        line: a.loc.line,
                    });
                }
                Stmt::Effect(_) => {} // 已在事件顶层提取;残留由解析器报错
            }
            i += 1;
        }
    }

    fn check_divert(&mut self, d: &DivertStmt, node: &NodeCtx, depth: u32) {
        let DivertTarget::Node(target) = &d.target else {
            return;
        };
        let Some(path) = self.symbols.resolve_target(
            target,
            self.symbols.event_order.get(node.event).map(String::as_str),
        ) else {
            self.diags.push(Diagnostic::error(
                "A101",
                &self.cur_file,
                Span::new(d.loc.line, d.loc.column, target.chars().count() as u32),
                format!("跃迁目标 `{target}` 不存在"),
            ));
            return;
        };
        let full = path.full_name(&self.program.events[path.event].name);
        if full == node.node_name && depth == 0 {
            self.diags.push(Diagnostic::warning(
                "A206",
                &self.cur_file,
                Span::new(d.loc.line, d.loc.column, target.chars().count() as u32),
                format!("无条件跃迁回 `{full}` 自身,会构成死循环"),
            ));
        }
        // 漂流语义:A209 同线漂流提示
        if d.drift {
            let cur_sl = self.program.events[node.event].storyline.clone();
            let tgt_sl = self.program.events[path.event].storyline.clone();
            if cur_sl == tgt_sl {
                self.diags.push(Diagnostic::warning(
                    "A209",
                    &self.cur_file,
                    Span::new(d.loc.line, d.loc.column, target.chars().count() as u32 + 3),
                    format!(
                        "漂流 `->>` 的目标 `{full}` 与当前节点在同一故事线 `{cur_sl}`;跨线移动才需要漂流,此处应使用 `->`"
                    ),
                ));
            }
        }
        let edge_kind = if d.drift {
            EdgeKind::Drift
        } else {
            EdgeKind::Divert
        };
        let label = if d.drift {
            Some("漂流".to_string())
        } else {
            None
        };
        let file = self.cur_file.clone();
        let line = d.loc.line;
        let from = node.node_name.clone();
        self.add_edge(&from, &full, edge_kind, label, &file, line);
    }

    /// 表达式检查:引用存在性 + 类型规则;expected 给出上下文期望类型。
    fn check_expr(&mut self, e: &Expr, expected: Option<ValueKind>) -> Option<ValueKind> {
        let kind = self.infer_expr(e);
        if let (Some(k), Some(exp)) = (kind, expected) {
            if k != exp {
                let loc = expr_loc(e);
                self.diags.push(Diagnostic::error(
                    "A103",
                    &self.cur_file,
                    Span::new(loc.line, loc.column, 4),
                    format!("类型不匹配:此处需要{},实际为{}", exp.label(), k.label()),
                ));
            }
        }
        kind
    }

    fn infer_expr(&mut self, e: &Expr) -> Option<ValueKind> {
        match e {
            Expr::Num(_) => Some(ValueKind::Num),
            Expr::Str(_) => Some(ValueKind::Str),
            Expr::Bool(_) => Some(ValueKind::Bool),
            Expr::Var { name, loc } => match self.symbols.vars.get_mut(name) {
                Some(v) => {
                    v.read = true;
                    v.kind
                }
                None => {
                    self.diags.push(Diagnostic::error(
                        "A102",
                        &self.cur_file,
                        Span::new(loc.line, loc.column, name.chars().count().max(1) as u32),
                        format!("变量 `{name}` 未声明(需要先 let)"),
                    ));
                    None
                }
            },
            Expr::Unary { op, expr } => {
                let k = self.infer_expr(expr);
                let need = match op {
                    UnOp::Neg => ValueKind::Num,
                    UnOp::Not => ValueKind::Bool,
                };
                match k {
                    Some(k) if k == need => Some(need),
                    Some(other) => {
                        let loc = expr_loc(e);
                        self.diags.push(Diagnostic::error(
                            "A103",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 3),
                            format!("一元运算需要{},得到{}", need.label(), other.label()),
                        ));
                        None
                    }
                    None => None,
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                let l = self.infer_expr(lhs);
                let r = self.infer_expr(rhs);
                let (l, r) = (l?, r?);
                let ok = match op {
                    BinOp::Add => l == r && (l == ValueKind::Num || l == ValueKind::Str),
                    BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        l == ValueKind::Num && r == ValueKind::Num
                    }
                    BinOp::Eq | BinOp::Neq => l == r,
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        l == ValueKind::Num && r == ValueKind::Num
                    }
                    BinOp::And | BinOp::Or => l == ValueKind::Bool && r == ValueKind::Bool,
                };
                if !ok {
                    let loc = expr_loc(e);
                    self.diags.push(Diagnostic::error(
                        "A103",
                        &self.cur_file,
                        Span::new(loc.line, loc.column, 2),
                        format!(
                            "运算 `{}` 不接受 {} 与 {}",
                            op.symbol(),
                            l.label(),
                            r.label()
                        ),
                    ));
                    return None;
                }
                Some(match op {
                    BinOp::Eq
                    | BinOp::Neq
                    | BinOp::Lt
                    | BinOp::Le
                    | BinOp::Gt
                    | BinOp::Ge
                    | BinOp::And
                    | BinOp::Or => ValueKind::Bool,
                    _ => l,
                })
            }
            Expr::Call { name, args, loc } => match name.as_str() {
                "has" => {
                    crate::states::check_has(
                        args,
                        *loc,
                        &self.cur_file,
                        self.program,
                        &mut self.diags,
                    );
                    Some(ValueKind::Bool)
                }
                "visits" => {
                    if args.len() != 1 {
                        self.diags.push(Diagnostic::error(
                            "A103",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 6),
                            "visits 需要恰好一个参数,如 visits(market)",
                        ));
                        return Some(ValueKind::Num);
                    }
                    let Some(Expr::Str(target)) = args.first() else {
                        self.diags.push(Diagnostic::error(
                            "A103",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 6),
                            "visits 的参数应为节点名",
                        ));
                        return Some(ValueKind::Num);
                    };
                    if self.symbols.resolve_node(target).is_none() {
                        self.diags.push(Diagnostic::error(
                            "A101",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 6 + target.chars().count() as u32),
                            format!("visits 的目标 `{target}` 不存在"),
                        ));
                    }
                    Some(ValueKind::Num)
                }
                "seen" | "perm" => {
                    let is_seen = name == "seen";
                    let Some(Expr::Str(target)) = args.first() else {
                        self.diags.push(Diagnostic::error(
                            "A103",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 4),
                            format!("{name} 的参数应为名称"),
                        ));
                        return Some(ValueKind::Bool);
                    };
                    if is_seen && self.symbols.resolve_node(target).is_none() {
                        self.diags.push(Diagnostic::error(
                            "A101",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 4 + target.chars().count() as u32),
                            format!("seen 的目标 `{target}` 不存在"),
                        ));
                    }
                    Some(ValueKind::Bool)
                }
                "turns" => {
                    if !args.is_empty() {
                        self.diags.push(Diagnostic::error(
                            "A103",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 5),
                            "turns 不接受参数",
                        ));
                    }
                    Some(ValueKind::Num)
                }
                "rnd" => {
                    if args.len() != 2 {
                        self.diags.push(Diagnostic::error(
                            "A103",
                            &self.cur_file,
                            Span::new(loc.line, loc.column, 3),
                            "rnd 需要两个数值参数,如 rnd(1, 6)",
                        ));
                        return Some(ValueKind::Num);
                    }
                    for a in args {
                        self.check_expr(a, Some(ValueKind::Num));
                    }
                    Some(ValueKind::Num)
                }
                other => {
                    self.diags.push(Diagnostic::error(
                        "A103",
                        &self.cur_file,
                        Span::new(loc.line, loc.column, other.chars().count() as u32),
                        format!("未知函数 `{other}`(可用:visits / turns / rnd)"),
                    ));
                    None
                }
            },
        }
    }

    /// 流分析:A201 不可达、A202 无终止、A205 once 无意义、A107 未读变量。
    fn flow_analysis(&mut self) {
        let entry = self.node_ids.get(&self.program.entry).copied().unwrap_or(0);
        let adj = self.graph_edges.iter().fold(
            vec![Vec::new(); self.graph_nodes.len()],
            |mut acc: Vec<Vec<u32>>, e| {
                acc[e.from as usize].push(e.to);
                acc
            },
        );
        // 可达性
        let mut reach = vec![false; self.graph_nodes.len()];
        if (entry as usize) < reach.len() {
            let mut q = VecDeque::new();
            q.push_back(entry);
            reach[entry as usize] = true;
            while let Some(n) = q.pop_front() {
                for &m in &adj[n as usize] {
                    if !reach[m as usize] {
                        reach[m as usize] = true;
                        q.push_back(m);
                    }
                }
            }
        }
        for (i, n) in self.graph_nodes.iter().enumerate() {
            let temporal = self
                .symbols
                .events
                .get(&n.name)
                .or_else(|| self.symbols.scenes.get(&n.name))
                .is_some_and(|path| self.program.events[path.event].period.is_some());
            if !reach[i] && i != entry as usize && !temporal {
                self.diags.push(Diagnostic::warning(
                    "A201",
                    &n.file,
                    Span::new(n.line, 1, n.name.chars().count() as u32),
                    format!("`{}` 不可达:没有任何跃迁或结构进入指向它", n.name),
                ));
            }
        }
        // 可重访性:节点处于某个环上(含自环)
        let cyclic = nodes_on_cycles(&adj);
        // A202 / A205
        for idx in 0..self.program.events.len() {
            let name = self.program.events[idx].name.clone();
            if self.symbols.events.get(&name).map(|p| p.event) != Some(idx) {
                continue;
            }
            self.cur_file = self
                .program
                .event_files
                .get(idx)
                .cloned()
                .unwrap_or_default();
            let loc = self.program.events[idx].loc;
            if self.program.events[idx].period.is_none()
                && !terminates(&self.program.events[idx].body)
            {
                self.diags.push(Diagnostic::warning(
                    "A202",
                    &self.cur_file,
                    Span::new(loc.line, loc.column, name.chars().count() as u32),
                    format!("事件 `{name}` 可能执行到结尾而没有跃迁(视同 END);建议显式 `-> END` 或补跃迁"),
                ));
            }
            let body = self.program.events[idx].body.clone();
            self.check_once_acyclic(&body, &name, &cyclic);
        }
        // A107
        let unused: Vec<(String, Span, String)> = self
            .symbols
            .vars
            .iter()
            .filter(|(_, v)| !v.read)
            .map(|(n, v)| (n.clone(), v.decl_span, v.decl_file.clone()))
            .collect();
        for (name, span, file) in unused {
            self.diags.push(Diagnostic::warning(
                "A107",
                &file,
                span,
                format!("变量 `{name}` 声明后从未被读取"),
            ));
        }
    }

    /// A205:once 选择所在节点不可重访时,once 与粘性等价。
    fn check_once_acyclic(&mut self, stmts: &[Stmt], node_name: &str, cyclic: &HashSet<u32>) {
        fn rec(s: &[Stmt], node: &str, ctx: &mut Ctx, cyclic: &HashSet<u32>) {
            let on_cycle = cyclic.contains(ctx.node_ids.get(node).unwrap_or(&u32::MAX));
            for st in s {
                match st {
                    Stmt::Choice(c) => {
                        if c.once && !on_cycle {
                            ctx.diags.push(Diagnostic::warning(
                                "A205",
                                &ctx.cur_file,
                                Span::new(c.loc.line, c.loc.column, 10),
                                "`once` 所在节点不会被重访,与默认粘性行为相同,可省略",
                            ));
                        }
                        rec(&c.body, node, ctx, cyclic);
                    }
                    Stmt::If(i) => {
                        for (_, b) in &i.branches {
                            rec(b, node, ctx, cyclic);
                        }
                    }
                    Stmt::Scene(sc) => {
                        let inner = format!("{node}.{}", sc.name);
                        rec(&sc.body, &inner, ctx, cyclic);
                    }
                    _ => {}
                }
            }
        }
        rec(stmts, node_name, self, cyclic);
    }

    fn compute_depth(&self, entry: u32) -> Vec<u32> {
        let mut depth = vec![u32::MAX; self.graph_nodes.len()];
        if (entry as usize) < depth.len() {
            let adj = self.graph_edges.iter().fold(
                vec![Vec::new(); self.graph_nodes.len()],
                |mut acc: Vec<Vec<u32>>, e| {
                    acc[e.from as usize].push(e.to);
                    acc
                },
            );
            let mut q = VecDeque::new();
            depth[entry as usize] = 0;
            q.push_back(entry);
            while let Some(n) = q.pop_front() {
                for &m in &adj[n as usize] {
                    if depth[m as usize] == u32::MAX {
                        depth[m as usize] = depth[n as usize] + 1;
                        q.push_back(m);
                    }
                }
            }
        }
        depth
    }
}

fn count_choices_words(stmts: &[Stmt]) -> (u32, u32) {
    let mut choices = 0;
    let mut words = 0;
    fn rec(stmts: &[Stmt], choices: &mut u32, words: &mut u32) {
        for s in stmts {
            match s {
                Stmt::Text(t) => {
                    for p in &t.parts {
                        if let Some(l) = match p {
                            TextPart::Str(l) => Some(l),
                            TextPart::Link(link) => Some(&link.label),
                            _ => None,
                        } {
                            *words += l.chars().filter(|c| !c.is_whitespace()).count() as u32;
                        }
                    }
                }
                Stmt::Choice(c) => {
                    *choices += 1;
                    rec(&c.body, choices, words);
                }
                Stmt::If(i) => {
                    for (_, b) in &i.branches {
                        rec(b, choices, words);
                    }
                }
                Stmt::Scene(s) => rec(&s.body, choices, words),
                _ => {}
            }
        }
    }
    rec(stmts, &mut choices, &mut words);
    (choices, words)
}

fn shorten_label(raw: &str) -> String {
    let s = raw.trim();
    if s.chars().count() > 12 {
        let prefix: String = s.chars().take(11).collect();
        format!("{prefix}…")
    } else {
        s.to_string()
    }
}

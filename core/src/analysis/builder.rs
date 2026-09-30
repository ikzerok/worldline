use super::{
    fingerprint_program, Analysis, CharacterInfo, NodePath, Stats, StorylineInfo, Symbols, VarInfo,
};
use crate::ast::*;
use crate::diagnostic::{sort_diagnostics, Diagnostic, Span};
use crate::graph::{AnchorDecl, EdgeKind, GraphEdge, GraphNode, RelationGraph};
use std::collections::{BTreeMap, HashMap};

mod expressions;
mod flow;
mod fragment_flow;
mod language;
mod variables;
mod walk;
struct Ctx<'a> {
    program: &'a Program,
    symbols: Symbols,
    diags: Vec<Diagnostic>,
    node_ids: HashMap<String, u32>,
    graph_nodes: Vec<GraphNode>,
    graph_edges: Vec<GraphEdge>,
    anchors: Vec<AnchorDecl>,
    cur_file: String,
    locals: HashMap<String, ValueKind>,
    in_fragment: bool,
    in_rule: bool,
    expression_fallback: Loc,
}

/// 节点执行上下文(引用解析与诊断归属)。
struct NodeCtx {
    event: usize,
    /// 当前节点全名(事件名或最深场景全名)。
    node_name: String,
}
pub(super) fn analyze(
    program: &Program,
    parse_diags: Vec<Diagnostic>,
) -> (Analysis, Vec<Diagnostic>) {
    let mut diags = parse_diags;
    let mut ctx = Ctx {
        program,
        symbols: Symbols::default(),
        diags: Vec::new(),
        node_ids: HashMap::new(),
        graph_nodes: Vec::new(),
        graph_edges: Vec::new(),
        anchors: Vec::new(),
        locals: HashMap::new(),
        in_fragment: false,
        in_rule: false,
        expression_fallback: Loc::new(0, 1),
        cur_file: program.files.first().cloned().unwrap_or_default(),
    };

    ctx.collect_decl_symbols();
    ctx.collect_nodes();
    ctx.collect_callable_symbols();
    ctx.collect_vars();
    ctx.walk_all();
    ctx.walk_callables();
    ctx.collect_fragment_transitions();
    ctx.flow_analysis();
    let world =
        crate::analysis_metadata::analyze_metadata(program, &mut ctx.symbols, &mut ctx.diags);

    let fragment_stats: Vec<_> = program
        .fragments
        .iter()
        .map(|f| count_choices_words(&f.body))
        .collect();
    let stats = Stats {
        events: ctx.graph_nodes.iter().filter(|n| n.is_event).count() as u32,
        scenes: ctx.graph_nodes.iter().filter(|n| !n.is_event).count() as u32,
        choices: ctx.graph_nodes.iter().map(|n| n.choice_count).sum::<u32>()
            + fragment_stats.iter().map(|s| s.0).sum::<u32>(),
        words: ctx.graph_nodes.iter().map(|n| n.word_count).sum::<u32>()
            + fragment_stats.iter().map(|s| s.1).sum::<u32>(),
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
}
fn count_choices_words(stmts: &[Stmt]) -> (u32, u32) {
    let mut choices = 0;
    let mut words = 0;
    fn rec(stmts: &[Stmt], choices: &mut u32, words: &mut u32) {
        for s in stmts {
            match s {
                Stmt::Text(t) | Stmt::Say(crate::language::SayStmt { text: t, .. }) => {
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

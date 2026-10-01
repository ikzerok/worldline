//! 顺序执行摘要；不使用包含不可执行后缀的展示图证明闭环。
use super::flow_safety::{expression_safe, globals, text_safe};
use crate::analysis::{NodePath, Symbols};
use crate::ast::*;
use crate::diagnostic::Span;
use crate::graph::GraphNode;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Clone)]
pub(super) struct Site {
    pub file: String,
    pub span: Span,
}

#[derive(Clone, Default)]
struct Summary {
    fallthrough: bool,
    returns: bool,
    ended: bool,
    pause: bool,
    unknown: bool,
    authored_flow: bool,
    call_depth: usize,
    jumps: BTreeMap<String, Site>,
}

impl Summary {
    fn continuing() -> Self {
        Self {
            fallthrough: true,
            ..Self::default()
        }
    }

    fn unknown() -> Self {
        Self {
            unknown: true,
            ..Self::continuing()
        }
    }

    fn merge(&mut self, other: Self) {
        self.fallthrough |= other.fallthrough;
        self.returns |= other.returns;
        self.ended |= other.ended;
        self.pause |= other.pause;
        self.unknown |= other.unknown;
        self.authored_flow |= other.authored_flow;
        self.call_depth = self.call_depth.max(other.call_depth);
        for (target, site) in other.jumps {
            self.jumps.entry(target).or_insert(site);
        }
    }

    fn then(&mut self, next: Self) {
        if self.fallthrough {
            self.fallthrough = false;
            self.merge(next);
        }
    }

    fn can_close_cycle(&self) -> bool {
        !self.fallthrough && !self.returns && !self.ended && !self.pause && !self.unknown
    }
}

struct Summarizer<'a> {
    program: &'a Program,
    fragments: HashMap<String, Summary>,
    active: HashSet<String>,
}

/// 仅折叠字面布尔与 not；不求值变量、规则、随机或运行状态。
fn truth(expr: Option<&Expr>) -> Option<bool> {
    match expr {
        None => Some(true),
        Some(Expr::Bool(value)) => Some(*value),
        Some(Expr::Unary {
            op: UnOp::Not,
            expr,
        }) => truth(Some(expr)).map(|value| !value),
        _ => None,
    }
}

impl Summarizer<'_> {
    fn fragment(&mut self, name: &str) -> Summary {
        if let Some(summary) = self.fragments.get(name) {
            return summary.clone();
        }
        if self.active.len() >= 128 || !self.active.insert(name.to_string()) {
            return Summary::unknown();
        }
        let program = self.program;
        let summary = program
            .fragments
            .iter()
            .find(|fragment| fragment.name == name)
            .map(|fragment| {
                let mut initialized = globals(program);
                for local in crate::language::locals(&fragment.body) {
                    initialized.remove(&local.name);
                }
                initialized.extend(
                    fragment
                        .parameters
                        .iter()
                        .map(|parameter| parameter.name.clone()),
                );
                self.block(&fragment.body, &fragment.file, &initialized)
            })
            .unwrap_or_else(Summary::unknown);
        self.active.remove(name);
        self.fragments.insert(name.to_string(), summary.clone());
        summary
    }

    fn block(&mut self, body: &[Stmt], file: &str, known: &HashSet<String>) -> Summary {
        let mut initialized = known.clone();
        let mut out = Summary::continuing();
        let mut index = 0;
        while out.fallthrough && index < body.len() {
            let next = match &body[index] {
                Stmt::Divert(divert) => {
                    let mut next = Summary {
                        authored_flow: true,
                        ..Summary::default()
                    };
                    match &divert.target {
                        DivertTarget::End => next.ended = true,
                        DivertTarget::Node(target) => {
                            next.jumps.insert(
                                target.clone(),
                                Site {
                                    file: file.to_string(),
                                    span: Span::new(
                                        divert.loc.line,
                                        divert.loc.column,
                                        target.chars().count() as u32,
                                    ),
                                },
                            );
                        }
                    }
                    next
                }
                Stmt::Return(_) => Summary {
                    returns: true,
                    ..Summary::default()
                },
                Stmt::Call(call) => {
                    let mut next = self.fragment(&call.name);
                    next.authored_flow = true;
                    next.fallthrough |= next.returns;
                    next.returns = false;
                    next.call_depth += 1;
                    next.unknown |= next.call_depth > 128
                        || call
                            .args
                            .iter()
                            .any(|argument| !expression_safe(argument, &initialized));
                    next
                }
                Stmt::If(branches) => {
                    let mut next = Summary::default();
                    let mut remainder = true;
                    for (condition, body) in &branches.branches {
                        next.unknown |= condition
                            .as_ref()
                            .is_some_and(|condition| !expression_safe(condition, &initialized));
                        let condition = truth(condition.as_ref());
                        if condition != Some(false) {
                            next.merge(self.block(body, file, &initialized));
                        }
                        if condition == Some(true) {
                            remainder = false;
                            break;
                        }
                    }
                    next.fallthrough |= remainder;
                    next
                }
                Stmt::Choice(_) => {
                    let mut next = Summary {
                        authored_flow: true,
                        ..Summary::default()
                    };
                    let mut guaranteed = false;
                    while let Some(Stmt::Choice(choice)) = body.get(index) {
                        let visible = truth(choice.cond.as_ref());
                        let enabled = truth(choice.enable.as_ref());
                        next.unknown |= choice
                            .cond
                            .as_ref()
                            .is_some_and(|condition| !expression_safe(condition, &initialized));
                        if visible != Some(false) {
                            next.unknown |=
                                choice.enable.as_ref().is_some_and(|condition| {
                                    !expression_safe(condition, &initialized)
                                }) || !text_safe(&choice.label, &initialized);
                        }
                        if visible != Some(false) && enabled != Some(false) {
                            next.pause = true;
                            next.merge(self.block(&choice.body, file, &initialized));
                            guaranteed |=
                                !choice.once && visible == Some(true) && enabled == Some(true);
                        }
                        index += 1;
                    }
                    next.fallthrough |= !guaranteed;
                    out.then(next);
                    continue;
                }
                Stmt::Scene(scene) => self.block(&scene.body, file, &initialized),
                Stmt::Text(text) | Stmt::Say(crate::language::SayStmt { text, .. }) => Summary {
                    unknown: !text_safe(&text.parts, &initialized),
                    ..Summary::continuing()
                },
                Stmt::Let(declaration) => {
                    let safe = expression_safe(&declaration.expr, &initialized);
                    initialized.insert(declaration.name.clone());
                    Summary {
                        unknown: !safe,
                        ..Summary::continuing()
                    }
                }
                Stmt::Local(local) => {
                    let safe = expression_safe(&local.expr, &initialized);
                    initialized.insert(local.name.clone());
                    Summary {
                        unknown: !safe,
                        ..Summary::continuing()
                    }
                }
                Stmt::Set(set) => Summary {
                    unknown: !initialized.contains(&set.name)
                        || !expression_safe(&set.expr, &initialized),
                    ..Summary::continuing()
                },
                Stmt::DynamicChange(_) => Summary::unknown(),
                _ => Summary::continuing(),
            };
            out.then(next);
            index += 1;
        }
        out
    }

    fn node(&mut self, path: &NodePath) -> Summary {
        let program = self.program;
        let event = &program.events[path.event];
        let file = program
            .event_files
            .get(path.event)
            .map(String::as_str)
            .unwrap_or_default();
        let mut body = event.body.as_slice();
        let mut continuations = Vec::new();
        for leaf in &path.scenes {
            let Some(index) = body
                .iter()
                .position(|stmt| matches!(stmt, Stmt::Scene(scene) if scene.name == *leaf))
            else {
                return Summary::unknown();
            };
            let Stmt::Scene(scene) = &body[index] else {
                unreachable!()
            };
            continuations.push(&body[index + 1..]);
            body = &scene.body;
        }
        let initialized = globals(program);
        let mut summary = self.block(body, file, &initialized);
        for continuation in continuations.into_iter().rev() {
            if !summary.fallthrough {
                break;
            }
            summary.then(self.block(continuation, file, &initialized));
        }
        // 准入可能拒绝重入。即使同事件场景无需再准入，也选择保守地不作闭环断言。
        summary.unknown |= truth(event.after.as_ref()) != Some(true)
            || event.effects.iter().any(|effect| {
                effect
                    .cond
                    .as_ref()
                    .is_some_and(|condition| !expression_safe(condition, &initialized))
            });
        summary
    }
}

pub(super) struct FlowSummary {
    pub reachable: Vec<bool>,
    pub fallthrough: Vec<bool>,
    pub authored_flow: Vec<bool>,
    pub cycles: Vec<Vec<(usize, Site)>>,
}

impl FlowSummary {
    pub fn new(program: &Program, symbols: &Symbols, nodes: &[GraphNode]) -> Self {
        let ids: HashMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.name.as_str(), index))
            .collect();
        let mut summarizer = Summarizer {
            program,
            fragments: HashMap::new(),
            active: HashSet::new(),
        };
        let mut summaries = Vec::new();
        let mut edges = Vec::new();
        for node in nodes {
            let Some(path) = symbols
                .events
                .get(&node.name)
                .or_else(|| symbols.scenes.get(&node.name))
            else {
                summaries.push(Summary::unknown());
                edges.push(Vec::new());
                continue;
            };
            let mut summary = summarizer.node(path);
            let mut outgoing = Vec::new();
            for (target, site) in &summary.jumps {
                let resolved = symbols
                    .resolve_target(target, Some(&program.events[path.event].name))
                    .map(|target| target.full_name(&program.events[target.event].name));
                if let Some(index) = resolved.as_deref().and_then(|name| ids.get(name)) {
                    outgoing.push((*index, site.clone()));
                } else {
                    summary.unknown = true;
                }
            }
            summaries.push(summary);
            edges.push(outgoing);
        }
        let adjacency: Vec<Vec<usize>> = edges
            .iter()
            .map(|outgoing| outgoing.iter().map(|(target, _)| *target).collect())
            .collect();
        let mut reachable = vec![false; nodes.len()];
        let mut queue = VecDeque::new();
        if let Some(&entry) = ids.get(program.entry.as_str()) {
            reachable[entry] = true;
            queue.push_back(entry);
        }
        while let Some(index) = queue.pop_front() {
            for &target in &adjacency[index] {
                if !reachable[target] {
                    reachable[target] = true;
                    queue.push_back(target);
                }
            }
        }
        let cycles = components(&adjacency)
            .into_iter()
            .filter(|group| {
                let members: HashSet<_> = group.iter().copied().collect();
                group.iter().all(|&index| {
                    summaries[index].can_close_cycle()
                        && !adjacency[index].is_empty()
                        && adjacency[index]
                            .iter()
                            .all(|target| members.contains(target))
                })
            })
            .map(|group| {
                group
                    .into_iter()
                    .map(|index| (index, edges[index][0].1.clone()))
                    .collect()
            })
            .collect();
        Self {
            reachable,
            fallthrough: summaries
                .iter()
                .map(|summary| summary.fallthrough)
                .collect(),
            authored_flow: summaries
                .iter()
                .map(|summary| summary.authored_flow)
                .collect(),
            cycles,
        }
    }
}

/// 迭代式 Kosaraju，避免长事件链导致分析栈溢出。
fn components(adjacency: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut visited = vec![false; adjacency.len()];
    let mut order = Vec::new();
    let mut reverse = vec![Vec::new(); adjacency.len()];
    for (index, targets) in adjacency.iter().enumerate() {
        for &target in targets {
            reverse[target].push(index);
        }
        if visited[index] {
            continue;
        }
        let mut stack = vec![(index, false)];
        while let Some((node, finished)) = stack.pop() {
            if finished {
                order.push(node);
            } else if !visited[node] {
                visited[node] = true;
                stack.push((node, true));
                stack.extend(adjacency[node].iter().rev().map(|&target| (target, false)));
            }
        }
    }
    visited.fill(false);
    let mut groups = Vec::new();
    while let Some(index) = order.pop() {
        if visited[index] {
            continue;
        }
        let mut group = Vec::new();
        let mut stack = vec![index];
        visited[index] = true;
        while let Some(node) = stack.pop() {
            group.push(node);
            for &previous in &reverse[node] {
                if !visited[previous] {
                    visited[previous] = true;
                    stack.push(previous);
                }
            }
        }
        group.sort_unstable();
        groups.push(group);
    }
    groups.sort_by_key(|group| group[0]);
    groups
}

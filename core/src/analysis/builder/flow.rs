use super::{flow_summary::FlowSummary, Ctx};
use crate::analysis_helpers::nodes_on_cycles;
use crate::ast::*;
use crate::diagnostic::{Diagnostic, DiagnosticSourceRole, Span};
use crate::source_provenance::{SourceOwner, StatementKind};
use std::collections::{HashSet, VecDeque};
impl<'a> Ctx<'a> {
    /// 流分析:A201 不可达、A202 缺尾、A205 once、A206 闭环、A107 未读变量。
    pub(super) fn flow_analysis(&mut self) {
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
        let flow = FlowSummary::new(self.program, &self.symbols, &self.graph_nodes);
        for group in &flow.cycles {
            let (first, site) = &group[0];
            let names = group
                .iter()
                .map(|(index, _)| format!("`{}`", self.graph_nodes[*index].name))
                .collect::<Vec<_>>()
                .join("、");
            let mut diagnostic = Diagnostic::warning(
                "A206",
                &site.file,
                site.span,
                format!("节点 {names} 构成没有选择暂停或出口的闭环；请补可退出分支或选择暂停"),
            );
            diagnostic.source_role = Some(if site.source_known {
                DiagnosticSourceRole::Target
            } else {
                DiagnosticSourceRole::Unavailable
            });
            for (index, site) in group {
                if index != first {
                    diagnostic = diagnostic.with_related_source_role(
                        &site.file,
                        site.span,
                        if site.source_known {
                            DiagnosticSourceRole::Target
                        } else {
                            DiagnosticSourceRole::Unavailable
                        },
                    );
                }
            }
            self.diags.push(diagnostic);
        }
        let mut reachable_tail = HashSet::new();
        let authored_flow = flow
            .authored_flow
            .get(entry as usize)
            .copied()
            .unwrap_or(false);
        for (index, node) in self.graph_nodes.iter().enumerate() {
            if authored_flow && flow.reachable[index] && flow.fallthrough[index] {
                if let Some(path) = self
                    .symbols
                    .events
                    .get(&node.name)
                    .or_else(|| self.symbols.scenes.get(&node.name))
                {
                    reachable_tail.insert(path.event);
                }
            }
        }
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
            let root_fallthrough = self
                .node_ids
                .get(&name)
                .is_some_and(|&node| flow.fallthrough[node as usize]);
            if reachable_tail.contains(&idx)
                || (self.program.events[idx].period.is_none() && root_fallthrough)
            {
                self.diags.push(tail_diagnostic(
                    &self.program.events[idx],
                    &self.cur_file,
                    loc,
                ));
            }
            let body = self.program.events[idx].body.clone();
            self.source_owner = Some(SourceOwner::new(&self.cur_file, loc.line));
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
            self.diags.push(
                Diagnostic::warning(
                    "A107",
                    &file,
                    span,
                    format!("变量 `{name}` 声明后从未被读取"),
                )
                .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target),
            );
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
                            let scope = ctx.enter_source(c.loc, StatementKind::Choice);
                            ctx.diags.push(Diagnostic::warning(
                                "A205",
                                &ctx.cur_file,
                                Span::new(c.loc.line, c.loc.column, 10),
                                "`once` 所在节点不会被重访,与默认粘性行为相同,可省略",
                            ));
                            ctx.leave_source(scope);
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

    pub(super) fn compute_depth(&self, entry: u32) -> Vec<u32> {
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

/// 只有明确开始试玩时调用；补充普通检查不能从历史正文推断的执行用途。
pub(super) fn execution_diagnostics(
    program: &Program,
    analysis: &crate::analysis::Analysis,
) -> Vec<Diagnostic> {
    let Some(&entry) = analysis.graph.ids.get(&program.entry) else {
        return Vec::new();
    };
    let Some(path) = analysis.symbols.events.get(&program.entry) else {
        return Vec::new();
    };
    let event = &program.events[path.event];
    if event.period.is_none() {
        return Vec::new();
    }
    let flow = FlowSummary::new(program, &analysis.symbols, &analysis.graph.nodes);
    if flow.authored_flow[entry as usize] || !flow.fallthrough[entry as usize] {
        return Vec::new();
    }
    let file = program
        .event_files
        .get(path.event)
        .map(String::as_str)
        .unwrap_or_default();
    vec![tail_diagnostic(event, file, event.loc)]
}

fn tail_diagnostic(event: &Event, file: &str, loc: Loc) -> Diagnostic {
    let name = &event.name;
    let span = Span::new(loc.line, loc.column, name.chars().count() as u32);
    if event.period.is_some() {
        Diagnostic::hint(
            "A202", file, span,
            format!("事件 `{name}` 在当前试玩路径中可能自然结束；可显式 `-> END`，作为历史资料无需补跃迁"),
        )
    } else {
        Diagnostic::warning(
            "A202",
            file,
            span,
            format!("事件 `{name}` 可能执行到结尾而没有跃迁(视同 END);建议显式 `-> END` 或补跃迁"),
        )
    }
}

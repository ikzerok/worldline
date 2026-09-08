//! 后续连接的显式条件与选择上下文；不执行代码，也不推断必然到达。
use crate::ast::{DivertTarget, Expr, Stmt, UnOp};
use crate::{EdgeKind, Program, RelationGraph, Symbols};
use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TransitionContext {
    pub conditions: Vec<String>,
    pub choices: Vec<String>,
}

pub fn expression(expr: &Expr) -> String {
    match expr {
        Expr::Num(n) => n.to_string(),
        Expr::Bool(b) => b.to_string(),
        Expr::Str(s) => crate::authoring::quote(s),
        Expr::Var { name, .. } => name.clone(),
        Expr::Unary { op, expr } => format!(
            "{}({})",
            if *op == UnOp::Not { "not " } else { "-" },
            expression(expr)
        ),
        Expr::Binary { op, lhs, rhs } => {
            format!("({} {} {})", expression(lhs), op.symbol(), expression(rhs))
        }
        Expr::Call { name, args, .. } => format!(
            "{}({})",
            name,
            args.iter().map(expression).collect::<Vec<_>>().join(", ")
        ),
    }
}

struct Occurrence {
    from: String,
    target: String,
    line: u32,
    choices: Vec<u32>,
    context: TransitionContext,
}

pub(crate) fn populate(program: &Program, symbols: &Symbols, graph: &mut RelationGraph) {
    fn walk(
        body: &[Stmt],
        node: &str,
        context: &TransitionContext,
        choices: &[u32],
        out: &mut Vec<Occurrence>,
    ) {
        for stmt in body {
            match stmt {
                Stmt::Divert(d) => {
                    if let DivertTarget::Node(target) = &d.target {
                        out.push(Occurrence {
                            from: node.into(),
                            target: target.clone(),
                            line: d.loc.line,
                            choices: choices.to_vec(),
                            context: context.clone(),
                        });
                    }
                }
                Stmt::Choice(c) => {
                    let mut inner = context.clone();
                    inner.choices.push(c.label_raw.clone());
                    if let Some(cond) = &c.cond {
                        inner.conditions.push(expression(cond));
                    }
                    if c.once {
                        inner.conditions.push("此选择尚未选取".into());
                    }
                    let mut positions = choices.to_vec();
                    positions.push(c.loc.line);
                    walk(&c.body, node, &inner, &positions, out);
                }
                Stmt::If(i) => {
                    let mut prior = Vec::new();
                    for (cond, branch) in &i.branches {
                        let mut inner = context.clone();
                        inner
                            .conditions
                            .extend(prior.iter().map(|p| format!("not ({p})")));
                        if let Some(cond) = cond {
                            let value = expression(cond);
                            inner.conditions.push(value.clone());
                            prior.push(value);
                        }
                        walk(branch, node, &inner, choices, out);
                    }
                }
                Stmt::Scene(s) => {
                    let target = format!("{node}.{}", s.name);
                    out.push(Occurrence {
                        from: node.into(),
                        target: target.clone(),
                        line: s.loc.line,
                        choices: choices.to_vec(),
                        context: context.clone(),
                    });
                    walk(&s.body, &target, context, &[], out);
                }
                _ => {}
            }
        }
    }
    for event in &program.events {
        let mut occurrences = Vec::new();
        walk(
            &event.body,
            &event.name,
            &TransitionContext::default(),
            &[],
            &mut occurrences,
        );
        for edge in &mut graph.edges {
            let from = &graph.nodes[edge.from as usize];
            let to = &graph.nodes[edge.to as usize];
            for occurrence in &occurrences {
                if occurrence.from != from.name
                    || !(occurrence.line == edge.line
                        || (edge.kind == EdgeKind::Choice
                            && occurrence.choices.contains(&edge.line)))
                {
                    continue;
                }
                let target = symbols
                    .resolve_target(&occurrence.target, Some(&event.name))
                    .map(|p| p.full_name(&program.events[p.event].name));
                if target.as_deref() == Some(&to.name)
                    && !edge.contexts.contains(&occurrence.context)
                {
                    edge.contexts.push(occurrence.context.clone());
                }
            }
        }
    }
    for edge in &mut graph.edges {
        let to = &graph.nodes[edge.to as usize];
        if let Some(path) = symbols.resolve_node(&to.name) {
            let from = &graph.nodes[edge.from as usize];
            let same_event = symbols
                .resolve_node(&from.name)
                .is_some_and(|source| source.event == path.event);
            if !to.is_event && same_event {
                continue;
            }
            let event = &program.events[path.event];
            let mut conditions = Vec::new();
            if let Some(perm) = &event.perm {
                conditions.push(format!("perm({perm})"));
            }
            if let Some(after) = &event.after {
                conditions.push(expression(after));
            }
            if !conditions.is_empty() {
                edge.target_requirement = Some(conditions.join(" and "));
            }
        }
    }
}

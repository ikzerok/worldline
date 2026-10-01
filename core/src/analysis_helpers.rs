//! 语义分析使用的纯流分析辅助函数。

use std::collections::{HashSet, VecDeque};

use crate::analysis::Symbols;
use crate::ast::*;

pub(crate) fn expr_kind_static(e: &Expr, symbols: &Symbols) -> Option<ValueKind> {
    match e {
        Expr::Var { name, .. } => symbols.vars.get(name).and_then(|v| v.kind),
        Expr::Call { name, args, .. } => match name.as_str() {
            "tag" => Some(ValueKind::Tag),
            "state" => Some(ValueKind::StateRef),
            "tags" | "members" | "union" | "intersect" | "difference" => Some(ValueKind::TagSet),
            "count" | "rnd" | "visits" | "turns" => Some(ValueKind::Num),
            "contains" | "has" | "seen" | "perm" => Some(ValueKind::Bool),
            "when" => args.get(1).and_then(|e| expr_kind_static(e, symbols)),
            _ => symbols.rule_results.get(name).copied(),
        },
        Expr::Unary { op, .. } => Some(match op {
            UnOp::Neg => ValueKind::Num,
            UnOp::Not => ValueKind::Bool,
        }),
        Expr::Binary { op, lhs, .. } => match op {
            BinOp::Eq
            | BinOp::Neq
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::And
            | BinOp::Or => Some(ValueKind::Bool),
            _ => expr_kind_static(lhs, symbols),
        },
        _ => e.static_kind(),
    }
}

pub(crate) fn expr_loc(e: &Expr) -> Loc {
    match e {
        Expr::Var { loc, .. } | Expr::Call { loc, .. } => *loc,
        Expr::Unary { expr, .. } => expr_loc(expr),
        Expr::Binary { lhs, .. } => expr_loc(lhs),
        _ => Loc::new(0, 1),
    }
}

/// 选择体内预序第一个跃迁(用于 Choice 边)。
pub(crate) fn first_divert(stmts: &[Stmt]) -> Option<&DivertTarget> {
    for s in stmts {
        match s {
            Stmt::Divert(d) => return Some(&d.target),
            Stmt::If(i) => {
                for (_, b) in &i.branches {
                    if let Some(t) = first_divert(b) {
                        return Some(t);
                    }
                }
            }
            Stmt::Choice(c) => {
                if let Some(t) = first_divert(&c.body) {
                    return Some(t);
                }
            }
            Stmt::Scene(sc) => {
                if let Some(t) = first_divert(&sc.body) {
                    return Some(t);
                }
            }
            _ => {}
        }
    }
    None
}

/// 每个节点是否处于环上(含自环):从后继可达自身。
pub(crate) fn nodes_on_cycles(adj: &[Vec<u32>]) -> HashSet<u32> {
    let n = adj.len();
    let mut out = HashSet::new();
    for start in 0..n {
        let mut seen = vec![false; n];
        let mut q: VecDeque<u32> = VecDeque::new();
        let mut found = false;
        for &m in &adj[start] {
            if m as usize == start {
                found = true;
                break;
            }
            if !seen[m as usize] {
                seen[m as usize] = true;
                q.push_back(m);
            }
        }
        while !found {
            let Some(v) = q.pop_front() else { break };
            for &m in &adj[v as usize] {
                if m as usize == start {
                    found = true;
                    break;
                }
                if !seen[m as usize] {
                    seen[m as usize] = true;
                    q.push_back(m);
                }
            }
        }
        if found {
            out.insert(start as u32);
        }
    }
    out
}

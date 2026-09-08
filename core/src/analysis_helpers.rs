//! 语义分析使用的纯流分析辅助函数。

use std::collections::{HashSet, VecDeque};

use crate::analysis::Symbols;
use crate::ast::*;

pub(crate) fn expr_kind_static(e: &Expr, symbols: &Symbols) -> Option<ValueKind> {
    match e {
        Expr::Var { name, .. } => symbols.vars.get(name).and_then(|v| v.kind),
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

/// 块的静态终止性:是否不存在"执行到块尾仍未跃迁"的路径(A202)。
pub(crate) fn terminates(stmts: &[Stmt]) -> bool {
    let mut i = 0;
    while i < stmts.len() {
        match &stmts[i] {
            Stmt::Divert(_) => return true,
            Stmt::If(s) => {
                let has_else = s.branches.last().map(|(c, _)| c.is_none()).unwrap_or(false);
                let all = s.branches.iter().all(|(_, b)| terminates(b));
                if has_else && all {
                    return true; // 必进某分支且各分支都终止
                }
                // 可能落到 if 之后:由后续语句决定
                i += 1;
                continue;
            }
            Stmt::Choice(_) => {
                let mut j = i;
                let mut unconditional = false;
                let mut all_bodies_term = true;
                while let Some(Stmt::Choice(c)) = stmts.get(j) {
                    if c.cond.is_none() {
                        unconditional = true;
                    }
                    if !terminates(&c.body) {
                        all_bodies_term = false;
                    }
                    j += 1;
                }
                if unconditional && all_bodies_term {
                    return true; // 必进某选择体且全部跃迁
                }
                i = j;
                continue;
            }
            Stmt::Scene(s) => {
                if terminates(&s.body) {
                    return true;
                }
                i += 1;
                continue;
            }
            Stmt::Text(_)
            | Stmt::Let(_)
            | Stmt::Set(_)
            | Stmt::Change(_)
            | Stmt::Anchor(_)
            | Stmt::Effect(_) => {
                i += 1;
            }
        }
    }
    false
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

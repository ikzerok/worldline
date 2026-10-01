//! 闭环证明的求值安全下界；不执行表达式，不推断动态规则结果。
use crate::ast::*;
use std::collections::HashSet;

pub(super) fn globals(program: &Program) -> HashSet<String> {
    // 成功创建Story之后，顶层初始化已全部完成；块内声明不提前初始化。
    program
        .lets
        .iter()
        .map(|declaration| declaration.name.clone())
        .collect()
}

pub(super) fn expression_safe(expr: &Expr, initialized: &HashSet<String>) -> bool {
    match expr {
        Expr::Num(_) | Expr::Str(_) | Expr::Bool(_) => true,
        Expr::Var { name, .. } => initialized.contains(name),
        Expr::Unary { expr, .. } => expression_safe(expr, initialized),
        Expr::Binary { op, lhs, rhs } => {
            let denominator_safe = !matches!(op, BinOp::Div | BinOp::Mod)
                || matches!(rhs.as_ref(), Expr::Num(value) if *value != 0.0);
            denominator_safe
                && expression_safe(lhs, initialized)
                && expression_safe(rhs, initialized)
        }
        Expr::Call { name, args, .. } => match (name.as_str(), args.as_slice()) {
            ("turns", []) => true,
            ("visits" | "seen" | "perm", [Expr::Str(_)]) => true,
            ("has", [state, tag]) => [state, tag]
                .into_iter()
                .all(|argument| matches!(argument, Expr::Var { .. } | Expr::Str(_))),
            // 规则、rnd及动态身份/集合保留运行失败可能，不把它们求值成常量。
            _ => false,
        },
    }
}

pub(super) fn text_safe(parts: &[TextPart], initialized: &HashSet<String>) -> bool {
    parts.iter().all(|part| match part {
        TextPart::Expr(expr) => expression_safe(expr, initialized),
        _ => true,
    })
}

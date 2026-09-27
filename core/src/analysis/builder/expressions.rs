use super::Ctx;
use crate::analysis_helpers::expr_loc;
use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
impl<'a> Ctx<'a> {
    /// 表达式检查:引用存在性 + 类型规则;expected 给出上下文期望类型。
    pub(super) fn check_expr(
        &mut self,
        e: &Expr,
        expected: Option<ValueKind>,
    ) -> Option<ValueKind> {
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
}

//! 条件作者器使用正式表达式 parser；仅增加调用方资源门，不改普通编译。
use super::{lex_expr, ExprParser, Tok};
use crate::{ast::Expr, source_provenance::ExpressionSource, Diagnostic};
use std::cell::Cell;
thread_local! { static EXCEEDED: Cell<Option<bool>> = const { Cell::new(None) }; }
struct Restore(Option<bool>);
impl Drop for Restore {
    fn drop(&mut self) {
        EXCEEDED.set(self.0);
    }
}
pub(crate) fn scoped<T>(operation: impl FnOnce() -> T) -> (T, bool) {
    let restore = Restore(EXCEEDED.replace(Some(false)));
    let value = operation();
    let exceeded = EXCEEDED.get() == Some(true);
    drop(restore);
    (value, exceeded)
}
pub(super) fn allow(tokens: &[(Tok, u32, u32)]) -> bool {
    if EXCEEDED.get().is_none() {
        return true;
    }
    let allowed = token_budget(tokens);
    if !allowed {
        EXCEEDED.set(Some(true));
    }
    allowed
}
fn token_budget(tokens: &[(Tok, u32, u32)]) -> bool {
    if tokens.len() > 2048 {
        return false;
    }
    let (mut depth, mut unary, mut operand) = (0usize, 0usize, true);
    let mut frames = Vec::new();
    for (token, _, _) in tokens {
        match token {
            Tok::KwNot | Tok::Op("-") if operand => unary += 1,
            Tok::LParen => {
                let added = unary + 1;
                frames.push(added);
                depth += added;
                unary = 0;
                operand = true;
            }
            Tok::RParen => {
                depth -= frames.pop().unwrap_or(0);
                unary = 0;
                operand = false;
            }
            Tok::Op(_) | Tok::KwAnd | Tok::KwOr | Tok::Comma => {
                unary = 0;
                operand = true;
            }
            _ => {
                unary = 0;
                operand = false;
            }
        }
        if depth + unary > 64 {
            return false;
        }
    }
    true
}

pub(crate) fn parse(
    source: &str,
    file: &str,
    line: u32,
    base: u32,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(Expr, ExpressionSource), ()> {
    let tokens = lex_expr(source, file, line, base, diagnostics);
    if !token_budget(&tokens) {
        return Err(());
    }
    let mut parser = ExprParser {
        toks: &tokens,
        pos: 0,
        file: file.to_owned(),
        line,
        end_column: base + source.chars().count() as u32 + 1,
        sources: Vec::new(),
    };
    let expression = parser.parse_expr(diagnostics);
    if parser.pos < tokens.len() {
        parser.err_here("表达式后有多余内容".into(), diagnostics);
    }
    let provenance = parser.sources.pop().unwrap_or_default();
    if !expression_budget(&expression) {
        discard(expression, provenance);
        return Err(());
    }
    Ok((expression, provenance))
}

pub(super) fn allow_expression(expression: &Expr) -> bool {
    if EXCEEDED.get().is_none() {
        return true;
    }
    let allowed = expression_budget(expression);
    if !allowed {
        EXCEEDED.set(Some(true));
    }
    allowed
}
fn expression_budget(expression: &Expr) -> bool {
    let mut pending = vec![(expression, 1)];
    while let Some((node, depth)) = pending.pop() {
        if depth > 64 {
            return false;
        }
        match node {
            Expr::Unary { expr, .. } => pending.push((expr, depth + 1)),
            Expr::Binary { lhs, rhs, .. } => {
                pending.push((rhs, depth + 1));
                pending.push((lhs, depth + 1));
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    pending.push((arg, depth + 1));
                }
            }
            _ => {}
        }
    }
    true
}
/// 超深左链迭代释放，拒绝路径也不递归 drop 整棵树。
pub(super) fn discard(expression: Expr, source: ExpressionSource) {
    let mut expressions = vec![expression];
    while let Some(node) = expressions.pop() {
        match node {
            Expr::Unary { expr, .. } => expressions.push(*expr),
            Expr::Binary { lhs, rhs, .. } => {
                expressions.push(*lhs);
                expressions.push(*rhs);
            }
            Expr::Call { args, .. } => expressions.extend(args),
            _ => {}
        }
    }
    let mut sources = vec![source];
    while let Some(source) = sources.pop() {
        sources.extend(source.children);
    }
}

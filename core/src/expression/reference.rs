//! 静态 ref 的身份 token 位置，复用正式表达式 AST 与词法器，允许同一语法的括号。
use super::*;
use std::ops::Range;

/// 返回 ID 字符串内部的字符范围；仅匹配完整 ref 调用的两个静态参数。
pub(crate) fn static_ref_id_range(source: &str, kind: &str, id: &str) -> Option<Range<usize>> {
    let mut diagnostics = Vec::new();
    let expression = parse_expr_src(source, "rename.wl", 1, 0, &mut diagnostics);
    let Expr::Call { name, args, .. } = expression else {
        return None;
    };
    let [Expr::Str(actual_kind), Expr::Str(actual_id)] = args.as_slice() else {
        return None;
    };
    if name != "ref" || actual_kind != kind || actual_id != id || !diagnostics.is_empty() {
        return None;
    }
    let tokens = lex_expr(source, "rename.wl", 1, 0, &mut diagnostics);
    let (_, column) = tokens
        .iter()
        .filter(|(token, _)| matches!(token, Tok::Str(_)))
        .nth(1)?;
    let start = (*column as usize).checked_sub(1)?;
    let chars: Vec<_> = source.chars().collect();
    let (_, end) = lexer::parse_quoted_raw(&chars, start, "rename.wl", 1, &mut diagnostics).ok()?;
    diagnostics.is_empty().then_some(start + 1..end - 1)
}

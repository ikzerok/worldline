//! 表达式词法、递归下降解析与文本内插。

use crate::ast::*;
use crate::diagnostic::{Diagnostic, DiagnosticSourceRole, Span};
use crate::lexer;
use crate::source_provenance::ExpressionSource;
mod quoted;
mod reference;
pub(crate) use reference::static_ref_id_range;
mod text;
pub use quoted::parse_quoted_interpolations_with_options;
pub(crate) use quoted::parse_with_sources as parse_quoted_with_sources;
pub(crate) use quoted::{remap_parts, static_literal_ranges};
pub(crate) use text::literal_ranges;
pub(crate) use text::parse_with_sources as parse_text_with_sources;
pub use text::{parse_interpolations, parse_interpolations_with_options};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    KwAnd,
    KwOr,
    KwNot,
    KwTrue,
    KwFalse,
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex_expr(
    src: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
) -> Vec<(Tok, u32, u32)> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let col = base_col + i as u32 + 1;
        if c == ' ' {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            match raw.parse::<f64>() {
                Ok(n) => toks.push((
                    Tok::Num(n),
                    base_col + start as u32 + 1,
                    base_col + i as u32 + 1,
                )),
                Err(_) => diags.push(
                    Diagnostic::error(
                        "P006",
                        file,
                        Span::new(line, col, raw.chars().count() as u32),
                        format!("非法数字 `{raw}`"),
                    )
                    .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target),
                ),
            }
            continue;
        }
        if c == '"' {
            let diagnostic_start = diags.len();
            let result = lexer::parse_quoted(&chars, i, file, line, diags);
            for diagnostic in &mut diags[diagnostic_start..] {
                let end = result.as_ref().map(|(_, end)| *end).unwrap_or(chars.len());
                diagnostic.span =
                    Span::new(line, base_col + i as u32 + 1, end.saturating_sub(i) as u32);
                diagnostic.source_role = Some(DiagnosticSourceRole::Expression);
            }
            match result {
                Ok((s, end)) => {
                    toks.push((Tok::Str(s), col, base_col + end as u32 + 1));
                    i = end;
                }
                Err(_) => break,
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let tok = match word.as_str() {
                "and" => Tok::KwAnd,
                "or" => Tok::KwOr,
                "not" => Tok::KwNot,
                "true" => Tok::KwTrue,
                "false" => Tok::KwFalse,
                _ => Tok::Ident(word),
            };
            toks.push((tok, base_col + start as u32 + 1, base_col + i as u32 + 1));
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let tok = match two.as_str() {
            "==" => Some(Tok::Op("==")),
            "!=" => Some(Tok::Op("!=")),
            "<=" => Some(Tok::Op("<=")),
            ">=" => Some(Tok::Op(">=")),
            _ => None,
        };
        if let Some(t) = tok {
            toks.push((t, col, col + 2));
            i += 2;
            continue;
        }
        let tok = match c {
            '+' => Tok::Op("+"),
            '-' => Tok::Op("-"),
            '*' => Tok::Op("*"),
            '/' => Tok::Op("/"),
            '%' => Tok::Op("%"),
            '<' => Tok::Op("<"),
            '>' => Tok::Op(">"),
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' => Tok::Comma,
            _ => {
                diags.push(
                    Diagnostic::error(
                        "P006",
                        file,
                        Span::new(line, col, 1),
                        format!("表达式中出现非法字符 `{c}`"),
                    )
                    .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target),
                );
                i += 1;
                continue;
            }
        };
        toks.push((tok, col, col + 1));
        i += 1;
    }
    toks
}

/// 表达式解析器:token 流上的递归下降,按规范优先级。
struct ExprParser<'a> {
    toks: &'a [(Tok, u32, u32)],
    pos: usize,
    file: String,
    line: u32,
    end_column: u32,
    sources: Vec<ExpressionSource>,
}

impl<'a> ExprParser<'a> {
    fn err_here(&self, msg: String, diags: &mut Vec<Diagnostic>) {
        let col = self
            .toks
            .get(self.pos)
            .map(|t| t.1)
            .unwrap_or(self.end_column);
        let mut diagnostic = Diagnostic::error(
            "P006",
            &self.file,
            Span::new(
                self.line,
                col,
                self.toks.get(self.pos).map(|t| t.2 - t.1).unwrap_or(0),
            ),
            msg,
        );
        if self.toks.get(self.pos).is_some() {
            diagnostic.source_role = Some(DiagnosticSourceRole::Target);
        }
        diags.push(diagnostic);
    }

    fn combine_source(&mut self, count: usize, start: Option<u32>) {
        let children = self
            .sources
            .split_off(self.sources.len().saturating_sub(count));
        let start = start.or_else(|| children.first().and_then(|s| s.span.map(|s| s.column)));
        let end = self.toks.get(self.pos.saturating_sub(1)).map(|t| t.2);
        let span = start
            .zip(end)
            .map(|(start, end)| Span::new(self.line, start, end.saturating_sub(start)));
        self.sources.push(ExpressionSource {
            span,
            file: self.file.clone(),
            children,
        });
    }

    fn leaf_source(&mut self, column: u32) {
        let end = self
            .toks
            .get(self.pos.saturating_sub(1))
            .map(|t| t.2)
            .unwrap_or(column);
        self.sources.push(ExpressionSource::leaf(
            Span::new(self.line, column, end.saturating_sub(column)),
            &self.file,
        ));
    }

    fn parse_expr(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        self.parse_or(diags)
    }

    fn parse_or(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let mut lhs = self.parse_and(diags);
        while matches!(self.peek(), Some((Tok::KwOr, _, _))) {
            self.pos += 1;
            let rhs = self.parse_and(diags);
            self.combine_source(2, None);
            lhs = Expr::Binary {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        lhs
    }

    fn parse_and(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let mut lhs = self.parse_not(diags);
        while matches!(self.peek(), Some((Tok::KwAnd, _, _))) {
            self.pos += 1;
            let rhs = self.parse_not(diags);
            self.combine_source(2, None);
            lhs = Expr::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        lhs
    }

    fn parse_not(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        if matches!(self.peek(), Some((Tok::KwNot, _, _))) {
            let start = self.peek().map(|t| t.1);
            self.pos += 1;
            let inner = self.parse_not(diags);
            self.combine_source(1, start);
            return Expr::Unary {
                op: UnOp::Not,
                expr: Box::new(inner),
            };
        }
        self.parse_cmp(diags)
    }

    fn parse_cmp(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let lhs = self.parse_add(diags);
        let op = match self.peek() {
            Some((Tok::Op("=="), _, _)) => Some(BinOp::Eq),
            Some((Tok::Op("!="), _, _)) => Some(BinOp::Neq),
            Some((Tok::Op("<"), _, _)) => Some(BinOp::Lt),
            Some((Tok::Op("<="), _, _)) => Some(BinOp::Le),
            Some((Tok::Op(">"), _, _)) => Some(BinOp::Gt),
            Some((Tok::Op(">="), _, _)) => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let rhs = self.parse_add(diags);
            self.combine_source(2, None);
            return Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        lhs
    }

    fn parse_add(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let mut lhs = self.parse_mul(diags);
        loop {
            let op = match self.peek() {
                Some((Tok::Op("+"), _, _)) => BinOp::Add,
                Some((Tok::Op("-"), _, _)) => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_mul(diags);
            self.combine_source(2, None);
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        lhs
    }

    fn parse_mul(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let mut lhs = self.parse_unary(diags);
        loop {
            let op = match self.peek() {
                Some((Tok::Op("*"), _, _)) => BinOp::Mul,
                Some((Tok::Op("/"), _, _)) => BinOp::Div,
                Some((Tok::Op("%"), _, _)) => BinOp::Mod,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_unary(diags);
            self.combine_source(2, None);
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        lhs
    }

    fn parse_unary(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        if matches!(self.peek(), Some((Tok::Op("-"), _, _))) {
            let start = self.peek().map(|t| t.1);
            self.pos += 1;
            let inner = self.parse_unary(diags);
            self.combine_source(1, start);
            return Expr::Unary {
                op: UnOp::Neg,
                expr: Box::new(inner),
            };
        }
        self.parse_primary(diags)
    }

    fn parse_primary(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let (tok, col, _) = match self.peek().cloned() {
            Some(t) => t,
            None => {
                self.err_here("表达式意外结束".into(), diags);
                self.sources.push(ExpressionSource::default());
                return Expr::Num(0.0);
            }
        };
        let loc = Loc::new(self.line, col);
        match tok {
            Tok::Num(n) => {
                self.pos += 1;
                self.leaf_source(col);
                Expr::Num(n)
            }
            Tok::Str(s) => {
                self.pos += 1;
                self.leaf_source(col);
                Expr::Str(s)
            }
            Tok::KwTrue => {
                self.pos += 1;
                self.leaf_source(col);
                Expr::Bool(true)
            }
            Tok::KwFalse => {
                self.pos += 1;
                self.leaf_source(col);
                Expr::Bool(false)
            }
            Tok::Ident(name) => {
                self.pos += 1;
                // 内建函数
                if matches!(self.peek(), Some((Tok::LParen, _, _))) {
                    self.pos += 1;
                    if matches!(name.as_str(), "visits" | "seen" | "perm") {
                        // 节点/权限谓词:参数是裸标识符
                        let arg = match self.peek().cloned() {
                            Some((Tok::Ident(a), start, _)) => {
                                self.pos += 1;
                                self.leaf_source(start);
                                Some(a)
                            }
                            Some((Tok::Str(a), start, _)) => {
                                self.pos += 1;
                                self.leaf_source(start);
                                Some(a)
                            }
                            _ => None,
                        };
                        let Some(arg) = arg else {
                            self.err_here(
                                format!("{name} 需要一个名称参数,如 {name}(hall)"),
                                diags,
                            );
                            self.combine_source(0, Some(col));
                            return Expr::Call {
                                name,
                                args: vec![],
                                loc,
                            };
                        };
                        if !matches!(self.peek(), Some((Tok::RParen, _, _))) {
                            self.err_here(format!("{name} 的参数之后应为 `)`"), diags);
                        } else {
                            self.pos += 1;
                        }
                        self.combine_source(1, Some(col));
                        return Expr::Call {
                            name,
                            args: vec![Expr::Str(arg)],
                            loc,
                        };
                    }
                    // 通用调用:turns() / rnd(a, b)
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some((Tok::RParen, _, _))) {
                        loop {
                            args.push(self.parse_expr(diags));
                            if matches!(self.peek(), Some((Tok::Comma, _, _))) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    if matches!(self.peek(), Some((Tok::RParen, _, _))) {
                        self.pos += 1;
                    } else {
                        self.err_here("调用参数之后应为 `)`".into(), diags);
                    }
                    self.combine_source(args.len(), Some(col));
                    return Expr::Call { name, args, loc };
                }
                self.leaf_source(col);
                Expr::Var { name, loc }
            }
            Tok::LParen => {
                self.pos += 1;
                let inner = self.parse_expr(diags);
                if matches!(self.peek(), Some((Tok::RParen, _, _))) {
                    self.pos += 1;
                } else {
                    self.err_here("括号未闭合".into(), diags);
                }
                if let Some(source) = self.sources.last_mut() {
                    let end = self
                        .toks
                        .get(self.pos.saturating_sub(1))
                        .map(|t| t.2)
                        .unwrap_or(col);
                    source.span = Some(Span::new(self.line, col, end.saturating_sub(col)));
                }
                inner
            }
            other => {
                self.err_here(format!("此处不应出现 {other:?}"), diags);
                self.pos += 1;
                self.sources.push(ExpressionSource::default());
                Expr::Num(0.0)
            }
        }
    }

    fn peek(&self) -> Option<&(Tok, u32, u32)> {
        self.toks.get(self.pos)
    }
}

/// 表达式入口:解析失败时报告 P006 并返回 0(分析层会跳过已报错表达式)。
pub fn parse_expr_src(
    src: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
) -> Expr {
    parse_expr_with_source(src, file, line, base_col, diags).0
}

pub(crate) fn parse_expr_with_source(
    src: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
) -> (Expr, ExpressionSource) {
    let toks = lex_expr(src, file, line, base_col, diags);
    let mut p = ExprParser {
        toks: &toks,
        pos: 0,
        file: file.to_string(),
        line,
        end_column: base_col + src.chars().count() as u32 + 1,
        sources: Vec::new(),
    };
    let expr = p.parse_expr(diags);
    if p.pos < toks.len() {
        p.err_here("表达式后有多余内容".into(), diags);
    }
    let source = p.sources.pop().unwrap_or_default();
    (expr, source)
}

/// 文本内插:把 `你有 {coins} 枚` 切成字面量与表达式片段。
/// 输入为原始文本(转义未解码);输出字面量已解码。
#[cfg(test)]
mod tests {
    use super::{parse_expr_src, parse_interpolations};
    use crate::ast::{BinOp, Expr, TextPart};

    #[test]
    fn multiplication_binds_more_tightly_than_addition() {
        let mut diagnostics = Vec::new();
        let expr = parse_expr_src("1 + 2 * 3", "test.wl", 1, 0, &mut diagnostics);
        assert!(diagnostics.is_empty());
        let Expr::Binary {
            op: BinOp::Add,
            rhs,
            ..
        } = expr
        else {
            panic!("expected addition at the expression root");
        };
        assert!(matches!(*rhs, Expr::Binary { op: BinOp::Mul, .. }));
    }

    #[test]
    fn interpolation_keeps_literal_and_expression_parts() {
        let mut diagnostics = Vec::new();
        let parts = parse_interpolations("余额 {coins}。", "test.wl", 2, 3, &mut diagnostics);
        assert!(diagnostics.is_empty());
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], TextPart::Str(text) if text == "余额 "));
        assert!(matches!(&parts[1], TextPart::Expr(Expr::Var { name, .. }) if name == "coins"));
        assert!(matches!(&parts[2], TextPart::Str(text) if text == "。"));
    }
}

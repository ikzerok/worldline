//! 表达式词法、递归下降解析与文本内插。

use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer;

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
) -> Vec<(Tok, u32)> {
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
                Ok(n) => toks.push((Tok::Num(n), base_col + start as u32 + 1)),
                Err(_) => diags.push(Diagnostic::error(
                    "P006",
                    file,
                    Span::new(line, col, raw.chars().count() as u32),
                    format!("非法数字 `{raw}`"),
                )),
            }
            continue;
        }
        if c == '"' {
            match lexer::parse_quoted(&chars, i, file, line, diags) {
                Ok((s, end)) => {
                    toks.push((Tok::Str(s), col));
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
            toks.push((tok, base_col + start as u32 + 1));
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
            toks.push((t, col));
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
                diags.push(Diagnostic::error(
                    "P006",
                    file,
                    Span::new(line, col, 1),
                    format!("表达式中出现非法字符 `{c}`"),
                ));
                i += 1;
                continue;
            }
        };
        toks.push((tok, col));
        i += 1;
    }
    toks
}

/// 表达式解析器:token 流上的递归下降,按规范优先级。
struct ExprParser<'a> {
    toks: &'a [(Tok, u32)],
    pos: usize,
    file: String,
    line: u32,
}

impl<'a> ExprParser<'a> {
    fn err_here(&self, msg: String, diags: &mut Vec<Diagnostic>) {
        let col = self.toks.get(self.pos).map(|t| t.1).unwrap_or(1);
        diags.push(Diagnostic::error(
            "P006",
            &self.file,
            Span::new(self.line, col, 1),
            msg,
        ));
    }

    fn parse_expr(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        self.parse_or(diags)
    }

    fn parse_or(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let mut lhs = self.parse_and(diags);
        while matches!(self.peek(), Some((Tok::KwOr, _))) {
            self.pos += 1;
            let rhs = self.parse_and(diags);
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
        while matches!(self.peek(), Some((Tok::KwAnd, _))) {
            self.pos += 1;
            let rhs = self.parse_not(diags);
            lhs = Expr::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        lhs
    }

    fn parse_not(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        if matches!(self.peek(), Some((Tok::KwNot, _))) {
            self.pos += 1;
            let inner = self.parse_not(diags);
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
            Some((Tok::Op("=="), _)) => Some(BinOp::Eq),
            Some((Tok::Op("!="), _)) => Some(BinOp::Neq),
            Some((Tok::Op("<"), _)) => Some(BinOp::Lt),
            Some((Tok::Op("<="), _)) => Some(BinOp::Le),
            Some((Tok::Op(">"), _)) => Some(BinOp::Gt),
            Some((Tok::Op(">="), _)) => Some(BinOp::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let rhs = self.parse_add(diags);
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
                Some((Tok::Op("+"), _)) => BinOp::Add,
                Some((Tok::Op("-"), _)) => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_mul(diags);
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
                Some((Tok::Op("*"), _)) => BinOp::Mul,
                Some((Tok::Op("/"), _)) => BinOp::Div,
                Some((Tok::Op("%"), _)) => BinOp::Mod,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_unary(diags);
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        lhs
    }

    fn parse_unary(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        if matches!(self.peek(), Some((Tok::Op("-"), _))) {
            self.pos += 1;
            let inner = self.parse_unary(diags);
            return Expr::Unary {
                op: UnOp::Neg,
                expr: Box::new(inner),
            };
        }
        self.parse_primary(diags)
    }

    fn parse_primary(&mut self, diags: &mut Vec<Diagnostic>) -> Expr {
        let (tok, col) = match self.peek().cloned() {
            Some(t) => t,
            None => {
                self.err_here("表达式意外结束".into(), diags);
                return Expr::Num(0.0);
            }
        };
        let loc = Loc::new(self.line, col);
        match tok {
            Tok::Num(n) => {
                self.pos += 1;
                Expr::Num(n)
            }
            Tok::Str(s) => {
                self.pos += 1;
                Expr::Str(s)
            }
            Tok::KwTrue => {
                self.pos += 1;
                Expr::Bool(true)
            }
            Tok::KwFalse => {
                self.pos += 1;
                Expr::Bool(false)
            }
            Tok::Ident(name) => {
                self.pos += 1;
                // 内建函数
                if matches!(self.peek(), Some((Tok::LParen, _))) {
                    self.pos += 1;
                    if matches!(name.as_str(), "visits" | "seen" | "perm") {
                        // 节点/权限谓词:参数是裸标识符
                        let arg = match self.peek().cloned() {
                            Some((Tok::Ident(a), _)) => {
                                self.pos += 1;
                                Some(a)
                            }
                            Some((Tok::Str(a), _)) => {
                                self.pos += 1;
                                Some(a)
                            }
                            _ => None,
                        };
                        let Some(arg) = arg else {
                            self.err_here(
                                format!("{name} 需要一个名称参数,如 {name}(hall)"),
                                diags,
                            );
                            return Expr::Call {
                                name,
                                args: vec![],
                                loc,
                            };
                        };
                        if !matches!(self.peek(), Some((Tok::RParen, _))) {
                            self.err_here(format!("{name} 的参数之后应为 `)`"), diags);
                        } else {
                            self.pos += 1;
                        }
                        return Expr::Call {
                            name,
                            args: vec![Expr::Str(arg)],
                            loc,
                        };
                    }
                    // 通用调用:turns() / rnd(a, b)
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some((Tok::RParen, _))) {
                        loop {
                            args.push(self.parse_expr(diags));
                            if matches!(self.peek(), Some((Tok::Comma, _))) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    if matches!(self.peek(), Some((Tok::RParen, _))) {
                        self.pos += 1;
                    } else {
                        self.err_here("调用参数之后应为 `)`".into(), diags);
                    }
                    return Expr::Call { name, args, loc };
                }
                Expr::Var { name, loc }
            }
            Tok::LParen => {
                self.pos += 1;
                let inner = self.parse_expr(diags);
                if matches!(self.peek(), Some((Tok::RParen, _))) {
                    self.pos += 1;
                } else {
                    self.err_here("括号未闭合".into(), diags);
                }
                inner
            }
            other => {
                self.err_here(format!("此处不应出现 {other:?}"), diags);
                self.pos += 1;
                Expr::Num(0.0)
            }
        }
    }

    fn peek(&self) -> Option<&(Tok, u32)> {
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
    let toks = lex_expr(src, file, line, base_col, diags);
    let mut p = ExprParser {
        toks: &toks,
        pos: 0,
        file: file.to_string(),
        line,
    };
    let expr = p.parse_expr(diags);
    if p.pos < toks.len() {
        p.err_here("表达式后有多余内容".into(), diags);
    }
    expr
}

/// 文本内插:把 `你有 {coins} 枚` 切成字面量与表达式片段。
/// 输入为原始文本(转义未解码);输出字面量已解码。
pub fn parse_interpolations(
    raw: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
) -> Vec<TextPart> {
    parse_interpolations_with_options(
        raw,
        file,
        line,
        base_col,
        diags,
        crate::compiler::CompileOptions::default(),
    )
}

pub fn parse_interpolations_with_options(
    raw: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
    options: crate::compiler::CompileOptions,
) -> Vec<TextPart> {
    let chars: Vec<char> = raw.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            match n {
                'n' => lit.push('\n'),
                't' => lit.push('\t'),
                '{' | '}' | '#' | '~' | '"' | '\\' | '[' | ']' => lit.push(n),
                other => {
                    diags.push(Diagnostic::error(
                        "P003",
                        file,
                        Span::new(line, base_col + i as u32 + 1, 2),
                        format!("未知的转义 \\{other}"),
                    ));
                    lit.push('\\');
                    lit.push(other);
                }
            }
            i += 2;
            continue;
        }
        if c == '[' && chars.get(i + 1) == Some(&'[') {
            let end = (i + 2..chars.len().saturating_sub(1))
                .find(|&j| chars[j] == ']' && chars[j + 1] == ']');
            let Some(end) = end else {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(line, base_col + i as u32, 2),
                    "正文对象链接未闭合",
                ));
                lit.extend(chars[i..].iter());
                break;
            };
            let inner: String = chars[i + 2..end].iter().collect();
            if let Some((target, label)) =
                crate::navigation::parse_link_with_options(&inner, options)
            {
                if !lit.is_empty() {
                    parts.push(TextPart::Str(std::mem::take(&mut lit)));
                }
                parts.push(TextPart::Link(crate::navigation::InlineLink {
                    target,
                    label,
                    start: i,
                    end: end + 2,
                    column: base_col + i as u32,
                }));
            } else {
                diags.push(Diagnostic::error(
                    "P004",
                    file,
                    Span::new(line, base_col + i as u32, (end + 2 - i) as u32),
                    "正文链接需要 [[对象类型:ID|显示文字]]，显示文字不可包含语法分隔符",
                ));
                lit.extend(chars[i..end + 2].iter());
            }
            i = end + 2;
            continue;
        }
        if c == '{' {
            if !lit.is_empty() {
                parts.push(TextPart::Str(std::mem::take(&mut lit)));
            }
            // 找到配对的 `}`(允许嵌套括号内的表达式含字符串,字符串里的 } 不算)
            let start = i + 1;
            let mut j = start;
            let mut in_str = false;
            while j < chars.len() {
                let d = chars[j];
                if in_str {
                    if d == '\\' {
                        j += 1;
                    } else if d == '"' {
                        in_str = false;
                    }
                } else if d == '"' {
                    in_str = true;
                } else if d == '}' {
                    break;
                }
                j += 1;
            }
            if j >= chars.len() {
                diags.push(Diagnostic::error(
                    "P003",
                    file,
                    Span::new(line, base_col + i as u32 + 1, 1),
                    "插值 `{` 未闭合",
                ));
                break;
            }
            let inner: String = chars[start..j].iter().collect();
            let expr = parse_expr_src(&inner, file, line, base_col + start as u32, diags);
            parts.push(TextPart::Expr(expr));
            i = j + 1;
            continue;
        }
        lit.push(c);
        i += 1;
    }
    if !lit.is_empty() {
        parts.push(TextPart::Str(lit));
    }
    parts
}

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

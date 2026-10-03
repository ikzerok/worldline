//! choice 尾部只在字符串、括号外识别显式 1.12 子句。
use super::super::{parse_quoted, Diagnostic, Span};
use crate::CompileOptions;

#[allow(clippy::too_many_arguments)]
pub(super) fn parse(
    tail: &str,
    file: &str,
    line: u32,
    column: u32,
    options: CompileOptions,
    diags: &mut Vec<Diagnostic>,
    origins: &mut crate::lexer::LineSource,
) -> (Option<String>, Option<String>, Option<String>, Option<Span>) {
    let leading = tail.chars().take_while(|c| c.is_whitespace()).count() as u32;
    let tail = tail.trim();
    if let Some(condition) = tail.strip_prefix("if").map(str::trim_start) {
        origins.condition =
            column - 1 + leading + tail[..tail.len() - condition.len()].chars().count() as u32;
    }
    let legacy = || tail.strip_prefix("if").map(|v| v.trim().to_owned());
    if !options.language_version.supports_language_112() {
        if tail.is_empty() || tail.starts_with("if") {
            return (legacy(), None, None, None);
        }
        error(
            diags,
            file,
            line,
            column,
            "choice enable / disabled 需要显式语言 1.12",
        );
        return (None, None, None, None);
    }
    let enable_at = if after_keyword(tail, "enable").is_some() {
        Some(0)
    } else {
        marker(tail, "enable", |i| {
            after_keyword(&tail[..i], "if").is_some_and(is_expression)
        })
    };
    let Some(index) = enable_at else {
        if tail.is_empty() || after_keyword(tail, "if").is_some() {
            return (legacy(), None, None, None);
        }
        error(
            diags,
            file,
            line,
            column,
            "choice 标签后需要 if 条件或 enable 条件 disabled \"说明\"",
        );
        return (None, None, None, None);
    };
    let cond = after_keyword(tail[..index].trim(), "if").map(str::to_owned);
    let enabled = tail[index + "enable".len()..].trim();
    origins.enable =
        column - 1 + leading + tail[..tail.len() - enabled.len()].chars().count() as u32;
    let Some(disabled_at) = marker(enabled, "disabled", |i| is_expression(enabled[..i].trim()))
    else {
        error(
            diags,
            file,
            line,
            column,
            "enable 必须配对 disabled \"作者禁用说明\"",
        );
        return (cond, Some(enabled.into()), None, None);
    };
    let expr = enabled[..disabled_at].trim();
    let reason_src = enabled[disabled_at + "disabled".len()..].trim();
    if expr.is_empty() {
        error(diags, file, line, column, "enable 后需要布尔条件表达式");
    }
    let chars: Vec<_> = reason_src.chars().collect();
    let reason = if chars.first() == Some(&'"') {
        match parse_quoted(&chars, 0, file, line, diags) {
            Ok((reason, end))
                if end == chars.len()
                    && !reason.trim().is_empty()
                    && !reason.contains(['\n', '\r']) =>
            {
                Some(reason)
            }
            _ => {
                error(
                    diags,
                    file,
                    line,
                    column,
                    "disabled 后须为非空单行字符串，不能有其他子句",
                );
                None
            }
        }
    } else {
        error(diags, file, line, column, "disabled 后需要双引号作者说明");
        None
    };
    let span = reason.as_ref().map(|_| {
        Span::new(
            line,
            column + leading + tail[..tail.len() - reason_src.len()].chars().count() as u32 + 1,
            chars.len().saturating_sub(2) as u32,
        )
    });
    (cond, Some(expr.into()), reason, span)
}

fn marker(source: &str, word: &str, accept: impl Fn(usize) -> bool) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 0u32;
    for (i, c) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        if c == '(' || c == '[' {
            depth += 1;
        }
        if c == ')' || c == ']' {
            depth = depth.saturating_sub(1);
        }
        if depth == 0
            && source[i..].starts_with(word)
            && (i == 0 || source[..i].ends_with(char::is_whitespace))
            && source[i + word.len()..].starts_with(char::is_whitespace)
            && accept(i)
        {
            return Some(i);
        }
    }
    None
}
fn after_keyword<'a>(source: &'a str, keyword: &str) -> Option<&'a str> {
    source
        .strip_prefix(keyword)
        .filter(|tail| tail.starts_with(char::is_whitespace))
        .map(str::trim)
}
fn is_expression(source: &str) -> bool {
    if source.is_empty() {
        return false;
    }
    let mut diagnostics = Vec::new();
    crate::expression::parse_expr_src(source, "choice", 1, 1, &mut diagnostics);
    diagnostics.is_empty()
}
fn error(diags: &mut Vec<Diagnostic>, file: &str, line: u32, column: u32, message: &str) {
    diags.push(Diagnostic::error(
        "P004",
        file,
        Span::new(line, column, 6),
        message,
    ));
}

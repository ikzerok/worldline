use super::*;
use crate::ast::{Expr, TextPart, TextStmt};
use crate::source_provenance::ExpressionSlot;
use crate::{CompileResult, Span};

pub(super) fn project(
    result: &CompileResult,
    text: &TextStmt,
    source: &ReviewSource,
    quoted: bool,
) -> Result<Vec<DialogueMappedPart>> {
    let original = result
        .sources
        .get(&PathBuf::from(&source.file))
        .ok_or_else(|| DialogueError::new("SOURCE_UNAVAILABLE", "台词文件来源不存在"))?;
    let line_start = original[..source.byte_start]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let mut expression_index = 0;
    let mut output = Vec::new();
    for part in &text.parts {
        let (part, source_range) = match part {
            TextPart::Str(text) => (DialoguePart::Literal { text: text.clone() }, None),
            TextPart::Expr(_) => {
                let mapped = result
                    .program
                    .source_provenance
                    .expression(
                        &source.file,
                        text.loc.line,
                        ExpressionSlot::Text(expression_index),
                    )
                    .ok_or_else(|| {
                        DialogueError::new("SOURCE_UNAVAILABLE", "表达式缺少正式来源映射")
                    })?;
                expression_index += 1;
                if mapped.file != source.file {
                    return Err(DialogueError::new(
                        "SOURCE_UNAVAILABLE",
                        "表达式来源文件无法确认",
                    ));
                }
                let range = span_range(
                    original,
                    line_start,
                    mapped.span.ok_or_else(|| {
                        DialogueError::new("SOURCE_UNAVAILABLE", "表达式缺少正式范围")
                    })?,
                )?;
                let raw = &original[range.clone()];
                let value = if quoted {
                    let mut diagnostics = Vec::new();
                    let value = crate::lexer::decode_escapes(
                        raw,
                        &source.file,
                        mapped.span.unwrap(),
                        &mut diagnostics,
                    );
                    if !diagnostics.is_empty() {
                        return Err(DialogueError::new(
                            "SOURCE_UNAVAILABLE",
                            "表达式外层转义无法确认",
                        ));
                    }
                    value
                } else {
                    raw.into()
                };
                (DialoguePart::Expression { source: value }, Some(range))
            }
            TextPart::Link(link) => {
                let span = Span::new(text.loc.line, link.column, (link.end - link.start) as u32);
                let range = span_range(original, line_start, span)?;
                let target = if link.target.kind == "file" {
                    TargetRef::new(
                        "file",
                        &crate::catalog::resolved_asset(&source.file, &link.target.id)
                            .to_string_lossy(),
                    )
                } else {
                    link.target.clone()
                };
                (
                    DialoguePart::Link {
                        target,
                        label: link.label.clone(),
                    },
                    Some(range),
                )
            }
        };
        output.push(DialogueMappedPart { part, source_range });
    }
    Ok(output)
}

fn span_range(source: &str, line_start: usize, span: Span) -> Result<Range<usize>> {
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |i| line_start + i);
    let line = source[line_start..line_end].trim_end_matches('\r');
    let start = span
        .column
        .checked_sub(1)
        .ok_or_else(|| DialogueError::new("SOURCE_UNAVAILABLE", "源列无法确认"))?
        as usize;
    let end = start
        .checked_add(span.length as usize)
        .ok_or_else(|| DialogueError::new("SOURCE_UNAVAILABLE", "源范围溢出"))?;
    let byte = |at| {
        line.char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(line.len()))
            .nth(at)
    };
    Ok(line_start
        + byte(start).ok_or_else(|| DialogueError::new("SOURCE_UNAVAILABLE", "源起点无法确认"))?
        ..line_start
            + byte(end)
                .ok_or_else(|| DialogueError::new("SOURCE_UNAVAILABLE", "源终点无法确认"))?)
}

pub(super) fn normalized(draft: &DialogueDraft) -> DialogueDraft {
    let mut result = draft.clone();
    result.parts.clear();
    for part in &draft.parts {
        if let DialoguePart::Literal { text } = part {
            if text.is_empty() {
                continue;
            }
            if let Some(DialoguePart::Literal { text: previous }) = result.parts.last_mut() {
                previous.push_str(text);
                continue;
            }
        }
        result.parts.push(part.clone());
    }
    result
}

pub(super) fn equivalent(expected: &DialogueDraft, actual: &DialogueDraft) -> bool {
    let expected = normalized(expected);
    let actual = normalized(actual);
    expected.kind == actual.kind
        && expected.speaker == actual.speaker
        && expected.direction == actual.direction
        && expected.parts.len() == actual.parts.len()
        && expected
            .parts
            .iter()
            .zip(&actual.parts)
            .all(|(left, right)| match (left, right) {
                (
                    DialoguePart::Expression { source: left },
                    DialoguePart::Expression { source: right },
                ) => {
                    left == right
                        || match (expression(left), expression(right)) {
                            (Ok(left), Ok(right)) => expr_equal(&left, &right),
                            _ => false,
                        }
                }
                _ => left == right,
            })
}

pub(super) fn expression(source: &str) -> Result<Expr> {
    let mut diagnostics = Vec::new();
    let expression = crate::expression::outline_budget::scoped(|| {
        crate::expression::parse_expr_src(source, "dialogue.wl", 1, 0, &mut diagnostics)
    })
    .map_err(|_| {
        DialogueError::new(
            "BUDGET_EXCEEDED",
            "新插值超过 256 token 或 64 层嵌套预算，输入未应用",
        )
    })?;
    if source.is_empty() || source.contains(['\n', '\r']) || !diagnostics.is_empty() {
        return Err(DialogueError::new(
            "INVALID_DRAFT",
            "插值需要单行、完整的正式表达式",
        ));
    }
    Ok(expression)
}
fn expr_equal(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Num(a), Expr::Num(b)) => a.to_bits() == b.to_bits(),
        (Expr::Str(a), Expr::Str(b)) => a == b,
        (Expr::Bool(a), Expr::Bool(b)) => a == b,
        (Expr::Var { name: a, .. }, Expr::Var { name: b, .. }) => a == b,
        (Expr::Unary { op: a, expr: x }, Expr::Unary { op: b, expr: y }) => {
            a == b && expr_equal(x, y)
        }
        (
            Expr::Binary {
                op: a,
                lhs: x,
                rhs: y,
            },
            Expr::Binary {
                op: b,
                lhs: p,
                rhs: q,
            },
        ) => a == b && expr_equal(x, p) && expr_equal(y, q),
        (
            Expr::Call {
                name: a, args: x, ..
            },
            Expr::Call {
                name: b, args: y, ..
            },
        ) => a == b && x.len() == y.len() && x.iter().zip(y).all(|(x, y)| expr_equal(x, y)),
        _ => false,
    }
}

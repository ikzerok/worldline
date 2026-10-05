use super::*;
use crate::{
    lexer::{Line, LineKind},
    parser::Parser,
    CompileOptions, Severity,
};
use std::collections::BTreeMap;

type Failure = (SourceOutlineStatus, String);
type Projection = (
    Vec<SourceOutlineEntry>,
    Vec<Range<usize>>,
    Vec<Range<usize>>,
);
fn budget(message: &str) -> Failure {
    (SourceOutlineStatus::BudgetExceeded, message.into())
}
fn unavailable() -> Failure {
    (
        SourceOutlineStatus::Unavailable,
        "无法证明当前声明的精确来源范围".into(),
    )
}

pub(super) fn build(
    file: &str,
    source: &str,
    options: CompileOptions,
) -> Result<Projection, Failure> {
    if source.len() > MAX_SOURCE_OUTLINE_BYTES {
        return Err(budget("当前文件超过 512 KiB 结构预算"));
    }
    let raw: Vec<&str> = source.split_inclusive('\n').collect();
    if raw.len() > MAX_SOURCE_OUTLINE_LINES {
        return Err(budget("当前文件超过 16384 行结构预算"));
    }
    if raw
        .iter()
        .any(|line| line.len() > MAX_SOURCE_OUTLINE_LINE_BYTES)
    {
        return Err(budget("当前文件单行超过 16384 字节结构预算"));
    }
    let comments = crate::lexer::comment_source_spans(source);
    if comments.iter().any(|comment| !comment.closed) {
        return Err((
            SourceOutlineStatus::SyntaxInvalid,
            "块注释未闭合，本文件结构暂不可用".into(),
        ));
    }
    check_expression_budget(source)?;
    let mut diagnostics = Vec::new();
    let all_lines = crate::lexer::lex_source_with_options(file, source, &mut diagnostics, options);
    if all_lines.len() > MAX_SOURCE_OUTLINE_LINES {
        return Err(budget("词法行超过结构预算"));
    }
    let mut indents = Vec::new();
    for line in &all_lines {
        while indents.last().is_some_and(|&indent| indent >= line.indent) {
            indents.pop();
        }
        indents.push(line.indent);
        if indents.len() > MAX_SOURCE_OUTLINE_DEPTH {
            return Err(budget("语法缩进超过 64 层结构预算"));
        }
    }
    let lines: Vec<Line> = all_lines
        .iter()
        .filter(|line| !matches!(line.kind, LineKind::Include { .. }) || line.indent != 0)
        .cloned()
        .collect();
    let program = Parser::new_with_options(&lines, &mut diagnostics, options).parse_program();
    if let Some(error) = diagnostics
        .iter()
        .find(|error| error.severity == Severity::Error)
    {
        return Err((
            SourceOutlineStatus::SyntaxInvalid,
            format!(
                "第 {} 行：{}；本文件结构暂不可用",
                error.span.line, error.message
            ),
        ));
    }
    let declarations = collect::declarations(&program, &lines);
    if declarations.len() > MAX_SOURCE_OUTLINE_ENTRIES {
        return Err(budget("当前文件超过 4096 个声明结构预算"));
    }
    let mut start = 0;
    let offsets: Vec<usize> = raw
        .iter()
        .map(|line| {
            let result = start;
            start += line.len();
            result
        })
        .collect();
    let mut statements = Vec::new();
    let mut by_line = BTreeMap::new();
    for line in &all_lines {
        let raw = *raw.get(line.no as usize - 1).ok_or_else(unavailable)?;
        let base = offsets[line.no as usize - 1];
        let begin = character_byte(raw, line.source.base as usize).ok_or_else(unavailable)?;
        let end = character_byte(raw, (line.source.base + line.source.length) as usize)
            .ok_or_else(unavailable)?;
        let range = base + begin..base + end;
        by_line.insert(line.no, range.clone());
        if !matches!(line.kind, LineKind::Include { .. }) {
            statements.push(range);
        }
    }
    let mut entries: Vec<SourceOutlineEntry> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for declaration in declarations {
        let header = by_line
            .get(&declaration.line)
            .ok_or_else(unavailable)?
            .clone();
        let end_line = program
            .source_provenance
            .block_ends
            .get(&(file.into(), declaration.line))
            .ok_or_else(unavailable)?;
        let end = by_line.get(end_line).ok_or_else(unavailable)?.end;
        if header.end > end || end > source.len() {
            return Err(unavailable());
        }
        while stack
            .last()
            .is_some_and(|&parent| entries[parent].body.end <= header.start)
        {
            stack.pop();
        }
        let parent = stack.last().copied();
        if let Some(parent) = parent {
            if end > entries[parent].body.end {
                return Err(unavailable());
            }
        }
        let occurrence = entries.len();
        entries.push(SourceOutlineEntry {
            occurrence,
            parent,
            depth: stack.len(),
            kind: declaration.kind.into(),
            id: declaration.id,
            display: declaration.display,
            entity_type: declaration.entity_type,
            line: declaration.line,
            body: header.start..end,
            header,
        });
        stack.push(occurrence);
    }
    Ok((
        entries,
        statements,
        comments.into_iter().map(|comment| comment.range).collect(),
    ))
}

fn character_byte(text: &str, column: usize) -> Option<usize> {
    text.char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.len()))
        .nth(column)
}

// Conservative resource guard, not syntax classification. Literal punctuation counts too:
// this bounds recursive expression/interpolation work before invoking the formal parser.
fn check_expression_budget(source: &str) -> Result<(), Failure> {
    let cleaned = crate::lexer::strip_comments(source);
    for line in cleaned.lines() {
        let mut depth = 0usize;
        let mut operators = 0;
        for ch in line.chars() {
            match ch {
                '(' | '[' | '{' => {
                    depth += 1;
                    operators += 1;
                }
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    operators += 1;
                }
                '+' | '-' | '*' | '/' | '%' | '!' => operators += 1,
                _ => {}
            }
            if depth > MAX_SOURCE_OUTLINE_DEPTH || operators > 256 {
                return Err(budget(
                    "单行分隔符/运算符超过结构安全预算（64 层或 256 个）",
                ));
            }
        }
        if line
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .filter(|word| *word == "not")
            .count()
            > 64
        {
            return Err(budget("单行 not 词超过 64 个结构安全预算"));
        }
    }
    Ok(())
}

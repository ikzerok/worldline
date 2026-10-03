//! 带外层引号的台词：字面正文只解码一次；表达式的引号解码有明确层次。
use super::*;

pub fn parse_quoted_interpolations_with_options(
    raw: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
    options: crate::CompileOptions,
) -> Vec<TextPart> {
    parse_with_ranges(
        raw,
        file,
        line,
        base_col,
        diags,
        options,
        &mut Vec::new(),
        &mut Vec::new(),
    )
}

pub(crate) fn parse_with_sources(
    raw: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
    options: crate::CompileOptions,
) -> (Vec<TextPart>, Vec<ExpressionSource>) {
    let mut sources = Vec::new();
    let parts = parse_with_ranges(
        raw,
        file,
        line,
        base_col,
        diags,
        options,
        &mut Vec::new(),
        &mut sources,
    );
    (parts, sources)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn parse_with_ranges(
    raw: &str,
    file: &str,
    line: u32,
    base_col: u32,
    diags: &mut Vec<Diagnostic>,
    options: crate::CompileOptions,
    ranges: &mut Vec<std::ops::Range<usize>>,
    sources: &mut Vec<ExpressionSource>,
) -> Vec<TextPart> {
    let raw_diagnostics_start = diags.len();
    let chars: Vec<char> = raw.chars().collect();
    let mut normalized = String::new();
    let mut positions = Vec::new();
    let mut i = 0;
    let mut inside = false;
    let mut quoted = false;
    let mut expression_escape = false;
    while i < chars.len() {
        let c = chars[i];
        if !inside {
            if c == '[' && chars.get(i + 1) == Some(&'[') {
                if let Some(end) = (i + 2..chars.len().saturating_sub(1))
                    .find(|&j| chars[j] == ']' && chars[j + 1] == ']')
                {
                    while i < end + 2 {
                        if chars[i] == '\\' && i + 1 < end {
                            let pair: String = chars[i..i + 2].iter().collect();
                            let decoded = lexer::decode_escapes(
                                &pair,
                                file,
                                Span::new(line, base_col + i as u32 + 1, 2),
                                diags,
                            );
                            for (offset, c) in decoded.chars().enumerate() {
                                normalized.push(c);
                                positions.push(i + offset.min(1));
                            }
                            i += 2;
                        } else {
                            normalized.push(chars[i]);
                            positions.push(i);
                            i += 1;
                        }
                    }
                    continue;
                }
            }
            normalized.push(c);
            positions.push(i);
            if c == '\\' && i + 1 < chars.len() {
                i += 1;
                normalized.push(chars[i]);
                positions.push(i);
            } else if c == '{' {
                inside = true;
                quoted = false;
                expression_escape = false;
            }
            i += 1;
            continue;
        }
        let (value, consumed) = if c == '\\' && i + 1 < chars.len() {
            let pair: String = chars[i..i + 2].iter().collect();
            (
                lexer::decode_escapes(
                    &pair,
                    file,
                    Span::new(line, base_col + i as u32 + 1, 2),
                    diags,
                ),
                2,
            )
        } else {
            (c.to_string(), 1)
        };
        for (offset, c) in value.chars().enumerate() {
            normalized.push(c);
            positions.push(i + offset.min(consumed - 1));
            if expression_escape {
                expression_escape = false;
            } else if quoted && c == '\\' {
                expression_escape = true;
            } else if c == '"' {
                quoted = !quoted;
            } else if !quoted && c == '}' {
                inside = false;
            }
        }
        i += consumed;
    }
    positions.push(chars.len());
    for diagnostic in &mut diags[raw_diagnostics_start..] {
        diagnostic.source_role = Some(DiagnosticSourceRole::Target);
    }
    let diagnostics_start = diags.len();
    let mut parts = super::text::parse_with_ranges(
        &normalized,
        file,
        line,
        base_col,
        diags,
        options,
        ranges,
        sources,
    );
    for source in sources.iter_mut() {
        source.map_boundaries(base_col, &positions);
    }
    for range in ranges.iter_mut() {
        range.start = positions[range.start];
        range.end = positions[range.end];
    }
    for part in &mut parts {
        match part {
            TextPart::Expr(expr) => remap_expr(expr, base_col, &positions),
            TextPart::Link(link) => {
                link.id_start = positions.get(link.id_start).copied().unwrap_or(chars.len());
                link.id_end = positions.get(link.id_end).copied().unwrap_or(chars.len());
                link.start = positions.get(link.start).copied().unwrap_or(chars.len());
                link.end = positions.get(link.end).copied().unwrap_or(chars.len());
                link.column = base_col + link.start as u32 + 1;
            }
            _ => {}
        }
    }
    for diagnostic in &mut diags[diagnostics_start..] {
        if diagnostic.span.line == line {
            crate::source_provenance::map_span(&mut diagnostic.span, base_col, &positions);
        }
    }
    parts
}
pub(super) fn remap_expr(expr: &mut Expr, base: u32, positions: &[usize]) {
    match expr {
        Expr::Var { loc, .. } => remap_loc(loc, base, positions),
        Expr::Call { loc, args, .. } => {
            remap_loc(loc, base, positions);
            for arg in args {
                remap_expr(arg, base, positions);
            }
        }
        Expr::Unary { expr, .. } => remap_expr(expr, base, positions),
        Expr::Binary { lhs, rhs, .. } => {
            remap_expr(lhs, base, positions);
            remap_expr(rhs, base, positions);
        }
        _ => {}
    }
}
fn remap_loc(loc: &mut Loc, base: u32, positions: &[usize]) {
    if loc.column > base {
        let offset = (loc.column - base - 1) as usize;
        loc.column = base
            + positions
                .get(offset)
                .copied()
                .unwrap_or_else(|| *positions.last().unwrap_or(&0)) as u32
            + 1;
    }
}

/// 已由正式词法器确定的静态字符串内部：花括号和链接外形是纯文字。
/// 复用字符串转义诊断，返回不含转义双字符的原字符范围。
pub(crate) fn static_literal_ranges(raw: &str) -> Result<Vec<std::ops::Range<usize>>, String> {
    let mut diagnostics = Vec::new();
    lexer::decode_escapes(
        raw,
        "search.wl",
        Span::new(1, 1, raw.chars().count() as u32),
        &mut diagnostics,
    );
    if !diagnostics.is_empty() {
        return Err("静态说明包含未完成转义".into());
    }
    let chars: Vec<_> = raw.chars().collect();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '\\' {
            at += 2;
            continue;
        }
        if let Some(last) = ranges.last_mut().filter(|r| r.end == at) {
            last.end += 1;
        } else {
            ranges.push(at..at + 1);
        }
        at += 1;
    }
    Ok(ranges)
}

/// 已解码 choice 标签的原稿边界映射；不再次解码或改变现有文本语义。
pub(crate) fn remap_parts(parts: &mut [TextPart], base: u32, positions: &[usize]) {
    for part in parts {
        match part {
            TextPart::Expr(expr) => remap_expr(expr, base, positions),
            TextPart::Link(link) => {
                link.start = positions[link.start];
                link.end = positions[link.end];
                link.id_start = positions[link.id_start];
                link.id_end = positions[link.id_end];
                link.column = base + link.start as u32 + 1;
            }
            _ => {}
        }
    }
}

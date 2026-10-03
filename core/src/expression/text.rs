use super::*;
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
                    diags.push(
                        Diagnostic::error(
                            "P003",
                            file,
                            Span::new(line, base_col + i as u32 + 1, 2),
                            format!("未知的转义 \\{other}"),
                        )
                        .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target),
                    );
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
                diags.push(
                    Diagnostic::error(
                        "P004",
                        file,
                        Span::new(line, base_col + i as u32 + 1, 2),
                        "正文对象链接未闭合",
                    )
                    .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target),
                );
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
                let id_start = i + 3 + target.kind.chars().count();
                let id_end = id_start + target.id.chars().count();
                parts.push(TextPart::Link(crate::navigation::InlineLink {
                    id_start,
                    id_end,
                    target,
                    label,
                    start: i,
                    end: end + 2,
                    column: base_col + i as u32 + 1,
                }));
            } else {
                diags.push(
                    Diagnostic::error(
                        "P004",
                        file,
                        Span::new(line, base_col + i as u32 + 1, (end + 2 - i) as u32),
                        "正文链接需要 [[对象类型:ID|显示文字]]，显示文字不可包含语法分隔符",
                    )
                    .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target),
                );
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
                diags.push(
                    Diagnostic::error(
                        "P003",
                        file,
                        Span::new(line, base_col + i as u32 + 1, 1),
                        "插值 `{` 未闭合",
                    )
                    .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target),
                );
                break;
            }
            let inner: String = chars[start..j].iter().collect();
            let (expr, source) =
                parse_expr_with_source(&inner, file, line, base_col + start as u32, diags);
            sources.push(source);
            parts.push(TextPart::Expr(expr));
            i = j + 1;
            continue;
        }
        if let Some(last) = ranges.last_mut().filter(|range| range.end == i) {
            last.end += 1;
        } else {
            ranges.push(i..i + 1);
        }
        lit.push(c);
        i += 1;
    }
    if !lit.is_empty() {
        parts.push(TextPart::Str(lit));
    }
    parts
}

pub(crate) fn literal_ranges(
    raw: &str,
    quoted: bool,
    options: crate::CompileOptions,
) -> Result<Vec<std::ops::Range<usize>>, String> {
    let mut ranges = Vec::new();
    let mut diagnostics = Vec::new();
    if quoted {
        super::quoted::parse_with_ranges(
            raw,
            "search.wl",
            1,
            0,
            &mut diagnostics,
            options,
            &mut ranges,
            &mut Vec::new(),
        );
    } else {
        parse_with_ranges(
            raw,
            "search.wl",
            1,
            0,
            &mut diagnostics,
            options,
            &mut ranges,
            &mut Vec::new(),
        );
    }
    if !diagnostics.is_empty() {
        return Err("正文含未完成 token，不能安全替换".into());
    }
    Ok(ranges)
}

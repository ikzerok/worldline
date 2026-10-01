use crate::lexer::{lex_source_with_options, LineKind};
use std::ops::Range;

pub(super) fn prose_ranges(source: &str, options: crate::CompileOptions) -> Vec<Range<usize>> {
    let mut diagnostics = Vec::new();
    let lines = lex_source_with_options("search.wl", source, &mut diagnostics, options);
    let physical: Vec<_> = source.split_inclusive('\n').collect();
    let mut offsets = vec![0];
    for line in &physical {
        offsets.push(offsets.last().unwrap() + line.len());
    }
    let mut ranges = Vec::new();
    for line in lines {
        if diagnostics.iter().any(|d| d.span.line == line.no) {
            continue;
        }
        let Some(raw) = physical.get(line.no.saturating_sub(1) as usize) else {
            continue;
        };
        let raw = raw.trim_end_matches(['\r', '\n']);
        let indent = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        let disabled = if let LineKind::Choice {
            disabled_span: Some(span),
            ..
        } = &line.kind
        {
            let positions: Vec<_> = raw
                .char_indices()
                .map(|(at, _)| at)
                .chain([raw.len()])
                .collect();
            let start = (line.indent + span.column.saturating_sub(1)) as usize;
            positions
                .get(start)
                .zip(positions.get(start + span.length as usize))
                .map(|(start, end)| (*start, *end))
        } else {
            None
        };
        let (start, text, quoted) = match line.kind {
            LineKind::Text { content, .. } => {
                let body = crate::parser::split_text_decorations(&content).0;
                let Some(at) = raw[indent..].find(&body) else {
                    continue;
                };
                (indent + at, body, false)
            }
            LineKind::Choice { .. } | LineKind::Description { .. } => {
                let Some(value) = quoted_body(raw) else {
                    continue;
                };
                value
            }
            LineKind::Language111 { keyword, .. } if keyword == "say" => {
                let Some(value) = quoted_body(raw) else {
                    continue;
                };
                value
            }
            _ => continue,
        };
        let Ok(literals) = crate::expression::literal_ranges(&text, quoted, options) else {
            continue;
        };
        let bytes: Vec<_> = text
            .char_indices()
            .map(|(at, _)| at)
            .chain([text.len()])
            .collect();
        for literal in literals {
            let start = offsets[line.no as usize - 1] + start;
            ranges.push(start + bytes[literal.start]..start + bytes[literal.end]);
        }
        if let Some((start, end)) = disabled {
            let text = &raw[start..end];
            if let Ok(literals) = crate::expression::static_literal_ranges(text) {
                let bytes: Vec<_> = text
                    .char_indices()
                    .map(|(at, _)| at)
                    .chain([text.len()])
                    .collect();
                for literal in literals {
                    let base = offsets[line.no as usize - 1] + start;
                    ranges.push(base + bytes[literal.start]..base + bytes[literal.end]);
                }
            }
        }
    }
    ranges
}
fn quoted_body(raw: &str) -> Option<(usize, String, bool)> {
    let chars: Vec<_> = raw.chars().collect();
    let start = chars.iter().position(|c| *c == '"')?;
    let (text, _) =
        crate::lexer::parse_quoted_raw(&chars, start, "search.wl", 1, &mut Vec::new()).ok()?;
    let byte = raw
        .char_indices()
        .nth(start + 1)
        .map(|(at, _)| at)
        .unwrap_or(raw.len());
    Some((byte, text, true))
}

pub(super) fn protected_signature(source: &str, options: crate::CompileOptions) -> String {
    let ranges = prose_ranges(source, options);
    let mut output = String::new();
    let mut previous = 0;
    for range in ranges {
        output.push_str(&source[previous..range.start]);
        previous = range.end;
    }
    output.push_str(&source[previous..]);
    output
}

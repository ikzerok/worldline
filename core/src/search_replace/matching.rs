use super::SearchOptions;
use std::ops::Range;

/// 字符级大小写比较保持原 UTF-8 范围，不以 lowercase 后的字节偏移改原稿。
pub fn literal_matches(text: &str, query: &str, options: SearchOptions) -> Vec<Range<usize>> {
    literal_match_iter(text, query, options).map(|hit| hit.range).collect()
}

pub(super) struct LiteralMatch {
    pub range: Range<usize>,
    pub line: u32,
    pub column: u32,
}

/// 逐处产出，行列随游标单次推进，不为每个同一长行命中重扫前缀。
pub(super) fn literal_match_iter<'a>(
    text: &'a str,
    query: &str,
    options: SearchOptions,
) -> impl Iterator<Item = LiteralMatch> + 'a {
    let chars: Vec<_> = if query.is_empty() { Vec::new() } else { text.char_indices().collect() };
    let needle: Vec<_> = query.chars().collect();
    let mut index = 0;
    let (mut line, mut column) = (1u32, 1u32);
    std::iter::from_fn(move || {
        if needle.is_empty() { return None; }
        while index + needle.len() <= chars.len() {
            let end = index + needle.len();
            let equal = chars[index..end].iter().zip(&needle).all(|((_, a), b)| {
                if options.case_sensitive { a == b } else { a.to_lowercase().eq(b.to_lowercase()) }
            });
            let word = |c: char| c.is_alphanumeric() || c == '_';
            let boundary = !options.whole_word
                || (index == 0 || !word(chars[index - 1].1))
                    && (end == chars.len() || !word(chars[end].1));
            let matched = equal && boundary;
            let next = if matched { end } else { index + 1 };
            let hit = matched.then(|| LiteralMatch {
                range: chars[index].0..chars.get(end).map(|(at, _)| *at).unwrap_or(text.len()),
                line, column,
            });
            for (_, c) in &chars[index..next] {
                if *c == '\n' { line = line.saturating_add(1); column = 1; }
                else { column = column.saturating_add(1); }
            }
            index = next;
            if hit.is_some() { return hit; }
        }
        None
    })
}

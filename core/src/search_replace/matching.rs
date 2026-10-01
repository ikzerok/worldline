use super::SearchOptions;
use std::ops::Range;

/// 字符级大小写比较保持原 UTF-8 范围，不以 lowercase 后的字节偏移改原稿。
pub fn literal_matches(text: &str, query: &str, options: SearchOptions) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let chars: Vec<_> = text.char_indices().collect();
    let needle: Vec<_> = query.chars().collect();
    let mut results = Vec::new();
    let mut index = 0;
    while index + needle.len() <= chars.len() {
        let end = index + needle.len();
        let equal = chars[index..end].iter().zip(&needle).all(|((_, a), b)| {
            if options.case_sensitive {
                a == b
            } else {
                a.to_lowercase().eq(b.to_lowercase())
            }
        });
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let boundary = !options.whole_word
            || (index == 0 || !word(chars[index - 1].1))
                && (end == chars.len() || !word(chars[end].1));
        if equal && boundary {
            results.push(chars[index].0..chars.get(end).map(|(at, _)| *at).unwrap_or(text.len()));
            index = end;
        } else {
            index += 1;
        }
    }
    results
}

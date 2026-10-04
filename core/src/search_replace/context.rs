use std::ops::Range;
use unicode_segmentation::GraphemeCursor;

const MAX_CHARS: usize = 160;
const LEADING_CHARS: usize = 48;

/// 围绕一处真实命中的有界原文窗口；最多 160 个 Unicode 标量、640 个 UTF-8 字节。
///
/// 不插入省略号、不改写换行，也不保证字素簇完整：组合字符与 ZWJ 序列可能被截断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchContext {
    /// 与 source_range 对应的连续原文字节。
    pub text: String,
    /// 在完整当前原文中的 UTF-8 字节半开范围。
    pub source_range: Range<usize>,
    /// 真实命中与窗口交集在 text 中的 UTF-8 字节半开范围；空替换可为零宽。
    pub highlight: Range<usize>,
    /// 窗口之前是否省略了当前语境范围内的原文。
    ///
    /// 无换行的命中以所在物理行内容为范围，不含 LF、CRLF 或单独 CR 终止符；
    /// 含 CR/LF 的命中以完整文档为范围。位于 CRLF 中间的零宽命中也以完整文档为范围。
    pub omitted_before: bool,
    /// 窗口之后是否省略原文；语境范围定义与 omitted_before 相同。
    pub omitted_after: bool,
    /// 真实命中的起点是否落在窗口之前。
    pub match_omitted_before: bool,
    /// 真实命中的终点是否落在窗口之后。
    pub match_omitted_after: bool,
    /// 窗口任一边缘是否截断完整原文中的扩展字素簇；不判断 highlight 的边缘。
    pub grapheme_clipped: bool,
}

/// 每份确切原文建立一次换行索引，供全部命中复用，避免逐命中重扫长行。
pub(super) struct ContextIndex<'a> {
    source: &'a str,
    line_breaks: Vec<usize>,
}

impl<'a> ContextIndex<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            source,
            line_breaks: source
                .bytes()
                .enumerate()
                .filter_map(|(at, byte)| matches!(byte, b'\r' | b'\n').then_some(at))
                .collect(),
        }
    }

    /// range 必须是原文内有效的 UTF-8 字节范围，包括有效的零宽范围。
    ///
    /// 通常最多保留命中前 48 个标量，优先完整显示不超过 160 个标量的命中；
    /// 更长命中显示其前缀。CRLF 尽量成对保留，只有完整命中占满窗口、无法同时容纳
    /// 相邻 CR/LF 时才允许窗口边缘拆开该对；不会因此丢弃本来可完整显示的命中。
    pub(super) fn context(&self, range: Range<usize>) -> SearchContext {
        let source = self.source;
        let matched = source
            .get(range.clone())
            .expect("命中范围必须对应有效原文字节");
        let next_break = self.line_breaks.partition_point(|at| *at < range.start);
        let multiline = self
            .line_breaks
            .get(next_break)
            .is_some_and(|at| *at < range.end)
            || splits_crlf(source, range.start);
        let span = if multiline {
            0..source.len()
        } else {
            let start = next_break
                .checked_sub(1)
                .map_or(0, |index| self.line_breaks[index] + 1);
            let end = self
                .line_breaks
                .get(next_break)
                .copied()
                .unwrap_or(source.len());
            start..end
        };
        // 只统计到上限后一位，不为长命中或长原文建立字符数组。
        let matched_chars = matched.chars().take(MAX_CHARS + 1).count();
        let (start, mut end) = if matched_chars > MAX_CHARS {
            let start = if splits_crlf(source, range.start) {
                range.start - 1
            } else {
                range.start
            };
            let end = advance(source, start..span.end, MAX_CHARS);
            (start, end)
        } else {
            // 把直接相邻的 CR/LF 纳入必需窗口，再分配可选的前后语境。
            let mut required = range.clone();
            let mut required_chars = matched_chars;
            if splits_crlf(source, required.start) && required_chars < MAX_CHARS {
                required.start -= 1;
                required_chars += 1;
            }
            if splits_crlf(source, required.end) && required_chars < MAX_CHARS {
                required.end += 1;
                required_chars += 1;
            }
            let leading = usize::from(required.start < range.start);
            let budget = (LEADING_CHARS - leading).min(MAX_CHARS - required_chars);
            let mut start = retreat(source, span.start..required.start, budget);
            if start < required.start && splits_crlf(source, start) {
                start += 1;
            }
            let mut end = advance(source, start..span.end, MAX_CHARS);
            if end > required.end && splits_crlf(source, end) {
                end -= 1;
            }
            (start, end)
        };
        // 长命中的截断点可以略微前移，避免仅显示 CRLF 的前半部分。
        if matched_chars > MAX_CHARS && splits_crlf(source, end) {
            end -= 1;
        }
        let highlight = range.start.max(start) - start..range.end.min(end) - start;
        SearchContext {
            text: source[start..end].into(),
            source_range: start..end,
            highlight,
            omitted_before: start > span.start,
            omitted_after: end < span.end,
            match_omitted_before: range.start < start,
            match_omitted_after: range.end > end,
            grapheme_clipped: !grapheme_boundary(source, start) || !grapheme_boundary(source, end),
        }
    }
}

#[cfg(test)]
fn match_context(source: &str, range: Range<usize>) -> SearchContext {
    ContextIndex::new(source).context(range)
}

fn grapheme_boundary(source: &str, at: usize) -> bool {
    GraphemeCursor::new(at, source.len(), true)
        .is_boundary(source, 0)
        .expect("完整原文必须能够判断字素边界")
}

fn splits_crlf(source: &str, at: usize) -> bool {
    at > 0 && source.as_bytes().get(at - 1..=at) == Some(b"\r\n")
}

fn advance(source: &str, range: Range<usize>, count: usize) -> usize {
    source[range.clone()]
        .char_indices()
        .nth(count)
        .map_or(range.end, |(at, _)| range.start + at)
}

fn retreat(source: &str, range: Range<usize>, count: usize) -> usize {
    if count == 0 {
        return range.end;
    }
    source[range.clone()]
        .char_indices()
        .rev()
        .nth(count - 1)
        .map_or(range.start, |(at, _)| range.start + at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str, matched: Range<usize>) -> SearchContext {
        let context = match_context(source, matched.clone());
        assert_eq!(context.text, source[context.source_range.clone()]);
        assert!(context.text.chars().count() <= MAX_CHARS);
        assert!(context.text.len() <= 640);
        assert!(context.text.is_char_boundary(context.highlight.start));
        assert!(context.text.is_char_boundary(context.highlight.end));
        assert!(context.highlight.start <= context.highlight.end);
        assert_eq!(
            &context.text[context.highlight.clone()],
            &source[matched.start.max(context.source_range.start)
                ..matched.end.min(context.source_range.end)]
        );
        assert_eq!(
            context.match_omitted_before,
            matched.start < context.source_range.start
        );
        assert_eq!(
            context.match_omitted_after,
            matched.end > context.source_range.end
        );
        context
    }

    #[test]
    fn long_line_centers_the_match_at_column_253() {
        let source = format!("{}目标{}", "a".repeat(252), "b".repeat(250));
        let context = check(&source, 252..252 + "目标".len());
        assert_eq!(context.source_range.start, 204);
        assert_eq!(&context.text[context.highlight.clone()], "目标");
        assert_eq!(context.highlight.start, LEADING_CHARS);
        assert_eq!(context.text.chars().count(), MAX_CHARS);
        assert!(context.omitted_before && context.omitted_after);
        assert!(!context.match_omitted_before && !context.match_omitted_after);
    }

    #[test]
    fn same_line_hits_have_distinct_precise_highlights() {
        let source = "prefix needle middle needle suffix";
        let first_at = source.find("needle").unwrap();
        let second_at = source.rfind("needle").unwrap();
        let first = check(source, first_at..first_at + 6);
        let second = check(source, second_at..second_at + 6);
        assert_eq!(first.text, second.text);
        assert_ne!(first.highlight, second.highlight);
        assert_eq!(second.highlight, second_at..second_at + 6);
        assert!(!first.omitted_before && !first.omitted_after);
    }

    #[test]
    fn unicode_scalars_preserve_exact_bytes_without_grapheme_claims() {
        let prefix = "🦀".repeat(80);
        let matched = "e\u{301}👩\u{200d}💻";
        let source = format!("{prefix}{matched}{}", "🦀".repeat(180));
        let context = check(&source, prefix.len()..prefix.len() + matched.len());
        assert_eq!(context.highlight.start, LEADING_CHARS * "🦀".len());
        assert_eq!(&context.text[context.highlight.clone()], matched);
        assert_eq!(context.text.chars().count(), MAX_CHARS);
        assert!(context.omitted_before && context.omitted_after);
        assert!(!context.grapheme_clipped);

        let source = format!("{}e\u{301}{}", "x".repeat(100), "y".repeat(200));
        let context = check(&source, 101..103);
        assert!(!context.grapheme_clipped);
        assert_eq!(&context.text[context.highlight], "\u{301}");
    }

    #[test]
    fn scalar_windows_explicitly_report_combining_and_zwj_clipping() {
        let source = format!(
            "{}e\u{301}{}M{}",
            "a".repeat(51),
            "b".repeat(47),
            "x".repeat(200)
        );
        let start = source.find('M').unwrap();
        let context = check(&source, start..start + 1);
        assert!(context.text.starts_with('\u{301}'));
        assert!(context.grapheme_clipped);
        assert!(!context.match_omitted_before && !context.match_omitted_after);

        let source = format!(
            "{}👩\u{200d}💻{}M{}",
            "a".repeat(50),
            "b".repeat(47),
            "x".repeat(200)
        );
        let start = source.find('M').unwrap();
        let context = check(&source, start..start + 1);
        assert!(context.text.starts_with('💻'));
        assert!(context.grapheme_clipped);

        let source = format!("M{}e\u{301}tail", "x".repeat(158));
        let context = check(&source, 0..1);
        assert!(context.text.ends_with('e'));
        assert!(context.grapheme_clipped);
    }

    #[test]
    fn oversized_combining_cluster_stays_bounded_and_flags_clipping() {
        let source = format!("e{}", "\u{301}".repeat(200));
        let context = check(&source, 0..source.len());
        assert_eq!(context.text.chars().count(), MAX_CHARS);
        assert_eq!(context.highlight, 0..context.text.len());
        assert!(context.grapheme_clipped);
        assert!(!context.match_omitted_before && context.match_omitted_after);
    }

    #[test]
    fn single_line_context_excludes_terminators_and_other_lines() {
        let source = "other\r\nleft needle right\r\nlast";
        let start = source.find("needle").unwrap();
        let context = check(source, start..start + 6);
        assert_eq!(context.text, "left needle right");
        assert_eq!(context.source_range, 7..24);
        assert!(!context.omitted_before && !context.omitted_after);

        let source = "first\rmiddle\rlast";
        let context = check(source, 6..12);
        assert_eq!(context.text, "middle");
        assert!(!context.omitted_before && !context.omitted_after);
    }

    #[test]
    fn multiline_context_retains_exact_crlf_and_document_omissions() {
        let prefix = "a".repeat(100);
        let matched = "first\r\nsecond";
        let source = format!("{prefix}{matched}{}", "z".repeat(200));
        let context = check(&source, prefix.len()..prefix.len() + matched.len());
        assert_eq!(&context.text[context.highlight.clone()], matched);
        assert!(context.omitted_before && context.omitted_after);
        assert!(!context.match_omitted_before && !context.match_omitted_after);

        let context = check("a\nb\nc", 1..4);
        assert_eq!(context.text, "a\nb\nc");
        assert!(!context.omitted_before && !context.omitted_after);
    }

    #[test]
    fn optional_context_edges_do_not_split_crlf() {
        let source = format!(
            "{}\r\n{}\n{}",
            "a".repeat(10),
            "b".repeat(47),
            "c".repeat(200)
        );
        let start = 59;
        let context = check(&source, start..start + 1);
        assert_eq!(context.source_range.start, 12);
        assert!(!splits_crlf(&source, context.source_range.start));
        assert!(!splits_crlf(&source, context.source_range.end));

        let source = format!("\n{}\r\nrest", "a".repeat(158));
        let context = check(&source, 0..1);
        assert_eq!(context.source_range.end, 159);
        assert!(!context.text.ends_with('\r'));
        assert!(context.omitted_after);
    }

    #[test]
    fn empty_replacement_has_a_precise_zero_width_highlight() {
        let context = check("before  after", 7..7);
        assert_eq!(context.highlight, 7..7);
        assert!(!context.match_omitted_before && !context.match_omitted_after);
        let context = check("", 0..0);
        assert!(context.text.is_empty());
        assert_eq!(context.highlight, 0..0);
        assert!(!context.omitted_before && !context.omitted_after);

        let context = check("first\r\nlast", 6..6);
        assert_eq!(context.text, "first\r\nlast");
        assert_eq!(context.highlight, 6..6);
        let context = check("first\r\nlast", 5..5);
        assert_eq!(context.text, "first");
        assert_eq!(context.highlight, 5..5);
        let context = check("first\r\n", 7..7);
        assert!(context.text.is_empty());
        assert_eq!(context.highlight, 0..0);
    }

    #[test]
    fn complete_match_takes_priority_over_leading_context() {
        let source = format!("{}{}tail", "p".repeat(100), "🦀".repeat(150));
        let context = check(&source, 100..700);
        assert_eq!(context.source_range.start, 90);
        assert_eq!(context.highlight, 10..610);
        assert!(!context.match_omitted_before && !context.match_omitted_after);

        let source = format!("pre{}tail", "🦀".repeat(160));
        let context = check(&source, 3..643);
        assert_eq!(context.text.len(), 640);
        assert_eq!(context.highlight, 0..640);
        assert_eq!(context.source_range, 3..643);
    }

    #[test]
    fn oversized_match_shows_its_prefix_and_reports_only_the_missing_tail() {
        let source = format!("pre{}tail", "🦀".repeat(200));
        let context = check(&source, 3..803);
        assert_eq!(context.source_range, 3..643);
        assert_eq!(context.highlight, 0..640);
        assert!(!context.match_omitted_before && context.match_omitted_after);
        assert!(context.omitted_before && context.omitted_after);

        let source = format!("{}\r\n{}", "a".repeat(159), "z".repeat(100));
        let context = check(&source, 0..source.len());
        assert_eq!(context.source_range, 0..159);
        assert!(context.match_omitted_after);
    }

    #[test]
    fn crlf_partners_are_included_when_the_match_budget_allows() {
        let context = check("left\r\nright", 5..6);
        assert_eq!(context.text, "left\r\nright");
        assert_eq!(&context.text[context.highlight.clone()], "\n");
        let context = check("left\r\nright", 4..5);
        assert_eq!(context.text, "left\r\nright");
        assert_eq!(&context.text[context.highlight], "\r");

        // 160 标量的完整命中无法再加入 CR；此处优先保留完整命中。
        let source = format!("\r\n{}", "a".repeat(159));
        let context = check(&source, 1..source.len());
        assert_eq!(context.source_range, 1..source.len());
        assert!(!context.match_omitted_before && !context.match_omitted_after);
        assert!(splits_crlf(&source, context.source_range.start));
        assert!(context.grapheme_clipped);
    }

    #[test]
    fn huge_sources_and_matches_produce_only_a_bounded_window() {
        let source = format!("{}目标{}", "🦀".repeat(250_000), "x".repeat(1_000_000));
        let start = 1_000_000;
        let context = check(&source, start..start + "目标".len());
        assert_eq!(&context.text[context.highlight.clone()], "目标");
        assert_eq!(context.text.chars().count(), MAX_CHARS);
        assert!(context.omitted_before && context.omitted_after);
        let context = check(&source, 0..source.len());
        assert_eq!(context.text.len(), 640);
        assert!(context.match_omitted_after);
    }

    #[test]
    fn one_index_serves_ten_thousand_same_line_hits() {
        let source = format!("prefix {}suffix", "hit ".repeat(10_000));
        let index = ContextIndex::new(&source);
        assert!(index.line_breaks.is_empty());
        let mut count = 0;
        for (at, matched) in source.match_indices("hit") {
            let context = index.context(at..at + matched.len());
            assert_eq!(&context.text[context.highlight.clone()], matched);
            assert_eq!(context.text, source[context.source_range.clone()]);
            assert!(context.text.chars().count() <= MAX_CHARS);
            assert!(!context.match_omitted_before && !context.match_omitted_after);
            count += 1;
        }
        assert_eq!(count, 10_000);
    }

    #[test]
    fn indexed_line_boundaries_match_physical_spans_at_every_valid_range() {
        let source = "甲\r\n乙\r丙\n丁\r\n";
        let index = ContextIndex::new(source);
        let boundaries: Vec<_> = source
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(source.len()))
            .collect();
        for start in &boundaries {
            for end in boundaries.iter().filter(|end| *end >= start) {
                let context = index.context(*start..*end);
                let span =
                    if source[*start..*end].contains(['\r', '\n']) || splits_crlf(source, *start) {
                        0..source.len()
                    } else {
                        let first = source[..*start].rfind(['\r', '\n']).map_or(0, |at| at + 1);
                        let last = source[*end..]
                            .find(['\r', '\n'])
                            .map_or(source.len(), |at| end + at);
                        first..last
                    };
                assert_eq!(context.source_range, span);
                assert!(!context.omitted_before && !context.omitted_after);
                assert!(!context.match_omitted_before && !context.match_omitted_after);
            }
        }
    }
}

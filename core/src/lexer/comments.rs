//! 注释原始范围与既有剥离文本共享一次词法扫描，避免编辑器猜测注释归属。
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct CommentSource {
    pub range: Range<usize>,
    pub closed: bool,
}

pub(super) fn scan(src: &str) -> (String, Vec<CommentSource>) {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.char_indices().peekable();
    let mut in_string = false;
    let mut comments = Vec::new();
    while let Some((at, c)) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => {
                    if let Some((_, next)) = chars.next() {
                        out.push(next);
                    }
                }
                '"' | '\n' => in_string = false,
                _ => {}
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == '/' && chars.peek().is_some_and(|(_, c)| *c == '/') {
            chars.next();
            out.push_str("  ");
            let mut end = src.len();
            for (index, value) in chars.by_ref() {
                if value == '\n' {
                    out.push('\n');
                    end = index;
                    break;
                }
                out.push(' ');
            }
            comments.push(CommentSource {
                range: at..end,
                closed: true,
            });
        } else if c == '/' && chars.peek().is_some_and(|(_, c)| *c == '*') {
            chars.next();
            out.push_str("  ");
            let mut end = src.len();
            let mut closed = false;
            while let Some((_, value)) = chars.next() {
                if value == '*' && chars.peek().is_some_and(|(_, c)| *c == '/') {
                    let (index, _) = chars.next().unwrap();
                    out.push_str("  ");
                    end = index + 1;
                    closed = true;
                    break;
                }
                out.push(if value == '\n' { '\n' } else { ' ' });
            }
            comments.push(CommentSource {
                range: at..end,
                closed,
            });
        } else {
            out.push(c);
        }
    }
    (out, comments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_scanner_preserves_unicode_columns_crlf_and_exact_comment_bytes() {
        let raw = "entity e as \"//🙂\" //中文🙂\r\n  /*内部🙂\r\n 注释*/ property a = 1\r\n";
        let (clean, comments) = scan(raw);
        assert_eq!(clean.chars().count(), raw.chars().count());
        assert_eq!(comments.len(), 2);
        assert_eq!(&raw[comments[0].range.clone()], "//中文🙂\r");
        assert_eq!(&raw[comments[1].range.clone()], "/*内部🙂\r\n 注释*/");
        assert!(comments.iter().all(|comment| comment.closed));
        assert!(clean.starts_with("entity e as \"//🙂\" "));
        assert!(clean.ends_with(" property a = 1\r\n"));
    }

    #[test]
    fn unclosed_block_is_distinct_from_a_line_comment_at_eof() {
        let (_, comments) = scan("// 首行\n/*未闭合🙂");
        assert!(comments[0].closed);
        assert!(!comments[1].closed);
        let (_, comments) = scan("// 无终止换行🙂");
        assert!(comments[0].closed);
    }
}

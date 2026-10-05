//! 单实体声明边界只从正式词法结果及共享注释范围取得。
use super::Failure;
use crate::lexer::{comment_source_spans, lex_source_with_options, LineKind};
use crate::CompileOptions;
use std::ops::Range;

pub(super) fn declaration(
    file: &str,
    source: &str,
    id: &str,
    line: u32,
    options: CompileOptions,
) -> Result<Range<usize>, Failure> {
    let lines = lex_source_with_options(file, source, &mut Vec::new(), options);
    let matches: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, value)| {
            value.indent == 0 && matches!(&value.kind, LineKind::Entity { name, .. } if name == id)
        })
        .collect();
    let [(index, header)] = matches.as_slice() else {
        return Err("无法唯一定位顶层 entity 声明，工程未修改".into());
    };
    if header.no != line {
        return Err("entity 编译来源与词法声明不一致，工程未修改".into());
    }
    let raw: Vec<_> = source.split_inclusive('\n').collect();
    let mut last = header.no as usize - 1;
    for member in lines
        .iter()
        .skip(index + 1)
        .take_while(|value| value.indent > 0)
    {
        if !matches!(
            member.kind,
            LineKind::Description { .. } | LineKind::Property { .. }
        ) {
            return Err("entity 含有不能安全移动的块内语法，工程未修改".into());
        }
        last = member.no as usize - 1;
    }
    let cleaned = crate::lexer::strip_comments(source);
    let clean: Vec<_> = cleaned.split_inclusive('\n').collect();
    // 只接受紧邻正文、显式缩进的纯注释尾行；独立空行/顶层注释始终留在原处。
    while raw.get(last + 1).is_some_and(|value| {
        !value.trim().is_empty()
            && value.starts_with(' ')
            && clean
                .get(last + 1)
                .is_some_and(|line| line.trim().is_empty())
    }) {
        last += 1;
    }
    let start = raw
        .iter()
        .take(header.no as usize - 1)
        .map(|line| line.len())
        .sum();
    let end = raw.iter().take(last + 1).map(|line| line.len()).sum();
    let range = start..end;
    for comment in comment_source_spans(source) {
        if comment.range.start >= range.end || comment.range.end <= range.start {
            continue;
        }
        if !comment.closed || comment.range.start < range.start || comment.range.end > range.end {
            return Err("块注释跨越 entity 声明边界，无法证明归属，工程未修改".into());
        }
        let line_start = source[..comment.range.start]
            .rfind('\n')
            .map_or(0, |at| at + 1);
        let prefix = &source[line_start..comment.range.start];
        if prefix.is_empty() {
            return Err("entity 范围含独立顶层注释，无法证明归属，工程未修改".into());
        }
    }
    Ok(range)
}

pub(super) fn insertion(source: &str, block: &str) -> Result<(String, usize), Failure> {
    if comment_source_spans(source)
        .iter()
        .any(|comment| !comment.closed)
    {
        return Err("目标源码有未闭合块注释，不能安全插入声明".into());
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let separator = if source.is_empty() || source.ends_with('\n') {
        ""
    } else {
        newline
    };
    let offset = source.len() + separator.len();
    Ok((format!("{separator}{block}"), offset))
}

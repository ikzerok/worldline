//! 完整源码先完成词法分类和注释剥离，再把正式表达式 token 范围映回原字节。
use crate::catalog::TargetRef;
use crate::lexer::LineKind;
use std::collections::BTreeMap;
use std::ops::Range;

pub(super) fn reference_ranges(
    source: &str,
    target: &TargetRef,
) -> BTreeMap<u32, Option<Range<usize>>> {
    let cleaned = crate::lexer::strip_comments(source);
    let physical: Vec<_> = cleaned.split_inclusive('\n').collect();
    let parsed = crate::lexer::lex_source_with_options(
        "rename.wl",
        source,
        &mut Vec::new(),
        crate::CompileOptions::v1_13(),
    );
    parsed
        .into_iter()
        .filter_map(|line| {
            let LineKind::Property { value_src, .. } = line.kind else {
                return None;
            };
            let range = (|| {
                let raw = physical.get(line.no as usize - 1)?;
                let equal = raw.find('=')?;
                let value_start =
                    equal + 1 + (raw[equal + 1..].len() - raw[equal + 1..].trim_start().len());
                let start_chars = raw[..value_start].chars().count();
                let token =
                    crate::expression::static_ref_id_range(&value_src, &target.kind, &target.id)?;
                Some(start_chars + token.start..start_chars + token.end)
            })();
            Some((line.no, range))
        })
        .collect()
}

pub(super) fn rewrite(line: &str, range: Range<usize>, new_id: &str) -> (String, usize) {
    let start = line
        .char_indices()
        .nth(range.start)
        .map_or(line.len(), |(at, _)| at);
    let end = line
        .char_indices()
        .nth(range.end)
        .map_or(line.len(), |(at, _)| at);
    let quoted = crate::authoring::quote(new_id);
    let mut rewritten = line.to_owned();
    rewritten.replace_range(start..end, &quoted[1..quoted.len() - 1]);
    (rewritten, 1)
}

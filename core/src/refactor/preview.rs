//! 预览与实际修改共用同一组原始字节编辑；不重新生成供显示的伪候选。
use serde::Serialize;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefactorByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefactorOccurrence {
    pub line: u32,
    pub field: Option<String>,
    pub before_range: RefactorByteRange,
    pub after_range: RefactorByteRange,
    pub before_token: String,
    pub after_token: String,
    pub before_context: String,
    pub after_context: String,
}

pub(super) struct Edit {
    pub range: Range<usize>,
    pub replacement: String,
    pub field: String,
}

pub(super) fn apply(
    source: &str,
    mut edits: Vec<Edit>,
) -> Result<(String, Vec<RefactorOccurrence>), String> {
    edits.sort_by_key(|edit| edit.range.start);
    let mut after = String::new();
    let mut cursor = 0;
    let mut occurrences = Vec::new();
    for edit in edits {
        if edit.range.start < cursor || edit.range.end < edit.range.start {
            return Err("重构身份范围重叠，整批未提交".into());
        }
        let prefix = source
            .get(cursor..edit.range.start)
            .ok_or("重构身份起点无效")?;
        let token = source.get(edit.range.clone()).ok_or("重构身份范围无效")?;
        after.push_str(prefix);
        let start = after.len();
        after.push_str(&edit.replacement);
        occurrences.push(RefactorOccurrence {
            line: source[..edit.range.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u32
                + 1,
            field: Some(edit.field),
            before_range: RefactorByteRange {
                start: edit.range.start,
                end: edit.range.end,
            },
            after_range: RefactorByteRange {
                start,
                end: after.len(),
            },
            before_token: token.into(),
            after_token: edit.replacement,
            before_context: context(source, edit.range.start, edit.range.end).into(),
            after_context: String::new(),
        });
        cursor = edit.range.end;
    }
    after.push_str(&source[cursor..]);
    for occurrence in &mut occurrences {
        occurrence.after_context = context(
            &after,
            occurrence.after_range.start,
            occurrence.after_range.end,
        )
        .into();
    }
    Ok((after, occurrences))
}

fn context(source: &str, start: usize, end: usize) -> &str {
    let from = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let to = source[end..]
        .find('\n')
        .map_or(source.len(), |index| end + index);
    &source[from..to]
}

/// 旧语言类型保持原改写语义；逐处提取旧/新身份的完整 token。
pub(super) fn legacy_edits(before: &str, after: &str, old: &str, new: &str) -> Vec<Edit> {
    let mut edits = Vec::new();
    let common = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let mut left = 0;
    let mut right = 0;
    while left < before.len() || right < after.len() {
        // 相同原文（包括新 ID 与相似 ID）先逐字符跳过；只在真实差异处定位身份。
        while let (Some(a), Some(b)) =
            (before[left..].chars().next(), after[right..].chars().next())
        {
            if a != b {
                break;
            }
            left += a.len_utf8();
            right += b.len_utf8();
        }
        if left == before.len() && right == after.len() {
            break;
        }
        if let (Some(start), Some(new_start)) =
            (left.checked_sub(common), right.checked_sub(common))
        {
            if before
                .get(start..)
                .is_some_and(|rest| rest.starts_with(old))
                && after
                    .get(new_start..)
                    .is_some_and(|rest| rest.starts_with(new))
                && edits
                    .last()
                    .is_none_or(|edit: &Edit| edit.range.end <= start)
            {
                edits.push(Edit {
                    range: start..start + old.len(),
                    replacement: new.into(),
                    field: "identity".into(),
                });
                left = start + old.len();
                right = new_start + new.len();
                continue;
            }
        }
        // 旧类型有字符串重编码时，保留旧候选并准确显示整条语法行的实际字节。
        return vec![Edit {
            range: 0..before.trim_end_matches(['\r', '\n']).len(),
            replacement: after.trim_end_matches(['\r', '\n']).into(),
            field: "source.syntax".into(),
        }];
    }
    edits
}

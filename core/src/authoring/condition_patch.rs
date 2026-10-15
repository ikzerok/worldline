//! 既有表单仅改条件时复用正式来源，避免重建事件/选择的其它原始字节。
use super::*;
use crate::condition_authoring::ConditionSlot;

fn expression(line: &Line, slot: ConditionSlot) -> Option<(&str, u32)> {
    match (&line.kind, slot) {
        (
            LineKind::Event {
                after_src: Some(source),
                ..
            },
            ConditionSlot::EventAfter,
        ) => Some((source, line.source.condition)),
        (
            LineKind::Choice {
                cond_src: Some(source),
                ..
            },
            ConditionSlot::ChoiceDisplay,
        ) => Some((source, line.source.condition)),
        (
            LineKind::Choice {
                enable_src: Some(source),
                ..
            },
            ConditionSlot::ChoiceEnable,
        ) => Some((source, line.source.enable)),
        _ => None,
    }
}
/// 同一文本的行起点和正式注释范围只扫描一次，批量槽定位不重复扫整份源码。
pub(super) struct SourceIndex<'a> {
    source: &'a str,
    lines: Vec<usize>,
    comments: Vec<crate::lexer::CommentSource>,
}
impl<'a> SourceIndex<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        let lines = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter_map(|(at, byte)| (byte == b'\n').then_some(at + 1)),
            )
            .collect();
        // 正式 scanner 按源码顺序返回互不重叠的注释，end 同样单调。
        let comments = crate::lexer::comment_source_spans(source);
        Self {
            source,
            lines,
            comments,
        }
    }
    /// 正式 lexer 的列/长度映射回原文，不能把剥离注释的中间串交给作者表单。
    pub(super) fn source(
        &self,
        line: &Line,
        slot: ConditionSlot,
    ) -> Result<Option<&'a str>, String> {
        if expression(line, slot).is_none() {
            return Ok(None);
        }
        let range = self.range(line, slot)?;
        self.source
            .get(range)
            .map(Some)
            .ok_or_else(|| "条件原文来源不完整".into())
    }
    fn range(&self, line: &Line, slot: ConditionSlot) -> Result<Range<usize>, String> {
        let (expression, column) = expression(line, slot).ok_or("条件槽不存在，不能做局部改写")?;
        let number = line.no.checked_sub(1).ok_or("条件行不存在")? as usize;
        let offset = *self.lines.get(number).ok_or("条件行不存在")?;
        let end = self
            .lines
            .get(number + 1)
            .copied()
            .unwrap_or(self.source.len());
        let raw = self
            .source
            .get(offset..end)
            .ok_or("条件行来源不完整")?
            .trim_end_matches('\n');
        let byte = |column: usize| {
            raw.char_indices()
                .map(|(at, _)| at)
                .chain(std::iter::once(raw.len()))
                .nth(column)
        };
        let column = (line.source.base as usize)
            .checked_add(column as usize)
            .ok_or("条件起点溢出")?;
        let start = byte(column).ok_or("条件起点不完整")?;
        let finish = column
            .checked_add(expression.chars().count())
            .ok_or("条件终点溢出")?;
        let end = byte(finish).ok_or("条件终点不完整")?;
        let range = offset + start..offset + end;
        let next = self
            .comments
            .partition_point(|comment| comment.range.end <= range.start);
        // 普通高级源码字段可显式改写完整表达式（含完整闭合注释）；typed API 另拒绝 Advanced。
        // 部分注释意味着来源范围不可信，仍须报错，不能回退整块 serializer。
        if self
            .comments
            .iter()
            .skip(next)
            .take_while(|comment| comment.range.start < range.end)
            .any(|comment| {
                !comment.closed
                    || comment.range.start < range.start
                    || comment.range.end > range.end
            })
        {
            return Err("条件来源截断注释边界，不能做局部改写".into());
        }
        Ok(range)
    }
}
fn parsed(source: &str, file: &Path) -> Vec<Line> {
    crate::lexer::lex_source_with_options(
        &file.to_string_lossy(),
        source,
        &mut Vec::new(),
        crate::CompileOptions::v1_12().with_localization_ids(true),
    )
}
fn apply(source: &str, mut patches: Vec<(Range<usize>, String)>) -> Result<String, String> {
    patches.sort_by_key(|patch| patch.0.start);
    let mut result = String::with_capacity(source.len());
    let mut cursor = 0;
    for (range, replacement) in patches {
        if range.start < cursor
            || range.start > range.end
            || !source.is_char_boundary(range.start)
            || !source.is_char_boundary(range.end)
        {
            return Err("条件补丁来源重叠、越界或不是完整 UTF-8 边界".into());
        }
        result.push_str(source.get(cursor..range.start).ok_or("条件补丁来源越界")?);
        result.push_str(&replacement);
        cursor = range.end;
    }
    result.push_str(source.get(cursor..).ok_or("条件补丁终点越界")?);
    Ok(result)
}

pub(super) fn choice(
    draft: &mut EventDraft,
    number: u32,
    next: &ChoiceDraft,
) -> Result<bool, String> {
    let Some(old) = draft
        .try_choices()?
        .into_iter()
        .find(|choice| choice.line == number)
    else {
        return Ok(false);
    };
    let mut compare = old.clone();
    compare.condition = next.condition.clone();
    compare.enable_condition = next.enable_condition.clone();
    if compare != *next {
        return Ok(false);
    }
    let lines = parsed(&draft.body, Path::new("event.wl"));
    let line = lines
        .iter()
        .find(|line| line.no == number)
        .ok_or("选择行已改变")?;
    let source_index = SourceIndex::new(&draft.body);
    let mut patches = Vec::new();
    for (slot, before, after) in [
        (
            ConditionSlot::ChoiceDisplay,
            &old.condition,
            &next.condition,
        ),
        (
            ConditionSlot::ChoiceEnable,
            &old.enable_condition,
            &next.enable_condition,
        ),
    ] {
        if before == after {
            continue;
        }
        if before.trim().is_empty() || after.trim().is_empty() {
            return Ok(false);
        }
        patches.push((source_index.range(line, slot)?, after.clone()));
    }
    draft.body = apply(&draft.body, patches)?;
    Ok(true)
}

pub(super) fn event(
    project: &Project,
    path: &Path,
    id: &str,
    next: &EventDraft,
    compiled: &crate::CompileResult,
) -> Result<Option<String>, String> {
    let (old_path, old) = project.event_draft_from(id, compiled)?;
    if old_path != path {
        return Ok(None);
    }
    let mut compare = old.clone();
    compare.after = next.after.clone();
    compare.body = next.body.clone();
    if compare != *next {
        return Ok(None);
    }
    let raw = project.document(path)?;
    if old.after == next.after && old.body == next.body {
        return Ok(Some(raw.to_owned()));
    }
    let lexical = parsed(raw, path);
    let Some(index) = lexical
        .iter()
        .position(|l| matches!(&l.kind, LineKind::Event { name, .. } if name == id))
    else {
        return Ok(None);
    };
    let block = block_at(raw, &lexical, index);
    let old_lines = parsed(&old.body, Path::new("event.wl"));
    let new_lines = parsed(&next.body, Path::new("event.wl"));
    let old_choices: Vec<_> = old_lines
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Choice { .. }))
        .collect();
    let new_choices: Vec<_> = new_lines
        .iter()
        .filter(|l| matches!(l.kind, LineKind::Choice { .. }))
        .collect();
    let raw_choices: Vec<_> = lexical
        .iter()
        .skip(index + 1)
        .take_while(|l| l.indent as usize > block.indent)
        .filter(|l| matches!(l.kind, LineKind::Choice { .. }))
        .collect();
    if old_choices.len() != new_choices.len() || old_choices.len() != raw_choices.len() {
        return Ok(None);
    }
    let raw_index = SourceIndex::new(raw);
    let draft_index = SourceIndex::new(&old.body);
    let next_index = SourceIndex::new(&next.body);
    let mut raw_patches = Vec::new();
    let mut draft_patches = Vec::new();
    for ((old_line, new_line), raw_line) in
        old_choices.into_iter().zip(new_choices).zip(raw_choices)
    {
        if old_line.no != new_line.no {
            return Ok(None);
        }
        for slot in [ConditionSlot::ChoiceDisplay, ConditionSlot::ChoiceEnable] {
            let before = draft_index.source(old_line, slot)?;
            let after = next_index.source(new_line, slot)?;
            if before == after {
                continue;
            }
            let (Some(before), Some(after)) = (before, after) else {
                return Ok(None);
            };
            if before.is_empty() || after.is_empty() {
                return Ok(None);
            }
            draft_patches.push((draft_index.range(old_line, slot)?, after.to_owned()));
            raw_patches.push((raw_index.range(raw_line, slot)?, after.to_owned()));
        }
    }
    if apply(&old.body, draft_patches)? != next.body {
        return Ok(None);
    }
    if old.after != next.after {
        if old.after.is_empty() || next.after.is_empty() {
            return Ok(None);
        }
        raw_patches.push((
            raw_index.range(&lexical[index], ConditionSlot::EventAfter)?,
            next.after.clone(),
        ));
    }
    project.ensure_workspace_writable()?;
    let inventory = crate::source_lifecycle::safety::inventory(project).map_err(|e| e.message)?;
    project
        .source_disk_baselines_match_inventory(&inventory)
        .map_err(|e| e.message)?;
    crate::source_lifecycle::safety::writable_path(path)?;
    Ok(Some(apply(raw, raw_patches)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_patch_assembly_uses_original_utf8_ranges_and_rejects_overlap() {
        let source = "甲abc🙂xyz\r\n";
        assert_eq!(
            apply(source, vec![(10..13, "Z".into()), (3..6, "ABCDE".into())]).unwrap(),
            "甲ABCDE🙂Z\r\n"
        );
        assert_eq!(apply(source, Vec::new()).unwrap(), source);
        for patches in [
            vec![(3..10, "a".into()), (6..13, "b".into())],
            vec![(3..7, "a".into())],
            vec![(Range { start: 10, end: 3 }, "a".into())],
            vec![(3..99, "a".into())],
        ] {
            assert!(apply(source, patches).is_err());
        }
    }

    #[test]
    fn indexed_formal_comments_distinguish_expression_overlap_from_quoted_text() {
        let source = "// 前置🙂\r\nevent start after ready /*内部🙂*/ and true\r\n  choice \"/*引号🙂*/\" if ready // 尾注释\r\n    -> END\r\n";
        let index = SourceIndex::new(source);
        let lines = parsed(source, Path::new("event.wl"));
        let event = lines
            .iter()
            .find(|line| matches!(line.kind, LineKind::Event { .. }))
            .unwrap();
        let choice = lines
            .iter()
            .find(|line| matches!(line.kind, LineKind::Choice { .. }))
            .unwrap();
        assert_eq!(
            index.source(event, ConditionSlot::EventAfter).unwrap(),
            Some("ready /*内部🙂*/ and true")
        );
        let mut partial = event.clone();
        partial.source.condition += "ready /*".chars().count() as u32;
        if let LineKind::Event { after_src, .. } = &mut partial.kind {
            *after_src = Some("x".into());
        }
        assert!(index.range(&partial, ConditionSlot::EventAfter).is_err());
        assert_eq!(
            &source[index.range(choice, ConditionSlot::ChoiceDisplay).unwrap()],
            "ready"
        );
    }
}

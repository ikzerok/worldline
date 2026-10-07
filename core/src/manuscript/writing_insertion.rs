//! 正式空 event 的受保护输入槽，不把不存在的文本伪装为 WritingBlock。
use super::writing::WritingBuffer;
use crate::ast::{DivertTarget, Stmt};
use crate::catalog::TargetRef;
use crate::lexer::{Line, LineKind};
use crate::project::Project;
use std::{ops::Range, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritingProseInsertion {
    target: TargetRef,
    path: PathBuf,
    baseline: String,
    language_version: String,
    refresh_generation: u64,
    generation: u64,
    source_signature: String,
    range: Range<usize>,
    expected: String,
    text: String,
    indent: String,
    newline: String,
    append_newline: bool,
}
impl WritingProseInsertion {
    /// 第一段的正文字符起点；可用作与下一次 Prose 投影一致的控件身份。
    pub fn offset(&self) -> usize {
        self.range.start + self.indent.len()
    }

    /// 当前可编辑的语义空白；源自真实空行，不由界面猜测缩进。
    pub fn text(&self) -> &str {
        &self.text
    }
}

pub(super) fn project_slot(
    project: &Project,
    buffer: &WritingBuffer,
    target: &TargetRef,
    content: &crate::CompileResult,
    lines: &[Line],
) -> Option<WritingProseInsertion> {
    if target.kind != "event" || content.has_errors() || guard(project, buffer).is_err() {
        return None;
    }
    let index = content.program.event_index(&target.id)?;
    let event = &content.program.events[index];
    let [Stmt::Divert(divert)] = event.body.as_slice() else {
        return None;
    };
    if divert.target != DivertTarget::End || divert.drift || !event.effects.is_empty() {
        return None;
    }
    let header = lines.iter().position(|line| {
        line.no == event.loc.line
            && matches!(&line.kind, LineKind::Event { name, .. } if name == &target.id)
    })?;
    let body: Vec<_> = lines
        .iter()
        .skip(header + 1)
        .take_while(|line| line.indent > lines[header].indent)
        .collect();
    let [terminal] = body.as_slice() else {
        return None;
    };
    if terminal.no != divert.loc.line
        || !matches!(&terminal.kind, LineKind::Divert { target, drift: false, .. } if target == "END")
    {
        return None;
    }
    let physical: Vec<_> = buffer.source().split_inclusive('\n').collect();
    let line_index = terminal.no.checked_sub(1)? as usize;
    let raw = *physical.get(line_index)?;
    let indent = &raw[..raw.len() - raw.trim_start_matches([' ', '\t']).len()];
    let mut start = physical[..line_index]
        .iter()
        .map(|line| line.len())
        .sum::<usize>();
    let newline = if raw.ends_with("\r\n") {
        "\r\n"
    } else if raw.ends_with('\n') {
        "\n"
    } else {
        physical[..line_index]
            .iter()
            .rev()
            .find_map(|line| {
                if line.ends_with("\r\n") {
                    Some("\r\n")
                } else if line.ends_with('\n') {
                    Some("\n")
                } else {
                    None
                }
            })
            .unwrap_or("\n")
    };
    let terminal_start = start;
    let mut range_end = start;
    let mut append_newline = true;
    // 清空普通 Prose 后留下一行缩进；复用它，控件起点不跳到下一行。
    let body_start = physical[..event.loc.line as usize]
        .iter()
        .map(|line| line.len())
        .sum::<usize>();
    for previous in physical[..line_index].iter().rev() {
        let content = previous.trim_end_matches(['\r', '\n']);
        if !content.trim().is_empty() || !content.starts_with(indent) || !previous.ends_with('\n') {
            break;
        }
        let previous_start = start - previous.len();
        if previous_start < body_start {
            break;
        }
        start = previous_start;
        if append_newline {
            range_end = terminal_start - (previous.len() - content.len());
        }
        append_newline = false;
    }
    // 不在横跨本行的块注释内部插入，也不让未闭合注释吞掉新输入。
    if crate::lexer::comment_source_spans(buffer.source())
        .iter()
        .any(|comment| {
            !comment.closed || (comment.range.start < start && comment.range.end > start)
        })
    {
        return None;
    }
    let expected = &buffer.source()[start..range_end];
    let text = expected
        .replace("\r\n", "\n")
        .split('\n')
        .map(|line| line.strip_prefix(indent).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    Some(WritingProseInsertion {
        target: target.clone(),
        path: buffer.path().to_owned(),
        baseline: buffer.baseline().into(),
        language_version: project.language_version().into(),
        refresh_generation: project.search_refresh_generation(),
        generation: buffer.generation(),
        source_signature: crate::presentation_commands::document_hash(buffer.source().as_bytes()),
        range: start..range_end,
        expected: expected.into(),
        text,
        indent: indent.into(),
        newline: newline.into(),
        append_newline,
    })
}

impl Project {
    /// 只修改唯一正文缓冲；空输入无副作用，无效新稿也原样保留。
    pub fn insert_writing_prose(
        &self,
        buffer: &mut WritingBuffer,
        slot: &WritingProseInsertion,
        text: &str,
    ) -> Result<(), String> {
        guard(self, buffer)?;
        self.verify_review_navigation()?;
        crate::source_lifecycle::safety::writable_path(buffer.path())?;
        let projection = self.project_writing_buffer(buffer, &slot.target)?;
        if projection.empty_prose_slot.as_ref() != Some(slot) {
            return Err("空正文位置已过期，输入未写入；请保留输入并重新查看当前草稿".into());
        }
        if text == slot.text() {
            return Ok(());
        }
        let normalized = text.replace("\r\n", "\n");
        let body = normalized
            .split('\n')
            .collect::<Vec<_>>()
            .join(&format!("{}{}", slot.newline, slot.indent));
        let replacement = format!(
            "{}{body}{}",
            slot.indent,
            if slot.append_newline {
                &slot.newline
            } else {
                ""
            }
        );
        buffer.replace_range(
            slot.generation,
            slot.range.clone(),
            &slot.expected,
            &replacement,
        )
    }
}

fn guard(project: &Project, buffer: &WritingBuffer) -> Result<(), String> {
    project.ensure_workspace_writable()?;
    if buffer.baseline() != project.content_baseline()
        || project.document(buffer.path())? != buffer.original()
    {
        return Err("正文草稿基线已过期；输入已保留，请核对工程变化".into());
    }
    if project
        .source_selection()
        .is_some_and(|set| !set.is_active(buffer.path()))
    {
        return Err("归档或非活动来源没有可编辑空正文槽".into());
    }
    crate::file_access::within(&project.root, buffer.path())?;
    Ok(())
}

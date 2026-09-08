//! 分支表单与正文之间的无损局部改写；UI 不解释语言。
use super::{block_at, comments, header_comment, lines, quote, EventDraft};
use crate::lexer::LineKind;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChoiceDraft {
    pub line: u32,
    pub depth: u32,
    pub label: String,
    pub once: bool,
    pub condition: String,
    /// 分支执行的正文和动作，包含内层条件/选择，移除末尾直接出口。
    pub body: String,
    /// 空值表示执行完后回到选择组之后。
    pub target: Option<String>,
    pub drift: bool,
}

impl EventDraft {
    pub fn choices(&self) -> Vec<ChoiceDraft> {
        let parsed = lines(&self.body, Path::new("event.wl"));
        parsed
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                let LineKind::Choice {
                    label_raw,
                    once,
                    cond_src,
                    ..
                } = &line.kind
                else {
                    return None;
                };
                let block = block_at(&self.body, &parsed, i);
                let mut body = self.body[block.header_end..block.range.end].to_owned();
                let mut target = None;
                let mut drift = false;
                let terminal = parsed
                    .iter()
                    .skip(i + 1)
                    .take_while(|l| l.indent > line.indent)
                    .last();
                if let Some(end) = terminal.filter(|l| l.indent as usize == block.body_indent) {
                    if let LineKind::Divert {
                        target: to,
                        drift: is_drift,
                        ..
                    } = &end.kind
                    {
                        let offset = self
                            .body
                            .split_inclusive('\n')
                            .take(end.no as usize - 1)
                            .map(str::len)
                            .sum::<usize>()
                            - block.header_end;
                        let len = body[offset..]
                            .find('\n')
                            .map_or(body.len() - offset, |n| n + 1);
                        let comment = comments(&body[offset..offset + len]);
                        body.replace_range(
                            offset..offset + len,
                            &format!("{}{}", " ".repeat(block.body_indent), comment),
                        );
                        target = Some(to.clone());
                        drift = *is_drift;
                    }
                }
                let body = body
                    .lines()
                    .map(|l| {
                        let spaces = l.bytes().take_while(|b| *b == b' ').count();
                        &l[spaces.min(block.body_indent)..]
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim_end()
                    .to_owned();
                Some(ChoiceDraft {
                    line: line.no,
                    depth: line.indent,
                    label: label_raw.clone(),
                    once: *once,
                    condition: cond_src.clone().unwrap_or_default(),
                    body,
                    target,
                    drift,
                })
            })
            .collect()
    }

    /// line=None 新增到第一个顶层选择组；已有分支按当前正文行定位。
    pub fn write_choice(&mut self, line: Option<u32>, choice: &ChoiceDraft) -> Result<(), String> {
        if choice.label.contains(['\n', '\r']) || choice.condition.contains(['\n', '\r']) {
            return Err("选择文案与显示条件须为单行".into());
        }
        if let Some(target) = &choice.target {
            if target != "END" {
                super::qualified(target)?;
            }
            if target == "END" && choice.drift {
                return Err("结束故事不能使用漂流".into());
            }
        }
        let parsed = lines(&self.body, Path::new("event.wl"));
        let existing = line
            .map(|line| {
                parsed
                    .iter()
                    .position(|l| l.no == line && matches!(l.kind, LineKind::Choice { .. }))
                    .ok_or("选择源位置已改变，请重新打开")
            })
            .transpose()?;
        let block = existing.map(|i| block_at(&self.body, &parsed, i));
        let indent = block.as_ref().map_or(0, |b| b.indent);
        let padding = " ".repeat(indent);
        let body_padding = " ".repeat(block.as_ref().map_or(2, |b| b.body_indent));
        let suffix = block
            .as_ref()
            .map(|b| header_comment(&self.body[b.range.start..b.header_end]))
            .unwrap_or("");
        let mut text = format!(
            "{padding}choice {}{}{}{suffix}\n",
            if choice.once { "once " } else { "" },
            quote(&choice.label),
            if choice.condition.trim().is_empty() {
                String::new()
            } else {
                format!(" if {}", choice.condition.trim())
            }
        );
        for line in choice.body.trim_end().lines() {
            text.push_str(&format!("{body_padding}{line}\n"));
        }
        if let Some(target) = &choice.target {
            text.push_str(&format!(
                "{body_padding}{} {target}\n",
                if choice.drift { "->>" } else { "->" }
            ));
        }
        if let Some(block) = block {
            self.body.replace_range(block.range, &text);
        } else {
            let index = parsed.iter().position(|l| {
                l.indent == 0 && matches!(l.kind, LineKind::Choice { .. } | LineKind::Divert { .. })
            });
            let offset = index
                .map(|i| {
                    self.body
                        .split_inclusive('\n')
                        .take(parsed[i].no as usize - 1)
                        .map(str::len)
                        .sum()
                })
                .unwrap_or(self.body.len());
            if offset > 0 && !self.body[..offset].ends_with('\n') {
                text.insert(0, '\n');
            }
            self.body.insert_str(offset, &text);
        }
        Ok(())
    }

    pub fn remove_choice(&mut self, line: u32) -> Result<(), String> {
        let parsed = lines(&self.body, Path::new("event.wl"));
        let i = parsed
            .iter()
            .position(|l| l.no == line && matches!(l.kind, LineKind::Choice { .. }))
            .ok_or("选择源位置不存在")?;
        let block = block_at(&self.body, &parsed, i);
        let retained = comments(&self.body[block.range.clone()]);
        self.body.replace_range(block.range, &retained);
        Ok(())
    }
}

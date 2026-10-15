use super::*;
use crate::source_provenance::{SourceOwner, StatementKind};
use crate::{
    ast::{Event, Stmt},
    lexer::LineKind,
    CompileResult,
};
use std::path::Path;

pub(super) fn event<'a>(
    compiled: &'a CompileResult,
    owner: &ConditionOwner,
) -> Result<(&'a Event, &'a str), String> {
    let mut found = compiled
        .program
        .events
        .iter()
        .zip(&compiled.program.event_files)
        .filter(|(event, _)| event.name == owner.target.id);
    let value = found.next().ok_or("条件所属事件不存在，请重新打开")?;
    if found.next().is_some() {
        return Err("条件所属事件身份重复；不能猜测编辑目标".into());
    }
    Ok((value.0, value.1))
}
pub(super) fn locate(
    compiled: &CompileResult,
    selector: &ConditionSelector,
) -> Result<(String, Option<ConditionSource>), String> {
    let (event, event_file) = event(compiled, &selector.owner)?;
    let (file, line_no) = if selector.slot == ConditionSlot::EventAfter {
        (event_file, event.loc.line)
    } else {
        let requested = selector.owner.choice_line.ok_or("缺少选择物理行号")?;
        let owner = SourceOwner::new(event_file, event.loc.line);
        let mut found = Vec::new();
        let mut blocks = vec![event.body.as_slice()];
        while let Some(block) = blocks.pop() {
            for statement in block {
                match statement {
                    Stmt::Choice(choice) => {
                        if choice.loc.line == requested {
                            let file = compiled
                                .program
                                .source_provenance
                                .statement_file(&owner, choice.loc, StatementKind::Choice)
                                .ok_or("选择来源文件不完整或有歧义")?;
                            found.push((file, choice.loc.line));
                        }
                        blocks.push(&choice.body);
                    }
                    Stmt::Scene(scene) => blocks.push(&scene.body),
                    Stmt::If(branch) => {
                        for (_, body) in &branch.branches {
                            blocks.push(body);
                        }
                    }
                    _ => {}
                }
            }
        }
        if found.len() != 1 {
            return Err("选择不属于指定事件、已改变或来源不唯一".into());
        }
        found[0]
    };
    let source = compiled
        .sources
        .get(Path::new(file))
        .ok_or("条件源码未在同一活动快照中")?;
    let mut diagnostics = Vec::new();
    let (lines, _) = crate::expression::condition::scoped(|| {
        crate::lexer::lex_source_with_options(file, source, &mut diagnostics, compiled.options)
    });
    let line = lines
        .iter()
        .find(|line| line.no == line_no)
        .ok_or("条件正式词法来源缺失")?;
    let (expression, offset) = match (&line.kind, selector.slot) {
        (
            LineKind::Event {
                name, after_src, ..
            },
            ConditionSlot::EventAfter,
        ) if name == &event.name => (after_src, line.source.condition),
        (LineKind::Choice { cond_src, .. }, ConditionSlot::ChoiceDisplay) => {
            (cond_src, line.source.condition)
        }
        (LineKind::Choice { enable_src, .. }, ConditionSlot::ChoiceEnable) => {
            (enable_src, line.source.enable)
        }
        _ => return Err("条件正式词法槽与 owner 不一致".into()),
    };
    let Some(expression) = expression else {
        return Ok((String::new(), None));
    };
    let start_column = line.source.base + offset;
    let (start, raw_line) = physical_line(source, line_no)?;
    let byte = |column: usize| {
        raw_line
            .char_indices()
            .nth(column)
            .map(|(at, _)| at)
            .or_else(|| (column == raw_line.chars().count()).then_some(raw_line.len()))
    };
    let begin = byte(start_column as usize).ok_or("条件起点不在物理源边界")?;
    let end =
        byte(start_column as usize + expression.chars().count()).ok_or("条件终点不在物理源边界")?;
    let byte_range = start + begin..start + end;
    Ok((
        source[byte_range.clone()].to_owned(),
        Some(ConditionSource {
            file: file.into(),
            line: line_no,
            column: start_column + 1,
            byte_range,
        }),
    ))
}
fn physical_line(source: &str, number: u32) -> Result<(usize, &str), String> {
    let mut offset = 0;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        if index + 1 == number as usize {
            return Ok((offset, line.trim_end_matches(['\r', '\n'])));
        }
        offset += line.len();
    }
    Err("条件物理行已不存在".into())
}

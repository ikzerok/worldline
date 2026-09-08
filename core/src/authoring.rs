//! 图形创作的源码修改接口。UI 传递字段,由 core 定位、生成并验证文本。
mod choices;
use crate::ast::{ChangeKind, EffectWhen, PropertyValue};
use crate::lexer::{lex_source, valid_identifier, Line, LineKind};
use crate::project::Project;
use crate::{compile_sources, Severity};
pub use choices::ChoiceDraft;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub struct EventDraft {
    pub id: String,
    pub summary: String,
    pub storyline: String,
    pub characters: Vec<String>,
    pub order: Option<u32>,
    pub period: Option<String>,
    pub predecessors: Vec<String>,
    pub perm: String,
    pub after: String,
    pub effects: Vec<EffectDraft>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectDraft {
    pub when: EffectWhen,
    pub condition: String,
    pub actions: String,
}

#[derive(Clone, Default)]
pub struct CharacterDraft {
    pub id: String,
    pub display: String,
    pub properties: Vec<(String, PropertyValue)>,
    pub relations: Vec<(String, String)>,
}

#[derive(Clone, Default)]
pub struct WorldDraft {
    pub id: String,
    pub display: String,
    pub description: String,
    pub properties: Vec<(String, PropertyValue)>,
}

pub fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "")
            .replace('\t', "\\t")
    )
}

pub fn property_source(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Str(s) => quote(s),
        PropertyValue::Num(n) => n.to_string(),
        PropertyValue::Bool(b) => b.to_string(),
    }
}

pub(crate) fn property_lines(properties: &[(String, PropertyValue)]) -> Result<String, String> {
    let mut out = String::new();
    for (name, value) in properties {
        identifier(name)?;
        if matches!(value, PropertyValue::Num(n) if !n.is_finite()) {
            return Err("数值属性必须为有限数值".into());
        }
        out.push_str(&format!("  property {name} = {}\n", property_source(value)));
    }
    Ok(out)
}

pub(crate) fn identifier(id: &str) -> Result<(), String> {
    if valid_identifier(id) && id != "END" {
        Ok(())
    } else {
        Err("ID 须以英文字母或下划线开头,仅含英文字母、数字、下划线,且不能为 END".into())
    }
}

fn qualified(id: &str) -> Result<(), String> {
    for part in id.split('.') {
        identifier(part)?;
    }
    Ok(())
}

struct Block {
    range: Range<usize>,
    header_end: usize,
    indent: usize,
    body_indent: usize,
}

fn block_at(text: &str, lines: &[Line], index: usize) -> Block {
    let line = &lines[index];
    let next = lines
        .iter()
        .skip(index + 1)
        .find(|l| l.indent <= line.indent);
    let offset = |no: u32| {
        text.split_inclusive('\n')
            .take(no.saturating_sub(1) as usize)
            .map(str::len)
            .sum::<usize>()
    };
    Block {
        range: offset(line.no)..next.map(|l| offset(l.no)).unwrap_or(text.len()),
        header_end: offset(line.no + 1),
        indent: line.indent as usize,
        body_indent: lines
            .get(index + 1)
            .filter(|l| l.indent > line.indent)
            .map(|l| l.indent as usize)
            .unwrap_or(line.indent as usize + 2),
    }
}

fn lines(text: &str, path: &Path) -> Vec<Line> {
    lex_source(&path.to_string_lossy(), text, &mut Vec::new())
}

fn header_comment(header: &str) -> &str {
    let cleaned = crate::lexer::strip_comments(header);
    let chars = cleaned.trim_end().chars().count();
    let offset = header
        .char_indices()
        .nth(chars)
        .map(|(i, _)| i)
        .unwrap_or(header.len());
    header[offset..].trim_end_matches(['\r', '\n'])
}

// 属性表单重建声明时保留作者注释;摘出注释后置于声明前,避免依附已删除属性。
pub(crate) fn comments(text: &str) -> String {
    let cleaned = crate::lexer::strip_comments(text);
    let retained: String = text
        .chars()
        .zip(cleaned.chars())
        .map(|(raw, clean)| {
            if raw != clean || raw.is_whitespace() {
                raw
            } else {
                ' '
            }
        })
        .collect();
    retained
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("{}\n", line.trim_start()))
        .collect()
}

fn event_header(draft: &EventDraft) -> String {
    let mut header = format!("event {}", draft.id);
    if !draft.summary.is_empty() {
        header.push_str(&format!(" as {}", quote(&draft.summary)));
    }
    if !draft.characters.is_empty() {
        header.push_str(&format!(" with {}", draft.characters.join(", ")));
    }
    if let Some(order) = draft.order {
        header.push_str(&format!(" at {order}"));
    }
    if let Some(period) = &draft.period {
        header.push_str(&format!(" during {period}"));
    }
    if !draft.predecessors.is_empty() {
        header.push_str(&format!(" follows {}", draft.predecessors.join(", ")));
    }
    if !draft.perm.trim().is_empty() {
        header.push_str(&format!(" perm {}", draft.perm.trim()));
    }
    if !draft.after.trim().is_empty() {
        header.push_str(&format!(" after {}", draft.after.trim()));
    }
    header
}

fn event_source(draft: &EventDraft, indent: usize, body_indent: usize) -> String {
    let padding = " ".repeat(indent);
    let mut out = format!("{padding}{}\n", event_header(draft));
    let padding = " ".repeat(body_indent);
    // 效果声明统一放在正文后,避免正文开头的注释在下一次提取时附着到效果块尾部。
    // 运行时机由 when 决定,与效果声明在正文前后的位置无关。
    for line in draft.body.trim_end().lines() {
        out.push_str(&format!("{padding}{line}\n"));
    }
    for effect in &draft.effects {
        let when = match effect.when {
            EffectWhen::Enter => "enter",
            EffectWhen::Done => "done",
            EffectWhen::Exit => "exit",
        };
        out.push_str(&format!("{padding}effect on {when}"));
        if !effect.condition.trim().is_empty() {
            out.push_str(&format!(" if {}", effect.condition.trim()));
        }
        out.push('\n');
        for line in effect.actions.trim_end().lines() {
            out.push_str(&format!("{padding}  {line}\n"));
        }
    }
    out.push('\n');
    out
}

impl Project {
    pub fn write_period(&mut self, id: &str, display: &str) -> Result<(), String> {
        let parent = self
            .compile()
            .analysis
            .timeline
            .periods
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.parent.clone());
        self.write_period_with_parent(id, display, parent.as_deref())
    }

    pub fn write_period_with_parent(
        &mut self,
        id: &str,
        display: &str,
        parent: Option<&str>,
    ) -> Result<(), String> {
        identifier(id)?;
        if let Some(parent) = parent {
            identifier(parent)?;
        }
        let result = compile_sources(&self.entry, &self.sources());
        let existing = result.analysis.timeline.periods.iter().find(|p| p.id == id);
        let path = existing
            .map(|p| PathBuf::from(&p.file))
            .unwrap_or_else(|| self.entry.clone());
        let mut text = self.document(&path)?.to_string();
        let out = format!(
            "period {id} as {}{}\n",
            quote(display),
            parent.map(|p| format!(" within {p}")).unwrap_or_default()
        );
        if let Some(period) = existing {
            let parsed = lines(&text, &path);
            let i = parsed
                .iter()
                .position(|l| l.no == period.line)
                .ok_or("时段声明不存在")?;
            let block = block_at(&text, &parsed, i);
            let suffix = header_comment(&text[block.range.start..block.header_end]);
            let header = format!("{}{suffix}\n", out.trim_end());
            text.replace_range(block.range.start..block.header_end, &header);
        } else {
            text = format!("{out}\n{text}");
        }
        self.set_text(&path, text)
    }

    pub fn order_events(&mut self, before: &str, after: &str) -> Result<(), String> {
        let (path, mut draft) = self.event_draft(after)?;
        if !draft.predecessors.iter().any(|id| id == before) {
            draft.predecessors.push(before.into());
        }
        self.write_event(&path, Some(after), &draft)
    }

    /// 修改先在副本中完成;任何语法或引用错误都不提交到当前文档。
    pub fn edit(
        &mut self,
        operation: impl FnOnce(&mut Project) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut candidate = self.clone();
        operation(&mut candidate)?;
        let result = candidate.compile();
        if let Some(d) = result
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
        {
            return Err(format!(
                "{}:{} {} {}",
                d.file.rsplit(['/', '\\']).next().unwrap_or(&d.file),
                d.span.line,
                d.code,
                d.message
            ));
        }
        *self = candidate;
        Ok(())
    }

    pub fn event_draft(&self, id: &str) -> Result<(PathBuf, EventDraft), String> {
        let result = compile_sources(&self.entry, &self.sources());
        self.event_draft_from(id, &result)
    }

    /// 一次编译取得全工程事件内容，供只读正文概览使用。
    pub fn event_drafts(&self) -> Vec<(PathBuf, EventDraft)> {
        let result = compile_sources(&self.entry, &self.sources());
        let mut nodes: Vec<_> = result
            .analysis
            .graph
            .nodes
            .iter()
            .filter(|n| n.is_event)
            .collect();
        nodes.sort_by_key(|node| {
            (
                result
                    .analysis
                    .symbols
                    .storyline_order
                    .iter()
                    .position(|s| s == &node.storyline),
                node.seq,
                node.name.clone(),
            )
        });
        nodes
            .iter()
            .filter_map(|node| self.event_draft_from(&node.name, &result).ok())
            .collect()
    }

    fn event_draft_from(
        &self,
        id: &str,
        result: &crate::CompileResult,
    ) -> Result<(PathBuf, EventDraft), String> {
        let index = result.program.event_index(id).ok_or("事件不存在")?;
        let event = &result.program.events[index];
        let path = PathBuf::from(&result.program.event_files[index]);
        let text = self.document(&path)?;
        let lines = lines(text, &path);
        let i = lines
            .iter()
            .position(|l| matches!(&l.kind, LineKind::Event { name, .. } if name == id))
            .ok_or("事件源位置不存在")?;
        let block = block_at(text, &lines, i);
        let padding = " ".repeat(block.body_indent);
        let mut body = String::new();
        let mut effects = Vec::new();
        let mut cursor = block.header_end;
        for (j, line) in lines.iter().enumerate().skip(i + 1) {
            if line.indent as usize <= block.indent {
                break;
            }
            if line.indent as usize != block.body_indent {
                continue;
            }
            let LineKind::Effect {
                when_src, cond_src, ..
            } = &line.kind
            else {
                continue;
            };
            let when = match when_src.as_str() {
                "enter" => EffectWhen::Enter,
                "done" => EffectWhen::Done,
                "exit" => EffectWhen::Exit,
                _ => return Err("效果时机须为 enter、done 或 exit".into()),
            };
            let effect = block_at(text, &lines, j);
            body.push_str(&text[cursor..effect.range.start]);
            // 头部注释放入动作区,条件保持词法层给出的原表达式,不从 AST 反推源码。
            let mut actions = comments(&text[effect.range.start..effect.header_end]);
            actions.push_str(
                &text[effect.header_end..effect.range.end]
                    .lines()
                    // 独立注释允许少于动作的缩进,只移除实际存在的空格。
                    .map(|l| {
                        let spaces = l.bytes().take_while(|b| *b == b' ').count();
                        &l[spaces.min(effect.body_indent)..]
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            effects.push(EffectDraft {
                when,
                condition: cond_src.clone().unwrap_or_default(),
                actions: actions.trim_end().into(),
            });
            cursor = effect.range.end;
        }
        body.push_str(&text[cursor..block.range.end]);
        let body = body
            .lines()
            .map(|l| l.strip_prefix(&padding).unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string();
        let (perm, after) = match &lines[i].kind {
            LineKind::Event {
                perm, after_src, ..
            } => (
                perm.clone().unwrap_or_default(),
                after_src.clone().unwrap_or_default(),
            ),
            _ => (String::new(), String::new()),
        };
        Ok((
            path,
            EventDraft {
                id: id.into(),
                summary: event.summary.clone().unwrap_or_default(),
                storyline: event.storyline.clone(),
                characters: event.characters.clone(),
                order: event.order,
                period: event.period.clone(),
                predecessors: event.predecessors.clone(),
                perm,
                after,
                effects,
                body,
            },
        ))
    }

    pub fn write_event(
        &mut self,
        path: &Path,
        original: Option<&str>,
        draft: &EventDraft,
    ) -> Result<(), String> {
        qualified(&draft.id)?;
        identifier(&draft.storyline)?;
        if let Some(original) = original {
            if original != draft.id {
                return Err("事件 ID 是稳定引用,修改事件内容时请保留 ID".into());
            }
        }
        let text = self.document(path)?.to_string();
        let lines = lines(&text, path);
        let block = original
            .and_then(|id| {
                lines
                    .iter()
                    .position(|l| matches!(&l.kind, LineKind::Event { name, .. } if name == id))
            })
            .map(|i| block_at(&text, &lines, i));
        if original.is_some() && block.is_none() {
            return Err("事件不存在于目标文件".into());
        }
        let old_storyline = if let Some(id) = original {
            Some(self.event_draft(id)?.1.storyline)
        } else {
            None
        };
        let mut text = text;
        let mut source = event_source(
            draft,
            block.as_ref().map(|b| b.indent).unwrap_or(2),
            block.as_ref().map(|b| b.body_indent).unwrap_or(4),
        );
        if let Some(block) = block {
            let suffix = header_comment(&text[block.range.start..block.header_end]);
            if let Some(end) = source.find('\n') {
                source.insert_str(end, suffix);
            }
            if old_storyline.as_deref() == Some(&draft.storyline) {
                text.replace_range(block.range, &source);
                return self.set_text(path, text);
            }
            source = event_source(draft, 2, 4);
            if let Some(end) = source.find('\n') {
                source.insert_str(end, suffix);
            }
            text.replace_range(block.range, "");
        }
        text.push_str(&format!("\nstoryline {}\n{}", draft.storyline, source));
        self.set_text(path, text)
    }

    pub fn move_event(&mut self, id: &str, storyline: &str, position: usize) -> Result<(), String> {
        identifier(storyline)?;
        if self.event_draft(id)?.1.period.is_some() {
            return Err("时段内事件为部分顺序,请用先后约束编辑时间关系".into());
        }
        let result = compile_sources(&self.entry, &self.sources());
        let mut nodes: Vec<_> = result
            .analysis
            .graph
            .nodes
            .iter()
            .filter(|n| {
                n.is_event
                    && n.storyline == storyline
                    && n.name != id
                    && !result
                        .analysis
                        .timeline
                        .events
                        .iter()
                        .any(|e| e.event == n.name)
            })
            .collect();
        nodes.sort_by_key(|n| (n.seq, &n.name));
        let mut names: Vec<String> = nodes.iter().map(|n| n.name.clone()).collect();
        names.insert(position.min(names.len()), id.into());
        for (i, name) in names.iter().enumerate() {
            let (path, mut draft) = self.event_draft(name)?;
            draft.storyline = storyline.into();
            draft.order = Some((i as u32 + 1) * 10);
            self.write_event(&path, Some(name), &draft)?;
        }
        Ok(())
    }

    pub fn connect_events(
        &mut self,
        from: &str,
        to: &str,
        label: &str,
        drift: bool,
    ) -> Result<(), String> {
        qualified(to)?;
        let (path, mut draft) = self.event_draft(from)?;
        let body_lines = lines(&draft.body, &path);
        let terminal = body_lines
            .iter()
            .find(|l| l.indent == 0 && matches!(l.kind, LineKind::Divert { .. }));
        let arrow = if drift { "->>" } else { "->" };
        let mut body: Vec<String> = draft.body.lines().map(str::to_string).collect();
        if label.trim().is_empty() {
            let line = format!("{arrow} {to}");
            if let Some(terminal) = terminal {
                body[terminal.no as usize - 1] = line;
            } else {
                body.push(line);
            }
        } else {
            let insert = body_lines
                .iter()
                .find(|l| {
                    l.indent == 0
                        && matches!(l.kind, LineKind::Choice { .. } | LineKind::Divert { .. })
                })
                .map(|l| l.no as usize - 1)
                .unwrap_or(body.len());
            body.insert(insert, format!("choice {}\n  {arrow} {to}", quote(label)));
        }
        draft.body = body.join("\n");
        self.write_event(&path, Some(from), &draft)
    }

    pub fn remove_event(&mut self, id: &str) -> Result<(), String> {
        let (path, _) = self.event_draft(id)?;
        let mut text = self.document(&path)?.to_string();
        let lines = lines(&text, &path);
        let i = lines
            .iter()
            .position(|l| matches!(&l.kind, LineKind::Event { name, .. } if name == id))
            .ok_or("事件不存在")?;
        text.replace_range(block_at(&text, &lines, i).range, "");
        self.set_text(&path, text)
    }

    pub fn write_character(
        &mut self,
        path: &Path,
        original: Option<&str>,
        draft: &CharacterDraft,
    ) -> Result<(), String> {
        identifier(&draft.id)?;
        let mut out = format!(
            "character {} as {}\n{}",
            draft.id,
            quote(&draft.display),
            property_lines(&draft.properties)?
        );
        for (target, label) in &draft.relations {
            identifier(target)?;
            out.push_str(&format!("  relation {target} as {}\n", quote(label)));
        }
        self.replace_metadata(path, original, "character", &out)?;
        if let Some(original) = original.filter(|id| *id != draft.id) {
            self.rename_character_references(original, &draft.id)?;
        }
        Ok(())
    }

    pub fn write_world(&mut self, draft: &WorldDraft) -> Result<(), String> {
        identifier(&draft.id)?;
        let result = compile_sources(&self.entry, &self.sources());
        let world = result.analysis.world;
        let path = world
            .as_ref()
            .map(|w| PathBuf::from(&w.file))
            .unwrap_or_else(|| self.entry.clone());
        let out = format!(
            "world {} as {}\n  description {}\n{}",
            draft.id,
            quote(&draft.display),
            quote(&draft.description),
            property_lines(&draft.properties)?
        );
        self.replace_metadata(&path, world.as_ref().map(|w| w.id.as_str()), "world", &out)
    }

    pub(crate) fn replace_metadata(
        &mut self,
        path: &Path,
        original: Option<&str>,
        kind: &str,
        out: &str,
    ) -> Result<(), String> {
        let mut text = self.document(path)?.to_string();
        if let Some(id) = original {
            let lines = lines(&text, path);
            let i = lines
                .iter()
                .position(|l| match &l.kind {
                    LineKind::World { name, .. } if kind == "world" => name == id,
                    LineKind::Character { name, .. } if kind == "character" => name == id,
                    LineKind::Catalog(crate::catalog::CatalogDecl::Tag(tag)) if kind == "tag" => {
                        tag.name == id
                    }
                    _ => false,
                })
                .ok_or("声明不存在")?;
            let block = block_at(&text, &lines, i);
            let retained = comments(&text[block.range.clone()]);
            text.replace_range(block.range, &format!("{retained}{out}\n"));
        } else {
            text = format!("{out}\n{text}");
        }
        self.set_text(path, text)
    }

    fn rename_character_references(&mut self, old: &str, new: &str) -> Result<(), String> {
        for (path, document) in &mut self.documents {
            let parsed = lines(&document.text, path);
            let mut physical: Vec<String> = document
                .text
                .split_inclusive('\n')
                .map(str::to_string)
                .collect();
            for line in parsed {
                let replacement = match line.kind {
                    LineKind::Text { content, .. } => {
                        let updated = crate::navigation::rename_links(&content, old, new);
                        (updated != content).then_some(updated)
                    }
                    LineKind::Choice {
                        label_raw,
                        once,
                        cond_src,
                        ..
                    } => {
                        let updated = crate::navigation::rename_links(&label_raw, old, new);
                        (updated != label_raw).then(|| {
                            format!(
                                "choice {}{}{}",
                                if once { "once " } else { "" },
                                quote(&updated),
                                cond_src.map(|c| format!(" if {c}")).unwrap_or_default()
                            )
                        })
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::Alias(alias))
                        if alias.target.kind == "character" && alias.target.id == old =>
                    {
                        Some(format!("alias character {new} as {}", quote(&alias.name)))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::AnchorLink(link))
                        if link.target.kind == "character" && link.target.id == old =>
                    {
                        Some(format!("anchor_link {} character {new}", link.anchor))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::State(state))
                        if state.target.kind == "character" && state.target.id == old =>
                    {
                        let tags = if state.tags.is_empty() {
                            "[]".into()
                        } else {
                            state.tags.join(", ")
                        };
                        Some(format!(
                            "state {} on character {new} with {tags} as {}",
                            state.id,
                            quote(&state.display)
                        ))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::Mark(mut link))
                        if link.target.kind == "character" && link.target.id == old =>
                    {
                        link.target.id = new.into();
                        Some(crate::catalog_edit::link_source(
                            &link.target,
                            &link.values,
                            false,
                        ))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::Attach(mut link))
                        if link.target.kind == "character" && link.target.id == old =>
                    {
                        link.target.id = new.into();
                        Some(crate::catalog_edit::link_source(
                            &link.target,
                            &link.values,
                            true,
                        ))
                    }
                    LineKind::Event {
                        name,
                        summary,
                        characters,
                        order,
                        period,
                        predecessors,
                        perm,
                        after_src,
                        ..
                    } if characters.iter().any(|id| id == old) => Some(event_header(&EventDraft {
                        id: name,
                        summary: summary.unwrap_or_default(),
                        characters: characters
                            .into_iter()
                            .map(|id| if id == old { new.into() } else { id })
                            .collect(),
                        order,
                        period,
                        predecessors,
                        perm: perm.unwrap_or_default(),
                        after: after_src.unwrap_or_default(),
                        ..Default::default()
                    })),
                    LineKind::ChangeLine { kind, id, note, .. }
                        if id == old && matches!(kind, ChangeKind::Meet | ChangeKind::Part) =>
                    {
                        Some(format!(
                            "{} {new}{}",
                            if kind == ChangeKind::Meet {
                                "meet"
                            } else {
                                "part"
                            },
                            note.map(|n| format!(" as {}", quote(&n)))
                                .unwrap_or_default()
                        ))
                    }
                    LineKind::Relation { target, label, .. } if target == old => {
                        Some(format!("relation {new} as {}", quote(&label)))
                    }
                    _ => None,
                };
                if let Some(replacement) = replacement {
                    let raw = &physical[line.no as usize - 1];
                    let cleaned = crate::lexer::strip_comments(raw);
                    let chars = cleaned.trim_end().chars().count();
                    let suffix_at = raw
                        .char_indices()
                        .nth(chars)
                        .map(|(i, _)| i)
                        .unwrap_or(raw.len());
                    let suffix = &raw[suffix_at..];
                    physical[line.no as usize - 1] =
                        format!("{}{replacement}{suffix}", " ".repeat(line.indent as usize));
                }
            }
            document.text = physical.concat();
        }
        Ok(())
    }
}

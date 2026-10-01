//! 图形创作的源码修改接口。UI 传递字段,由 core 定位、生成并验证文本。
mod choices;
mod events;
mod predecessors;
mod world;
use crate::ast::{ChangeKind, EffectWhen, PropertyValue};
use crate::lexer::{lex_source, valid_identifier, Line, LineKind};
use crate::project::Project;
use crate::Severity;
pub use choices::ChoiceDraft;
pub use predecessors::{EventPredecessorOption, EventPredecessorOptions};
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

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EntityDraft {
    pub id: String,
    pub entity_type: String,
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
        PropertyValue::Ref(target) => {
            format!("ref({}, {})", quote(&target.kind), quote(&target.id))
        }
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

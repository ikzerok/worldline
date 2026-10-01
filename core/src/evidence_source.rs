//! 非语义证据来源投影；声明头范围由 core 的 AST 与正式词法共同确认。
use crate::ast::Stmt;
use crate::lexer::LineKind;
use crate::CompileResult;
use serde::{Deserialize, Serialize};
use std::{ops::Range, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceSource {
    pub file: String,
    pub line: u32,
    #[serde(flatten)]
    pub owner: EvidenceSourceOwner,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceSourceOwner {
    Choice { node: String },
    Rule { name: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSourcePrecision {
    StatementHeader,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSourceTarget {
    pub path: PathBuf,
    pub range: Range<usize>,
    pub line: u32,
    pub column: u32,
    pub precision: EvidenceSourcePrecision,
}

/// 只读定位。必须传入实际证据对应的编译快照；调用方另行校验工作区与草稿基线。
pub fn resolve_evidence_source(
    snapshot: &CompileResult,
    source: &EvidenceSource,
) -> Result<EvidenceSourceTarget, String> {
    if source.line == 0 || source.file.is_empty() {
        return Err("证据没有有效的声明来源".into());
    }
    let path = PathBuf::from(&source.file);
    let text = snapshot
        .sources
        .get(&path)
        .ok_or("证据来源不属于此编译快照")?;
    let valid = match &source.owner {
        EvidenceSourceOwner::Choice { node } => choice_body(snapshot, &source.file, node)
            .is_some_and(|body| count_choices(body, source.line) == 1),
        EvidenceSourceOwner::Rule { name } => {
            snapshot
                .program
                .rules
                .iter()
                .filter(|rule| {
                    rule.name == *name && rule.file == source.file && rule.loc.line == source.line
                })
                .count()
                == 1
        }
    };
    if !valid {
        return Err("证据所属声明已变化或无法唯一确认".into());
    }
    let line = crate::lexer::lex_source_with_options(
        &source.file,
        text,
        &mut Vec::new(),
        snapshot.options,
    )
    .into_iter()
    .find(|line| line.no == source.line)
    .ok_or("证据声明头已不存在")?;
    let classified = match (&source.owner, line.kind) {
        (EvidenceSourceOwner::Choice { .. }, LineKind::Choice { .. }) => true,
        (EvidenceSourceOwner::Rule { .. }, LineKind::Language111 { keyword, .. }) => {
            keyword == "rule"
        }
        _ => false,
    };
    if !classified {
        return Err("证据来源不再是对应的声明头".into());
    }
    let mut offset = 0;
    let raw = text
        .split_inclusive('\n')
        .nth(source.line as usize - 1)
        .ok_or("证据声明头范围已不存在")?;
    for previous in text.split_inclusive('\n').take(source.line as usize - 1) {
        offset += previous.len();
    }
    let header = raw.trim_end_matches(['\r', '\n']);
    let start = header.len() - header.trim_start().len();
    if start == header.len() {
        return Err("证据声明头为空".into());
    }
    Ok(EvidenceSourceTarget {
        path,
        range: offset + start..offset + header.len(),
        line: source.line,
        column: header[..start].chars().count() as u32 + 1,
        precision: EvidenceSourcePrecision::StatementHeader,
    })
}

fn choice_body<'a>(snapshot: &'a CompileResult, file: &str, node: &str) -> Option<&'a [Stmt]> {
    if let Some(name) = node.strip_prefix("fragment:") {
        let mut definitions = snapshot
            .program
            .fragments
            .iter()
            .filter(|fragment| fragment.name == name && fragment.file == file);
        let definition = definitions.next()?;
        return definitions
            .next()
            .is_none()
            .then_some(definition.body.as_slice());
    }
    let path = snapshot
        .analysis
        .symbols
        .events
        .get(node)
        .or_else(|| snapshot.analysis.symbols.scenes.get(node))?;
    if snapshot.program.event_files.get(path.event)? != file {
        return None;
    }
    let event = snapshot.program.events.get(path.event)?;
    if path.full_name(&event.name) != node {
        return None;
    }
    let mut body = event.body.as_slice();
    for name in &path.scenes {
        body = find_scene(body, name)?;
    }
    Some(body)
}

fn find_scene<'a>(body: &'a [Stmt], name: &str) -> Option<&'a [Stmt]> {
    for statement in body {
        match statement {
            Stmt::Scene(scene) if scene.name == name => return Some(&scene.body),
            Stmt::If(branches) => {
                for (_, branch) in &branches.branches {
                    if let Some(found) = find_scene(branch, name) {
                        return Some(found);
                    }
                }
            }
            Stmt::Choice(choice) => {
                if let Some(found) = find_scene(&choice.body, name) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn count_choices(body: &[Stmt], line: u32) -> usize {
    body.iter()
        .map(|statement| match statement {
            Stmt::Choice(choice) => {
                usize::from(choice.loc.line == line) + count_choices(&choice.body, line)
            }
            Stmt::If(branches) => branches
                .branches
                .iter()
                .map(|(_, branch)| count_choices(branch, line))
                .sum(),
            // A scene belongs to its own complete node identity.
            _ => 0,
        })
        .sum()
}

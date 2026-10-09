//! 非语义证据来源投影；声明头范围由 core 的 AST 与正式词法共同确认。
use crate::ast::{ChangeKind, Stmt};
use crate::CompileResult;
use serde::{Deserialize, Serialize};
use std::{ops::Range, path::PathBuf};
mod batch;
mod outputs;
mod state_actions;
mod variable_writes;
pub use batch::{
    resolve_evidence_source, resolve_evidence_sources, MAX_EVIDENCE_SOURCE_BATCH,
    MAX_EVIDENCE_SOURCE_BATCH_BYTES,
};
pub use outputs::{runtime_output_source_file, RuntimeOutputSourceIndex};
pub use state_actions::state_action_source;
pub use variable_writes::{variable_write_source, variable_write_source_file};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum VariableWriteOperation {
    Let,
    Const,
    Set,
}

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
    Choice {
        node: String,
    },
    Rule {
        name: String,
    },
    StateAction {
        node: String,
        action: ChangeKind,
        timing: String,
        effect_index: Option<usize>,
        action_index: Option<usize>,
    },
    VariableWrite {
        node: String,
        variable: String,
        operation: VariableWriteOperation,
    },
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

fn choice_body<'a>(snapshot: &'a CompileResult, node: &str) -> Option<&'a [Stmt]> {
    if let Some(name) = node.strip_prefix("fragment:") {
        let mut definitions = snapshot
            .program
            .fragments
            .iter()
            .filter(|fragment| fragment.name == name);
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

fn count_choices(
    body: &[Stmt],
    line: u32,
    file: &str,
    sources: &RuntimeOutputSourceIndex<'_>,
) -> usize {
    body.iter()
        .map(|statement| match statement {
            Stmt::Choice(choice) => {
                usize::from(choice.loc.line == line && sources.get(statement) == Some(file))
                    + count_choices(&choice.body, line, file, sources)
            }
            Stmt::If(branches) => branches
                .branches
                .iter()
                .map(|(_, branch)| count_choices(branch, line, file, sources))
                .sum(),
            // A scene belongs to its own complete node identity.
            _ => 0,
        })
        .sum()
}

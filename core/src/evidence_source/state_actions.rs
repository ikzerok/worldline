//! 实际状态动作只按正式 AST 身份与 parser 来源侧表定位，不从变更值反查。
use super::{EvidenceSource, EvidenceSourceOwner};
use crate::ast::{ChangeKind, EffectWhen, Loc, Program, Stmt};
use crate::lexer::LineKind;
use crate::source_provenance::{SourceOwner, StatementKind};

/// 供 runtime 记录瞬态证据；不使用执行帧当前文件或静态状态候选猜测来源。
/// 完整词法与快照校验由 `resolve_evidence_source` 在导航时执行。
pub fn state_action_source(
    program: &Program,
    line: u32,
    owner: &EvidenceSourceOwner,
) -> Option<EvidenceSource> {
    let source = find(program, line, owner)?;
    Some(EvidenceSource {
        file: source.file.into(),
        line,
        owner: owner.clone(),
    })
}

pub(super) struct StateActionSource<'a> {
    pub file: &'a str,
    loc: Loc,
    kind: StatementKind,
}

impl StateActionSource<'_> {
    pub fn matches_header(&self, action: ChangeKind, header: &LineKind) -> bool {
        match (self.kind, header) {
            (StatementKind::Change, LineKind::Become(change)) => {
                change.kind == action && change.loc == self.loc
            }
            (
                StatementKind::DynamicChange,
                LineKind::Language111 {
                    keyword,
                    source,
                    loc,
                },
            ) => {
                keyword == "become"
                    && *loc == self.loc
                    && crate::language::dynamic_change_parts(source)
                        .is_some_and(|(_, _, kind)| kind == action)
            }
            _ => false,
        }
    }
}

pub(super) fn find<'a>(
    program: &'a Program,
    line: u32,
    owner: &EvidenceSourceOwner,
) -> Option<StateActionSource<'a>> {
    let EvidenceSourceOwner::StateAction {
        node,
        action,
        timing,
        effect_index,
        action_index,
    } = owner
    else {
        return None;
    };
    if line == 0
        || !matches!(
            action,
            ChangeKind::Become | ChangeKind::AddTags | ChangeKind::RemoveTags
        )
    {
        return None;
    }
    let (root, loc, kind) = if timing == "during" {
        if effect_index.is_some() || action_index.is_some() {
            return None;
        }
        let body = node_body(program, node)?;
        let mut found = None;
        let mut count = 0;
        visit_actions(body.body, &mut |loc, candidate, kind| {
            if loc.line == line && candidate == *action {
                count += 1;
                found = Some((loc, kind));
            }
        });
        if count != 1 {
            return None;
        }
        let (loc, kind) = found?;
        (SourceOwner::new(body.file, body.line), loc, kind)
    } else {
        let when = match timing.as_str() {
            "enter" => EffectWhen::Enter,
            "exit" => EffectWhen::Exit,
            "done" => EffectWhen::Done,
            _ => return None,
        };
        let mut events = program
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| event.name == *node);
        let (index, event) = events.next()?;
        if events.next().is_some() {
            return None;
        }
        let effect = event.effects.get((*effect_index)?)?;
        let change = effect.actions.get((*action_index)?)?;
        if effect.when != when || change.kind != *action || change.loc.line != line {
            return None;
        }
        (
            SourceOwner::new(program.event_files.get(index)?, event.loc.line),
            change.loc,
            StatementKind::Change,
        )
    };
    let file = program.source_provenance.statement_file(&root, loc, kind)?;
    (!file.is_empty()).then_some(StateActionSource { file, loc, kind })
}

pub(super) struct NodeBody<'a> {
    pub file: &'a str,
    pub line: u32,
    pub scenes: Vec<String>,
    pub body: &'a [Stmt],
}

pub(super) fn node_body<'a>(program: &'a Program, node: &str) -> Option<NodeBody<'a>> {
    if let Some(name) = node.strip_prefix("fragment:") {
        let mut fragments = program.fragments.iter().filter(|f| f.name == name);
        let fragment = fragments.next()?;
        return fragments.next().is_none().then_some(NodeBody {
            file: &fragment.file,
            line: fragment.loc.line,
            scenes: Vec::new(),
            body: fragment.body.as_slice(),
        });
    }
    let mut found = None;
    let mut count = 0;
    for (index, event) in program.events.iter().enumerate() {
        if is_node_prefix(&event.name, node) {
            visit_node_bodies(
                &event.body,
                &event.name,
                node,
                &mut Vec::new(),
                &mut |scenes, body| {
                    count += 1;
                    found = program.event_files.get(index).map(|file| NodeBody {
                        file,
                        line: event.loc.line,
                        scenes: scenes.to_vec(),
                        body,
                    });
                },
            );
        }
    }
    if count == 1 {
        found
    } else {
        None
    }
}

fn is_node_prefix(parent: &str, node: &str) -> bool {
    parent == node
        || node
            .strip_prefix(parent)
            .is_some_and(|s| s.starts_with('.'))
}

fn visit_node_bodies<'a>(
    body: &'a [Stmt],
    current: &str,
    target: &str,
    scenes: &mut Vec<String>,
    visit: &mut impl FnMut(&[String], &'a [Stmt]),
) {
    if current == target {
        visit(scenes, body);
        return;
    }
    for statement in body {
        match statement {
            Stmt::Scene(scene) => {
                let node = format!("{current}.{}", scene.name);
                if is_node_prefix(&node, target) {
                    scenes.push(scene.name.clone());
                    visit_node_bodies(&scene.body, &node, target, scenes, visit);
                    scenes.pop();
                }
            }
            Stmt::Choice(choice) => visit_node_bodies(&choice.body, current, target, scenes, visit),
            Stmt::If(branches) => {
                for (_, branch) in &branches.branches {
                    visit_node_bodies(branch, current, target, scenes, visit);
                }
            }
            _ => {}
        }
    }
}

fn visit_actions(body: &[Stmt], visit: &mut impl FnMut(Loc, ChangeKind, StatementKind)) {
    for statement in body {
        match statement {
            Stmt::Change(statement) => visit(
                statement.change.loc,
                statement.change.kind,
                StatementKind::Change,
            ),
            Stmt::DynamicChange(statement) => {
                visit(statement.loc, statement.kind, StatementKind::DynamicChange)
            }
            Stmt::Choice(choice) => visit_actions(&choice.body, visit),
            Stmt::If(branches) => {
                for (_, branch) in &branches.branches {
                    visit_actions(branch, visit);
                }
            }
            // 场景有自己的完整节点身份，不能归入父事件/场景的正文动作。
            _ => {}
        }
    }
}

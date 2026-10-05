//! 变量证据定位真实已执行语句；不从变量值或初始声明反推来源。
use super::{EvidenceSource, EvidenceSourceOwner, VariableWriteOperation};
use crate::ast::{Loc, Program, Stmt};
use crate::lexer::LineKind;

/// 供 runtime 记录瞬态证据；词法与当前快照校验由定位入口完成。
pub fn variable_write_source(
    program: &Program,
    line: u32,
    owner: &EvidenceSourceOwner,
) -> Option<EvidenceSource> {
    let file = variable_write_source_file(program, line, owner)?;
    Some(EvidenceSource {
        file: file.into(),
        line,
        owner: owner.clone(),
    })
}

/// 借用正式来源，供 runtime 在复制证据前检查来源字段与序列化额度。
pub fn variable_write_source_file<'a>(
    program: &'a Program,
    line: u32,
    owner: &EvidenceSourceOwner,
) -> Option<&'a str> {
    Some(find(program, line, owner)?.file)
}

pub(super) struct VariableWriteSource<'a> {
    pub file: &'a str,
    loc: Loc,
    variable: &'a str,
    operation: VariableWriteOperation,
}

impl VariableWriteSource<'_> {
    pub fn matches_header(&self, header: &LineKind) -> bool {
        let (name, span) = match (self.operation, header) {
            (
                VariableWriteOperation::Let,
                LineKind::Let {
                    name, name_span, ..
                },
            )
            | (
                VariableWriteOperation::Const,
                LineKind::Const {
                    name, name_span, ..
                },
            )
            | (
                VariableWriteOperation::Set,
                LineKind::Set {
                    name, name_span, ..
                },
            ) => (name, name_span),
            _ => return false,
        };
        name == self.variable && Loc::new(span.line, span.column) == self.loc
    }
}

pub(super) fn find<'a>(
    program: &'a Program,
    line: u32,
    owner: &EvidenceSourceOwner,
) -> Option<VariableWriteSource<'a>> {
    let EvidenceSourceOwner::VariableWrite {
        node,
        variable,
        operation,
    } = owner
    else {
        return None;
    };
    if line == 0 || variable.is_empty() {
        return None;
    }
    let body = super::state_actions::node_body(program, node)?;
    let mut found = None;
    let mut count = 0;
    visit_writes(body.body, &mut |loc, name, candidate, declared_file| {
        if loc.line == line && name == variable && candidate == *operation {
            count += 1;
            found = Some((loc, name, declared_file));
        }
    });
    if count != 1 {
        return None;
    }
    let (loc, variable, declared_file) = found?;
    let file = program.source_provenance.variable_write_file(
        (body.file, body.line),
        &body.scenes,
        loc,
        variable,
        *operation,
    )?;
    if declared_file.is_some_and(|declared| declared != file) {
        return None;
    }
    (!file.is_empty()).then_some(VariableWriteSource {
        file,
        loc,
        variable,
        operation: *operation,
    })
}

fn visit_writes<'a>(
    body: &'a [Stmt],
    visit: &mut impl FnMut(Loc, &'a str, VariableWriteOperation, Option<&'a str>),
) {
    for statement in body {
        if let Some((loc, name, operation)) = VariableWriteOperation::statement(statement) {
            let file = match statement {
                Stmt::Let(statement) => Some(statement.file.as_str()),
                _ => None,
            };
            visit(loc, name, operation, file);
        }
        match statement {
            Stmt::Choice(choice) => visit_writes(&choice.body, visit),
            Stmt::If(branches) => {
                for (_, branch) in &branches.branches {
                    visit_writes(branch, visit);
                }
            }
            // 场景独占完整节点身份；参数和 local 不属于全局变量写入。
            _ => {}
        }
    }
}

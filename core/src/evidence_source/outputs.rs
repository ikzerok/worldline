//! 正式执行语句的来源；不由渲染文字或暂停后节点反推。
use crate::ast::{Program, Stmt};
use crate::source_provenance::{SourceOwner, StatementKind};

/// 必须传入该 program 内真实执行的正文/说话/选择语句；缺失、歧义或合成 AST 无来源时返回 None。
pub fn runtime_output_source_file<'a>(
    program: &'a Program,
    node: &str,
    statement: &Stmt,
) -> Option<&'a str> {
    let loc = match statement {
        Stmt::Text(text) => text.loc,
        Stmt::Say(say) => say.loc,
        Stmt::Choice(choice) => choice.loc,
        _ => return None,
    };
    let body = super::state_actions::node_body(program, node)?;
    if !contains(body.body, statement) {
        return None;
    }
    program.source_provenance.statement_file(
        &SourceOwner::new(body.file, body.line),
        loc,
        StatementKind::of(statement),
    )
}

fn contains(body: &[Stmt], target: &Stmt) -> bool {
    body.iter().any(|statement| {
        std::ptr::eq(statement, target)
            || match statement {
                Stmt::Choice(choice) => contains(&choice.body, target),
                Stmt::If(branches) => branches
                    .branches
                    .iter()
                    .any(|(_, body)| contains(body, target)),
                // 场景归属自己的完整节点身份。
                _ => false,
            }
    })
}

use super::*;
use crate::ast::Stmt;

/// 在共享递归本地化收集器之前验证一次；来源导航只接收该不可变快照。
pub(super) fn ast_envelope(compiled: &crate::CompileResult) -> Result<(), ProductionError> {
    let mut pending: Vec<_> = compiled
        .program
        .events
        .iter()
        .map(|event| (event.body.as_slice(), 0))
        .chain(
            compiled
                .program
                .fragments
                .iter()
                .map(|fragment| (fragment.body.as_slice(), 0)),
        )
        .collect();
    let mut nodes = pending.len();
    while let Some((body, depth)) = pending.pop() {
        if depth > 64 {
            return Err(ProductionError::budget());
        }
        nodes = nodes.saturating_add(body.len());
        if nodes > 200_000 {
            return Err(ProductionError::budget());
        }
        for statement in body {
            match statement {
                Stmt::Scene(scene) => pending.push((&scene.body, depth + 1)),
                Stmt::Choice(choice) => pending.push((&choice.body, depth + 1)),
                Stmt::If(condition) => {
                    nodes = nodes.saturating_add(condition.branches.len());
                    if nodes > 200_000 {
                        return Err(ProductionError::budget());
                    }
                    pending.extend(
                        condition
                            .branches
                            .iter()
                            .map(|(_, body)| (body.as_slice(), depth + 1)),
                    );
                }
                _ => {}
            }
        }
        if pending.len() > 200_000 {
            return Err(ProductionError::budget());
        }
    }
    Ok(())
}

use super::*;
use crate::{
    ast::{BinOp, Expr, UnOp, ValueKind},
    CompileResult,
};

pub(super) fn validate_budget(tree: &ConditionNode) -> Result<(), String> {
    let mut pending = vec![(tree, 1)];
    let mut count = 0;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if count > MAX_CONDITION_NODES || depth > MAX_CONDITION_DEPTH {
            return Err("条件树超过 128 节点或 24 层预算".into());
        }
        match node {
            ConditionNode::Not { child } => pending.push((child, depth + 1)),
            ConditionNode::And { left, right } | ConditionNode::Or { left, right } => {
                pending.push((right, depth + 1));
                pending.push((left, depth + 1));
            }
            ConditionNode::GlobalBool { variable } => super::protocol::identifier(variable)?,
            ConditionNode::Has { state_id, tag_id } => {
                super::protocol::identifier(state_id)?;
                super::protocol::identifier(tag_id)?;
            }
            ConditionNode::Compare {
                variable, value, ..
            } => {
                super::protocol::identifier(variable)?;
                match value {
                    ConditionLiteral::Num(n) if !n.is_finite() => {
                        return Err("条件数值必须有限".into())
                    }
                    ConditionLiteral::Str(s)
                        if s.len() > MAX_CONDITION_SOURCE_BYTES || s.contains('\r') =>
                    {
                        return Err("条件字符串超过预算或含不能无损表达的回车".into());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}
pub(super) fn expression_budget(expression: &Expr) -> bool {
    let mut syntax = vec![(expression, 1)];
    let mut syntax_count = 0;
    while let Some((node, depth)) = syntax.pop() {
        syntax_count += 1;
        if syntax_count > 2048 || depth > 64 {
            return false;
        }
        match node {
            Expr::Unary { expr, .. } => syntax.push((expr, depth + 1)),
            Expr::Binary { lhs, rhs, .. } => {
                syntax.push((rhs, depth + 1));
                syntax.push((lhs, depth + 1));
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    syntax.push((arg, depth + 1));
                }
            }
            _ => {}
        }
    }
    let mut pending = vec![(expression, 1)];
    let mut count = 0;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if count > MAX_CONDITION_NODES || depth > MAX_CONDITION_DEPTH {
            return false;
        }
        match node {
            Expr::Unary {
                op: UnOp::Not,
                expr,
            } => pending.push((expr, depth + 1)),
            Expr::Binary {
                op: BinOp::And | BinOp::Or,
                lhs,
                rhs,
            } => {
                pending.push((rhs, depth + 1));
                pending.push((lhs, depth + 1));
            }
            _ => {}
        }
    }
    true
}
pub(super) fn project(expression: &Expr, compiled: &CompileResult) -> Option<ConditionNode> {
    Some(match expression {
        Expr::Bool(value) => ConditionNode::Bool { value: *value },
        Expr::Var { name, .. }
            if compiled.analysis.symbols.vars.get(name)?.kind == Some(ValueKind::Bool) =>
        {
            ConditionNode::GlobalBool {
                variable: name.clone(),
            }
        }
        Expr::Call { name, args, .. } if name == "has" && args.len() == 2 => {
            let state_id = static_name(&args[0])?;
            let tag_id = static_name(&args[1])?;
            if !compiled.analysis.catalog.states.contains_key(state_id)
                || !compiled
                    .analysis
                    .catalog
                    .tags
                    .get(tag_id)
                    .is_some_and(|t| t.declared)
            {
                return None;
            }
            ConditionNode::Has {
                state_id: state_id.into(),
                tag_id: tag_id.into(),
            }
        }
        Expr::Unary {
            op: UnOp::Not,
            expr,
        } => ConditionNode::Not {
            child: Box::new(project(expr, compiled)?),
        },
        Expr::Binary { op, lhs, rhs } => match op {
            BinOp::And => ConditionNode::And {
                left: Box::new(project(lhs, compiled)?),
                right: Box::new(project(rhs, compiled)?),
            },
            BinOp::Or => ConditionNode::Or {
                left: Box::new(project(lhs, compiled)?),
                right: Box::new(project(rhs, compiled)?),
            },
            _ => {
                let (variable, value, literal_left) = match (&**lhs, &**rhs) {
                    (Expr::Var { name, .. }, literal) => (name, literal_value(literal)?, false),
                    (literal, Expr::Var { name, .. }) => (name, literal_value(literal)?, true),
                    _ => return None,
                };
                let op = comparison(*op)?;
                if value_kind(&value) != compiled.analysis.symbols.vars.get(variable)?.kind? {
                    return None;
                }
                ConditionNode::Compare {
                    variable: variable.clone(),
                    op,
                    value,
                    literal_left,
                }
            }
        },
        _ => return None,
    })
}
fn static_name(expression: &Expr) -> Option<&str> {
    match expression {
        Expr::Str(s) | Expr::Var { name: s, .. } => Some(s),
        _ => None,
    }
}
fn literal_value(expression: &Expr) -> Option<ConditionLiteral> {
    match expression {
        Expr::Num(n) if n.is_finite() => Some(ConditionLiteral::Num(*n)),
        Expr::Str(s) if !s.contains('\r') => Some(ConditionLiteral::Str(s.clone())),
        Expr::Bool(b) => Some(ConditionLiteral::Bool(*b)),
        Expr::Unary {
            op: UnOp::Neg,
            expr,
        } => match &**expr {
            Expr::Num(n) if n.is_finite() => Some(ConditionLiteral::Num(-n)),
            _ => None,
        },
        _ => None,
    }
}
fn value_kind(value: &ConditionLiteral) -> ValueKind {
    match value {
        ConditionLiteral::Num(_) => ValueKind::Num,
        ConditionLiteral::Str(_) => ValueKind::Str,
        ConditionLiteral::Bool(_) => ValueKind::Bool,
    }
}
fn comparison(op: BinOp) -> Option<ConditionComparison> {
    Some(match op {
        BinOp::Eq => ConditionComparison::Eq,
        BinOp::Neq => ConditionComparison::Neq,
        BinOp::Lt => ConditionComparison::Lt,
        BinOp::Le => ConditionComparison::Le,
        BinOp::Gt => ConditionComparison::Gt,
        BinOp::Ge => ConditionComparison::Ge,
        _ => return None,
    })
}
pub(super) fn render(tree: &ConditionNode) -> Result<String, String> {
    validate_budget(tree)?;
    fn write(tree: &ConditionNode) -> String {
        match tree {
            ConditionNode::Bool { value } => value.to_string(),
            ConditionNode::GlobalBool { variable } => variable.clone(),
            ConditionNode::Has { state_id, tag_id } => {
                let identity = |id: &str| {
                    if matches!(id, "true" | "false" | "and" | "or" | "not") {
                        crate::authoring::quote(id)
                    } else {
                        id.to_owned()
                    }
                };
                format!("has({}, {})", identity(state_id), identity(tag_id))
            }
            ConditionNode::Compare {
                variable,
                op,
                value,
                literal_left,
            } => {
                let value = match value {
                    ConditionLiteral::Num(n) => n.to_string(),
                    ConditionLiteral::Str(s) => crate::authoring::quote(s),
                    ConditionLiteral::Bool(b) => b.to_string(),
                };
                let symbol = match op {
                    ConditionComparison::Eq => "==",
                    ConditionComparison::Neq => "!=",
                    ConditionComparison::Lt => "<",
                    ConditionComparison::Le => "<=",
                    ConditionComparison::Gt => ">",
                    ConditionComparison::Ge => ">=",
                };
                if *literal_left {
                    format!("{value} {symbol} {variable}")
                } else {
                    format!("{variable} {symbol} {value}")
                }
            }
            ConditionNode::Not { child } => format!("not ({})", write(child)),
            ConditionNode::And { left, right } => {
                format!("({}) and ({})", write(left), write(right))
            }
            ConditionNode::Or { left, right } => format!("({}) or ({})", write(left), write(right)),
        }
    }
    let text = write(tree);
    if text.len() > MAX_CONDITION_SOURCE_BYTES {
        return Err("生成条件超过 16 KiB 预算".into());
    }
    Ok(text)
}

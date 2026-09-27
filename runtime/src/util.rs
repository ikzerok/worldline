use std::collections::{BTreeMap, HashSet};

use worldline_core::ast::{Expr, Stmt, UnOp};
use worldline_core::Analysis;

use super::{RunError, Value};

pub(super) fn static_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var { name, .. } | Expr::Str(name) => Some(name),
        _ => None,
    }
}

pub(super) fn unique_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.iter()
        .filter(|tag| seen.insert(*tag))
        .cloned()
        .collect()
}

pub(super) fn initial_states(analysis: &Analysis) -> BTreeMap<String, Vec<String>> {
    analysis
        .catalog
        .states
        .iter()
        .map(|(id, state)| (id.clone(), unique_tags(&state.tags)))
        .collect()
}

pub(super) fn normalize_seed(seed: u64) -> u64 {
    if seed == 0 {
        0x9E37_79B9_7F4A_7C15
    } else {
        seed
    }
}

pub(super) fn next_rnd(rng: &mut u64) -> u64 {
    let mut value = normalize_seed(*rng);
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *rng = value;
    value
}

pub(super) fn choice_signature(choice: &worldline_core::ast::ChoiceStmt) -> String {
    format!(
        "label={};condition={};once={}",
        choice.label_raw,
        choice
            .cond
            .as_ref()
            .map(expression_signature)
            .unwrap_or_else(|| "always".into()),
        choice.once
    )
}

pub(super) fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn expression_signature(expression: &Expr) -> String {
    match expression {
        Expr::Num(value) => format!("num:{:016x}", value.to_bits()),
        Expr::Str(value) => format!("str:{}", serde_json::to_string(value).unwrap_or_default()),
        Expr::Bool(value) => format!("bool:{value}"),
        Expr::Var { name, .. } => format!("var:{name}"),
        Expr::Unary { op, expr } => format!("unary:{op:?}({})", expression_signature(expr)),
        Expr::Binary { op, lhs, rhs } => format!(
            "binary:{:?}({},{})",
            op,
            expression_signature(lhs),
            expression_signature(rhs)
        ),
        Expr::Call { name, args, .. } => format!(
            "call:{name}({})",
            args.iter()
                .map(expression_signature)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

pub(super) fn expression_source(expression: &Expr) -> String {
    match expression {
        Expr::Num(value) => value.to_string(),
        Expr::Str(value) => serde_json::to_string(value).unwrap_or_default(),
        Expr::Bool(value) => value.to_string(),
        Expr::Var { name, .. } => name.clone(),
        Expr::Unary { op, expr } => {
            let operator = match op {
                UnOp::Neg => "-",
                UnOp::Not => "not ",
            };
            format!("{operator}{}", expression_source(expr))
        }
        Expr::Binary { op, lhs, rhs } => format!(
            "{} {} {}",
            expression_source(lhs),
            op.symbol(),
            expression_source(rhs)
        ),
        Expr::Call { name, args, .. } => format!(
            "{name}({})",
            args.iter()
                .map(expression_source)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(super) fn num_op(
    l: &Value,
    r: &Value,
    line: u32,
    f: impl Fn(f64, f64) -> f64,
) -> Result<Value, RunError> {
    match (l, r) {
        (Value::Num(a), Value::Num(b)) => Ok(Value::Num(f(*a, *b))),
        _ => Err(RunError {
            message: format!("算术运算不接受 {} 与 {}", l.kind_label(), r.kind_label()),
            node: None,
            line: Some(line),
        }),
    }
}

pub(super) fn cmp_op(
    l: &Value,
    r: &Value,
    line: u32,
    f: impl Fn(f64, f64) -> bool,
) -> Result<Value, RunError> {
    match (l, r) {
        (Value::Num(a), Value::Num(b)) => Ok(Value::Bool(f(*a, *b))),
        _ => Err(RunError {
            message: format!("比较运算不接受 {} 与 {}", l.kind_label(), r.kind_label()),
            node: None,
            line: Some(line),
        }),
    }
}

pub(super) fn stmt_line(s: &Stmt) -> u32 {
    match s {
        Stmt::Text(t) => t.loc.line,
        Stmt::Choice(c) => c.loc.line,
        Stmt::If(i) => i.loc.line,
        Stmt::Divert(d) => d.loc.line,
        Stmt::Let(l) => l.loc.line,
        Stmt::Set(st) => st.loc.line,
        Stmt::Scene(sc) => sc.loc.line,
        Stmt::Change(c) => c.change.loc.line,
        Stmt::Anchor(a) => a.loc.line,
        Stmt::Effect(f) => f.loc.line,
    }
}

pub(super) fn expr_loc_line(e: &Expr) -> u32 {
    match e {
        Expr::Var { loc, .. } | Expr::Call { loc, .. } => loc.line,
        Expr::Unary { expr, .. } => expr_loc_line(expr),
        Expr::Binary { lhs, .. } => expr_loc_line(lhs),
        _ => 0,
    }
}

pub(super) fn seed_now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    use std::time::{SystemTime, UNIX_EPOCH};
    #[cfg(target_arch = "wasm32")]
    use web_time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15);
    nanos | 1 // xorshift 种子非零
}

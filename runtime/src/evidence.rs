//! Bounded, transient evidence collected during the existing expression evaluation.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use worldline_core::ast::{Expr, UnOp};

use crate::{RunError, Value};

const MAX_NODES: usize = 128;
const MAX_DEPTH: usize = 24;
const MAX_BYTES: usize = 16 * 1024;
const MAX_TEXT: usize = 2048;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConditionEvidence {
    pub display_expression: String,
    pub nodes: Vec<EvidenceNode>,
    pub omitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceNode {
    pub parent: Option<usize>,
    pub label: String,
    #[serde(flatten)]
    pub outcome: EvidenceOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EvidenceOutcome {
    Evaluated { value: Value },
    Error { message: String },
    NotEvaluated,
    Omitted,
}

pub(super) struct EvidenceBudget {
    nodes: usize,
    bytes: usize,
}

impl Default for EvidenceBudget {
    fn default() -> Self {
        Self {
            nodes: 512,
            bytes: 64 * 1024,
        }
    }
}

pub(super) struct EvidenceRecorder {
    evidence: ConditionEvidence,
    indices: HashMap<*const Expr, usize>,
    node_limit: usize,
    bytes_left: usize,
    byte_limit: usize,
}

impl EvidenceRecorder {
    pub fn new(expression: &Expr, budget: &EvidenceBudget) -> Self {
        let mut recorder = Self {
            evidence: ConditionEvidence {
                display_expression: String::new(),
                nodes: Vec::new(),
                omitted: false,
            },
            indices: HashMap::new(),
            node_limit: MAX_NODES.min(budget.nodes),
            bytes_left: MAX_BYTES.min(budget.bytes),
            byte_limit: MAX_BYTES.min(budget.bytes),
        };
        let mut display = EvidenceDisplay::default();
        display_expression(expression, &mut display, 0);
        recorder.evidence.display_expression = recorder.text(&display.text);
        if display.omitted {
            recorder.evidence.omitted = true;
        }
        recorder.add(expression, None, 0);
        recorder
    }

    fn text(&mut self, text: &str) -> String {
        let limit = MAX_TEXT.min(self.bytes_left);
        let mut end = text.len().min(limit);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end < text.len() {
            self.evidence.omitted = true;
        }
        self.bytes_left -= end;
        text[..end].into()
    }

    fn add(&mut self, expression: &Expr, parent: Option<usize>, depth: usize) {
        if self.evidence.nodes.len() >= self.node_limit
            || depth >= MAX_DEPTH
            || self.bytes_left == 0
        {
            self.evidence.omitted = true;
            return;
        }
        let label = match expression {
            Expr::Num(value) => value.to_string(),
            Expr::Bool(value) => value.to_string(),
            Expr::Str(_) => "字符串".into(),
            Expr::Var { name, .. } => name.clone(),
            Expr::Unary { op, .. } => match op {
                UnOp::Neg => "-",
                UnOp::Not => "not",
            }
            .into(),
            Expr::Binary { op, .. } => op.symbol().into(),
            Expr::Call { name, .. } => name.clone(),
        };
        let label = self.text(&label);
        let index = self.evidence.nodes.len();
        self.indices.insert(expression as *const Expr, index);
        self.evidence.nodes.push(EvidenceNode {
            parent,
            label,
            outcome: EvidenceOutcome::NotEvaluated,
        });
        match expression {
            Expr::Unary { expr, .. } => self.add(expr, Some(index), depth + 1),
            Expr::Binary { lhs, rhs, .. } => {
                self.add(lhs, Some(index), depth + 1);
                self.add(rhs, Some(index), depth + 1);
            }
            // Other built-ins consume static identifiers, not evaluated operands.
            Expr::Call { name, args, .. } if name == "rnd" => {
                for argument in args.iter().take(2) {
                    self.add(argument, Some(index), depth + 1);
                }
            }
            _ => {}
        }
    }

    pub fn record(&mut self, expression: &Expr, result: &Result<Value, RunError>) {
        let Some(&index) = self.indices.get(&(expression as *const Expr)) else {
            return;
        };
        let outcome = match result {
            Ok(Value::Str(value)) if value.len() > self.bytes_left || value.len() > MAX_TEXT => {
                self.evidence.omitted = true;
                EvidenceOutcome::Omitted
            }
            Ok(Value::Num(value)) if !value.is_finite() => {
                // JSON cannot represent nonfinite f64 values faithfully.
                self.evidence.omitted = true;
                EvidenceOutcome::Omitted
            }
            Ok(value) => {
                if let Value::Str(text) = value {
                    self.bytes_left -= text.len();
                }
                EvidenceOutcome::Evaluated {
                    value: value.clone(),
                }
            }
            Err(error) => {
                let message = self.text(&error.message);
                EvidenceOutcome::Error { message }
            }
        };
        self.evidence.nodes[index].outcome = outcome;
    }

    pub fn finish(self, budget: &mut EvidenceBudget) -> ConditionEvidence {
        budget.nodes = budget.nodes.saturating_sub(self.evidence.nodes.len());
        budget.bytes = budget
            .bytes
            .saturating_sub(self.byte_limit - self.bytes_left);
        self.evidence
    }
}

#[derive(Default)]
struct EvidenceDisplay {
    text: String,
    omitted: bool,
}

// Display only: never used by choice_signature, observations, or execution.
fn display_expression(expression: &Expr, out: &mut EvidenceDisplay, depth: usize) {
    if out.omitted || out.text.len() >= MAX_TEXT {
        out.omitted = true;
        return;
    }
    if depth >= MAX_DEPTH {
        append(out, "…");
        out.omitted = true;
        return;
    }
    match expression {
        Expr::Num(value) => append(out, &value.to_string()),
        Expr::Bool(value) => append(out, &value.to_string()),
        Expr::Str(value) => {
            append(out, "\"");
            for character in value.chars() {
                if out.omitted || out.text.len() >= MAX_TEXT {
                    out.omitted = true;
                    break;
                }
                match character {
                    '"' => append(out, "\\\""),
                    '\\' => append(out, "\\\\"),
                    '\n' => append(out, "\\n"),
                    '\r' => append(out, "\\r"),
                    '\t' => append(out, "\\t"),
                    c => append(out, &c.to_string()),
                }
            }
            append(out, "\"");
        }
        Expr::Var { name, .. } => append(out, name),
        Expr::Unary { op, expr } => {
            append(
                out,
                match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "not ",
                },
            );
            append(out, "(");
            display_expression(expr, out, depth + 1);
            append(out, ")");
        }
        Expr::Binary { op, lhs, rhs } => {
            append(out, "(");
            display_expression(lhs, out, depth + 1);
            append(out, " ");
            append(out, op.symbol());
            append(out, " ");
            display_expression(rhs, out, depth + 1);
            append(out, ")");
        }
        Expr::Call { name, args, .. } => {
            append(out, name);
            append(out, "(");
            for (index, argument) in args.iter().enumerate() {
                if out.omitted || out.text.len() >= MAX_TEXT {
                    out.omitted = true;
                    break;
                }
                if index > 0 {
                    append(out, ", ");
                }
                display_expression(argument, out, depth + 1);
            }
            append(out, ")");
        }
    }
}

fn append(out: &mut EvidenceDisplay, text: &str) {
    if out.omitted {
        return;
    }
    let mut end = text.len().min(MAX_TEXT.saturating_sub(out.text.len()));
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.text.push_str(&text[..end]);
    if end < text.len() {
        out.omitted = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multibyte_static_argument_truncation_is_explicit_even_below_byte_limit() {
        let mut display = EvidenceDisplay::default();
        append(&mut display, "perm(");
        display_expression(
            &Expr::Str(format!("aaa{}", "😀".repeat(600))),
            &mut display,
            0,
        );
        append(&mut display, ")");
        assert!(display.omitted);
        assert!(display.text.len() < MAX_TEXT);
        assert!(!display.text.ends_with("\")"));
    }
}

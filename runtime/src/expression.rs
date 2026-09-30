use worldline_core::ast::{BinOp, Expr, TextPart, UnOp};

use super::evidence::{EvidenceBudget, EvidenceRecorder};
use super::util::{cmp_op, expr_loc_line, next_rnd, num_op};
use super::ConditionEvidence;
use super::{RunError, Story, Value};

impl<'p> Story<'p> {
    // -- 求值 ---------------------------------------------------------------

    pub(super) fn render_parts(
        &self,
        parts: &[TextPart],
    ) -> Result<(String, Vec<worldline_core::navigation::RenderedLink>), RunError> {
        let mut out = String::new();
        let mut links = Vec::new();
        for p in parts {
            match p {
                TextPart::Str(s) => out.push_str(s),
                TextPart::Link(link) => {
                    let start = out.len();
                    out.push_str(&link.label);
                    let mut target = link.target.clone();
                    if target.kind == "file" {
                        if let Some(file) = self
                            .current_node()
                            .and_then(|node| {
                                self.symbols
                                    .events
                                    .get(node.split('.').next().unwrap_or(&node))
                            })
                            .and_then(|node| self.program.event_files.get(node.event))
                        {
                            target.id = worldline_core::catalog::resolved_asset(file, &target.id)
                                .to_string_lossy()
                                .into_owned();
                        }
                    }
                    links.push(worldline_core::navigation::RenderedLink {
                        target,
                        start,
                        end: out.len(),
                    });
                }
                TextPart::Expr(e) => {
                    let v = self.eval(e)?;
                    out.push_str(&v.display());
                }
            }
        }
        Ok((out, links))
    }

    pub(super) fn eval(&self, e: &Expr) -> Result<Value, RunError> {
        let mut rng = self.rng.get();
        let result = self.eval_with_rng(e, &mut rng);
        self.rng.set(rng);
        result
    }

    pub(super) fn eval_with_rng(&self, e: &Expr, rng: &mut u64) -> Result<Value, RunError> {
        self.eval_recorded(e, rng, &mut None)
    }

    pub(super) fn eval_condition(
        &self,
        expression: &Expr,
        budget: &mut EvidenceBudget,
    ) -> (Result<Value, RunError>, ConditionEvidence) {
        let mut rng = self.rng.get();
        let mut recorder = Some(EvidenceRecorder::new(expression, budget));
        let result = self.eval_recorded(expression, &mut rng, &mut recorder);
        self.rng.set(rng);
        (result, recorder.expect("condition recorder").finish(budget))
    }

    fn eval_recorded(
        &self,
        e: &Expr,
        rng: &mut u64,
        recorder: &mut Option<EvidenceRecorder>,
    ) -> Result<Value, RunError> {
        let result = (|| match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Var { name, loc } => self.vars.get(name).cloned().ok_or_else(|| RunError {
                message: format!("变量 `{name}` 未定义"),
                node: self.current_node(),
                line: Some(loc.line),
            }),
            Expr::Unary { op, expr } => {
                let v = self.eval_recorded(expr, rng, recorder)?;
                match (op, v) {
                    (UnOp::Neg, Value::Num(n)) => Ok(Value::Num(-n)),
                    (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (_, v) => Err(RunError {
                        message: format!("一元运算不适用于{}", v.kind_label()),
                        node: self.current_node(),
                        line: Some(expr_loc_line(e)),
                    }),
                }
            }
            Expr::Binary { op, lhs, rhs } => {
                let l = self.eval_recorded(lhs, rng, recorder)?;
                let r = self.eval_recorded(rhs, rng, recorder)?;
                let line = expr_loc_line(e);
                let type_err = || RunError {
                    message: format!(
                        "运算 `{}` 不接受 {} 与 {}",
                        op.symbol(),
                        l.kind_label(),
                        r.kind_label()
                    ),
                    node: self.current_node(),
                    line: Some(line),
                };
                match op {
                    BinOp::Add => match (&l, &r) {
                        (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a + b)),
                        (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
                        _ => Err(type_err()),
                    },
                    BinOp::Sub => num_op(&l, &r, line, |a, b| a - b),
                    BinOp::Mul => num_op(&l, &r, line, |a, b| a * b),
                    BinOp::Div => {
                        let (Value::Num(a), Value::Num(b)) = (&l, &r) else {
                            return Err(type_err());
                        };
                        if *b == 0.0 {
                            return Err(RunError {
                                message: "除以零".to_string(),
                                node: self.current_node(),
                                line: Some(line),
                            });
                        }
                        Ok(Value::Num(a / b))
                    }
                    BinOp::Mod => {
                        let (Value::Num(a), Value::Num(b)) = (&l, &r) else {
                            return Err(type_err());
                        };
                        if *b == 0.0 {
                            return Err(RunError {
                                message: "对零取模".to_string(),
                                node: self.current_node(),
                                line: Some(line),
                            });
                        }
                        Ok(Value::Num(a % b))
                    }
                    BinOp::Eq => Ok(Value::Bool(l == r)),
                    BinOp::Neq => Ok(Value::Bool(l != r)),
                    BinOp::Lt => cmp_op(&l, &r, line, |a, b| a < b),
                    BinOp::Le => cmp_op(&l, &r, line, |a, b| a <= b),
                    BinOp::Gt => cmp_op(&l, &r, line, |a, b| a > b),
                    BinOp::Ge => cmp_op(&l, &r, line, |a, b| a >= b),
                    BinOp::And => match (&l, &r) {
                        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
                        _ => Err(type_err()),
                    },
                    BinOp::Or => match (&l, &r) {
                        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
                        _ => Err(type_err()),
                    },
                }
            }
            Expr::Call { name, args, loc } => match name.as_str() {
                "has" => {
                    let [state, tag] = args.as_slice() else {
                        return Err(RunError {
                            message: "has 需要状态与标签两个参数".into(),
                            node: self.current_node(),
                            line: Some(loc.line),
                        });
                    };
                    let static_id = |arg: &Expr| match arg {
                        Expr::Var { name, .. } | Expr::Str(name) => Ok(name.clone()),
                        _ => Err(RunError {
                            message: "has 的参数必须是静态标识符或字符串".into(),
                            node: self.current_node(),
                            line: Some(loc.line),
                        }),
                    };
                    let (state, tag) = (static_id(state)?, static_id(tag)?);
                    Ok(Value::Bool(
                        self.states
                            .get(&state)
                            .is_some_and(|tags| tags.contains(&tag)),
                    ))
                }
                "visits" => {
                    let Some(Expr::Str(target)) = args.first() else {
                        return Err(RunError {
                            message: "visits 需要节点名参数".into(),
                            node: self.current_node(),
                            line: Some(loc.line),
                        });
                    };
                    let full = self
                        .symbols
                        .resolve_node(target)
                        .map(|p| p.full_name(&self.program.events[p.event].name))
                        .unwrap_or_else(|| target.clone());
                    Ok(Value::Num(*self.visits.get(&full).unwrap_or(&0) as f64))
                }
                "seen" => {
                    let Some(Expr::Str(target)) = args.first() else {
                        return Err(RunError {
                            message: "seen 需要节点名参数".into(),
                            node: self.current_node(),
                            line: Some(loc.line),
                        });
                    };
                    let full = self
                        .symbols
                        .resolve_node(target)
                        .map(|p| p.full_name(&self.program.events[p.event].name))
                        .unwrap_or_else(|| target.clone());
                    Ok(Value::Bool(*self.visits.get(&full).unwrap_or(&0) > 0))
                }
                "perm" => {
                    let Some(Expr::Str(target)) = args.first() else {
                        return Err(RunError {
                            message: "perm 需要权限名参数".into(),
                            node: self.current_node(),
                            line: Some(loc.line),
                        });
                    };
                    Ok(Value::Bool(self.perm_list().iter().any(|p| p == target)))
                }
                "turns" => Ok(Value::Num(self.turns as f64)),
                "rnd" => {
                    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
                        return Err(RunError {
                            message: "rnd 需要两个数值参数".into(),
                            node: self.current_node(),
                            line: Some(loc.line),
                        });
                    };
                    let (Value::Num(lo), Value::Num(hi)) = (
                        self.eval_recorded(a, rng, recorder)?,
                        self.eval_recorded(b, rng, recorder)?,
                    ) else {
                        return Err(RunError {
                            message: "rnd 的参数必须是数值".into(),
                            node: self.current_node(),
                            line: Some(loc.line),
                        });
                    };
                    let lo = lo.ceil() as u64;
                    let hi = hi.floor() as u64;
                    if hi < lo {
                        return Err(RunError {
                            message: format!("rnd 的上界({hi})小于下界({lo})"),
                            node: self.current_node(),
                            line: Some(loc.line),
                        });
                    }
                    let span = hi - lo + 1;
                    Ok(Value::Num((lo + next_rnd(rng) % span) as f64))
                }
                other => Err(RunError {
                    message: format!("未知函数 `{other}`"),
                    node: self.current_node(),
                    line: Some(loc.line),
                }),
            },
        })();
        if let Some(recorder) = recorder {
            recorder.record(e, &result);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldline_core::ast::Stmt;

    #[test]
    fn recording_and_plain_evaluation_have_identical_values_errors_rng_and_state() {
        for expression in [
            "not ((score + 2 >= 5) and (score * 2 == 6))",
            "false and rnd(1, 100) > 0",
            "true or rnd(1, 100) > 0",
            "rnd(1, 100) / 0 > 0 and rnd(1, 100) > 0",
            "rnd(1, 100) > 0 and rnd(1, 100) / 0 > 0",
            "not (seen(\"start\") and visits(\"start\") > 0)",
        ] {
            let source = format!(
                "let score = 3\nevent start\n  choice \"选\" if {expression}\n    -> END\n"
            );
            let compiled = worldline_core::compile_source("test.wl", &source);
            assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
            let Stmt::Choice(choice) = &compiled.program.events[0].body[0] else {
                panic!("choice")
            };
            let expr = choice.cond.as_ref().unwrap();
            let plain = Story::new_with_seed(&compiled.program, &compiled.analysis, 31).unwrap();
            let recorded = Story::new_with_seed(&compiled.program, &compiled.analysis, 31).unwrap();
            let expected = plain.eval(expr);
            let (actual, _) = recorded.eval_condition(expr, &mut EvidenceBudget::default());
            assert_eq!(
                serde_json::to_value(expected).unwrap(),
                serde_json::to_value(actual).unwrap(),
                "{expression}"
            );
            assert_eq!(
                plain.save().unwrap(),
                recorded.save().unwrap(),
                "{expression}"
            );
        }
    }
}

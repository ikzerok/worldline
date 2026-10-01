//! 可选数组和显式全部可见投影共享一次真实求值。
use super::util::expression_source;
use super::{
    ChoiceExplanation, ChoicePresentation, ChoiceView, ConditionExplanation, Pause, RunError,
    Story, Value,
};
use worldline_core::ast::{Program, Stmt};
pub const CHOICE_PRESENTATION_CAPABILITY: &str = "runtime.choice_presentation.v1";

impl<'p> Story<'p> {
    pub(super) fn pause_choices(&mut self, fi: usize) -> Result<bool, RunError> {
        // 选择组 = 连续 Choice 语句
        let stmts = self.frames[fi].stmts;
        let start = self.frames[fi].idx;
        let mut group_len = 0usize;
        while matches!(stmts.get(start + group_len), Some(Stmt::Choice(_))) {
            group_len += 1;
        }
        let mut choices = Vec::new();
        let mut presentations = Vec::new();
        let mut explanations = Vec::with_capacity(group_len);
        let mut evidence_budget = super::evidence::EvidenceBudget::default();
        self.failed_explanations = None;
        let mut offset = 0usize;
        let rng_before = self.rng.get();
        while let Some(Stmt::Choice(c)) = stmts.get(start + offset) {
            let mut identity = self.choice_identity(fi, start, offset, c.label_raw.clone());
            let condition = if let Some(cond) = &c.cond {
                let (value, evidence) = self.eval_condition(cond, &mut evidence_budget);
                let value = value.map_err(|mut error| {
                    error
                        .node
                        .get_or_insert_with(|| self.current_node().unwrap_or_default());
                    if error.line.is_none() || error.line == Some(0) {
                        error.line = Some(c.loc.line);
                    }
                    error
                });
                let value = match value {
                    Ok(value) => value,
                    Err(error) => {
                        explanations.push(ChoiceExplanation {
                            enable_condition: None,
                            choice: identity,
                            available: false,
                            condition: Some(ConditionExplanation {
                                expression: expression_source(cond),
                                result: None,
                                error: Some(error.message.clone()),
                                evidence: Some(evidence),
                            }),
                            unavailable_reason: Some("条件求值失败".into()),
                        });
                        self.failed_explanations = Some(explanations);
                        return Err(error);
                    }
                };
                let result = matches!(value, Value::Bool(true));
                Some(ConditionExplanation {
                    expression: expression_source(cond),
                    result: Some(result),
                    error: None,
                    evidence: Some(evidence),
                })
            } else {
                None
            };
            if condition
                .as_ref()
                .is_some_and(|value| value.result == Some(false))
            {
                explanations.push(ChoiceExplanation {
                    enable_condition: None,
                    choice: identity,
                    available: false,
                    condition,
                    unavailable_reason: Some("条件求值为 false".into()),
                });
                offset += 1;
                continue;
            }
            if c.once {
                let id = self.choice_id(fi, start, offset);
                if self.taken_once.contains(&id) {
                    explanations.push(ChoiceExplanation {
                        enable_condition: None,
                        choice: identity,
                        available: false,
                        condition,
                        unavailable_reason: Some("once 选择已使用".into()),
                    });
                    offset += 1;
                    continue;
                }
            }
            let enable_condition = if let Some(expr) = &c.enable {
                let (value, evidence) = self.eval_condition(expr, &mut evidence_budget);
                match value {
                    Ok(value) => Some(ConditionExplanation {
                        expression: expression_source(expr),
                        result: Some(matches!(value, Value::Bool(true))),
                        error: None,
                        evidence: Some(evidence),
                    }),
                    Err(mut error) => {
                        error
                            .node
                            .get_or_insert_with(|| self.current_node().unwrap_or_default());
                        if error.line.is_none() || error.line == Some(0) {
                            error.line = Some(c.loc.line);
                        }
                        explanations.push(ChoiceExplanation {
                            choice: identity,
                            available: false,
                            condition,
                            enable_condition: Some(ConditionExplanation {
                                expression: expression_source(expr),
                                result: None,
                                error: Some(error.message.clone()),
                                evidence: Some(evidence),
                            }),
                            unavailable_reason: Some("可选条件求值失败".into()),
                        });
                        self.failed_explanations = Some(explanations);
                        return Err(error);
                    }
                }
            } else {
                None
            };
            let enabled = enable_condition
                .as_ref()
                .is_none_or(|e| e.result == Some(true));
            let (label, links) = match self.render_parts(&c.label) {
                Ok(rendered) => rendered,
                Err(error) => {
                    explanations.push(ChoiceExplanation {
                        enable_condition,
                        choice: identity,
                        available: false,
                        condition,
                        unavailable_reason: Some(format!("选择标签求值失败：{}", error.message)),
                    });
                    self.failed_explanations = Some(explanations);
                    return Err(error);
                }
            };
            identity.label = label.clone();
            let index = enabled.then_some(choices.len());
            presentations.push(ChoicePresentation {
                id: identity.id.clone(),
                label: label.clone(),
                links: links.clone(),
                line: c.loc.line,
                offset,
                enabled,
                index,
                disabled_reason: if enabled {
                    None
                } else {
                    c.disabled_reason.clone()
                },
            });
            if enabled {
                choices.push(ChoiceView {
                    id: identity.id.clone(),
                    label,
                    links,
                    line: c.loc.line,
                    offset,
                });
            }
            explanations.push(ChoiceExplanation {
                choice: identity,
                available: enabled,
                condition,
                enable_condition,
                unavailable_reason: if enabled {
                    None
                } else {
                    Some("可选条件求值为 false".into())
                },
            });
            offset += 1;
        }
        if choices.is_empty() {
            // 组耗尽:落穿到组后(隐式汇聚)
            self.frames[fi].idx = start + group_len;
            return Ok(false);
        }
        self.paused = Some(Box::new(Pause {
            frame_depth: fi,
            start,
            group_len,
            choices,
            presentations,
            explanations,
            rng_before,
        }));
        Ok(true)
    }

    pub fn choice_presentations(&self) -> &[ChoicePresentation] {
        self.paused
            .as_ref()
            .map(|p| p.presentations.as_slice())
            .unwrap_or(&[])
    }
    /// 投影序号与旧 choose(index) 的可选序号是不同的命名空间。
    pub fn choose_presentation(&mut self, index: usize) -> Result<(), RunError> {
        let view = self
            .choice_presentations()
            .get(index)
            .ok_or_else(|| RunError::new("可见选择序号不存在或已过期"))?;
        let index = view
            .index
            .filter(|_| view.enabled)
            .ok_or_else(|| RunError::new("该选择当前不可选择"))?;
        self.choose(index)
    }
    pub fn choose_id(&mut self, id: &str) -> Result<(), RunError> {
        let index = self
            .choice_presentations()
            .iter()
            .position(|v| v.id == id)
            .ok_or_else(|| RunError::new("选择身份不存在或已过期"))?;
        self.choose_presentation(index)
    }
}

pub(super) fn uses_presentation(program: &Program) -> bool {
    fn body(stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::Choice(c) => c.enable.is_some() || body(&c.body),
            Stmt::If(i) => i.branches.iter().any(|(_, b)| body(b)),
            Stmt::Scene(s) => body(&s.body),
            _ => false,
        })
    }
    program.events.iter().any(|e| body(&e.body)) || program.fragments.iter().any(|f| body(&f.body))
}

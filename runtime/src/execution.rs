use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub(super) use std::time::Instant as MonotonicInstant;
#[cfg(target_arch = "wasm32")]
pub(super) use web_time::Instant as MonotonicInstant;

use worldline_core::ast::{DivertTarget, EffectWhen, Stmt};
use worldline_core::evidence_source::VariableWriteOperation;

use super::util::{choice_signature, stable_hash, stmt_line};
use super::{
    AnchorKind, ChoiceCoverage, ChoiceIdentity, Frame, FrameSrc, Output, ReplayBudget,
    ReplayCancellation, ReplayStatus, ReplayStep, ReplayTrace, RunError, Story, Value,
};

pub(super) enum ReplayStop {
    Status(ReplayStatus),
    Yield,
    OutputBudgetExceeded,
    ComparisonRejected(crate::RouteComparisonError),
}

pub(super) struct ContinueOutcome {
    pub(super) outputs: Vec<Output>,
    pub(super) stop: Option<ReplayStop>,
}

pub(super) struct ReplayExecutionBudget<'a> {
    pub(super) limits: ReplayBudget,
    pub(super) cancellation: &'a ReplayCancellation,
    pub(super) started: MonotonicInstant,
    pub(super) steps: u64,
    pub(super) slice: Option<ReplayBudget>,
    pub(super) slice_started: MonotonicInstant,
    pub(super) slice_steps: u64,
    pub(crate) comparison_limit: Option<usize>,
    pub(crate) output_usage: Option<&'a mut crate::route_comparison::OutputUsage>,
}

impl ReplayExecutionBudget<'_> {
    pub(super) fn consume_step(&mut self) -> Option<ReplayStop> {
        if self.cancellation.is_cancelled() {
            return Some(ReplayStop::Status(ReplayStatus::Cancelled));
        }
        if self.limits.time_budget_ms != u64::MAX
            && self.started.elapsed() >= Duration::from_millis(self.limits.time_budget_ms)
        {
            return Some(ReplayStop::Status(ReplayStatus::TimeBudgetExceeded));
        }
        if self.steps >= self.limits.max_steps {
            return Some(ReplayStop::Status(ReplayStatus::StepBudgetExceeded));
        }
        if self.slice_exhausted() {
            return Some(ReplayStop::Yield);
        }
        self.steps += 1;
        if self.slice.is_some() {
            self.slice_steps += 1;
        }
        None
    }

    pub(super) fn slice_exhausted(&self) -> bool {
        self.slice.is_some_and(|slice| {
            self.slice_steps >= slice.max_steps
                || ((self.comparison_limit.is_none() || self.slice_steps > 0)
                    && self.slice_started.elapsed() >= Duration::from_millis(slice.time_budget_ms))
        })
    }
}

impl<'p> Story<'p> {
    // -- 推进 ---------------------------------------------------------------

    pub(super) fn continue_story_inner(
        &mut self,
        budget: &mut ReplayExecutionBudget<'_>,
    ) -> Result<ContinueOutcome, RunError> {
        let mut out = Vec::new();
        let mut counted_outputs = 0;
        if self.paused.is_some() {
            return Ok(self.comparison_outcome(budget, out, None, counted_outputs));
        }
        loop {
            if let Some(stop) = self.comparison_boundary(budget, &mut out, &mut counted_outputs) {
                return Ok(ContinueOutcome {
                    outputs: out,
                    stop: Some(stop),
                });
            }
            let Some(fi) = self.frames.len().checked_sub(1) else {
                out.push(Output::Ended);
                return Ok(self.comparison_outcome(budget, out, None, counted_outputs));
            };
            if let Some(stop) = budget.consume_step() {
                return Ok(self.comparison_outcome(budget, out, Some(stop), counted_outputs));
            }
            if self.frames[fi].idx >= self.frames[fi].stmts.len() {
                // 事件自然完成先 done 后 exit；弹栈前保留记录的事件归属。
                if fi == 0 {
                    self.run_exit_effects(true)?;
                }
                self.frames.pop();
                continue;
            }
            // 借用当前语句;修改帧前先放弃借用
            let stmt_loc_line = stmt_line(&self.frames[fi].stmts[self.frames[fi].idx]);
            match &self.frames[fi].stmts[self.frames[fi].idx] {
                Stmt::Local(_)
                | Stmt::Call(_)
                | Stmt::Return(_)
                | Stmt::Say(_)
                | Stmt::DynamicChange(_) => {
                    self.execute_language(fi, &mut out)?;
                }
                Stmt::Text(t) => {
                    let (content, links) = self.render_parts(&t.parts)?;
                    let tags = t.tags.clone();
                    let new_line = !self.glue_pending;
                    self.glue_pending = false;
                    if !content.is_empty() {
                        self.capture_report_output(fi);
                        out.push(Output::Text {
                            speaker: None,
                            content,
                            new_line,
                            tags,
                            links,
                        });
                    }
                    if t.glue {
                        self.glue_pending = true;
                    }
                    self.frames[fi].idx += 1;
                }
                Stmt::Let(l) => {
                    if !l.is_const || !self.vars.contains_key(&l.name) {
                        let v = self.eval(&l.expr)?;
                        let operation = if l.is_const {
                            VariableWriteOperation::Const
                        } else {
                            VariableWriteOperation::Let
                        };
                        self.write_variable(&l.name, v, operation, l.loc.line);
                    }
                    self.frames[fi].idx += 1;
                }
                Stmt::Set(s) => {
                    if !self.vars.contains_key(&s.name) {
                        return Err(RunError {
                            message: format!("变量 `{}` 已声明但尚未初始化，不能 set", s.name),
                            node: self.current_node(),
                            line: Some(s.loc.line),
                        });
                    }
                    let v = self.eval(&s.expr)?;
                    self.write_variable(&s.name, v, VariableWriteOperation::Set, s.loc.line);
                    self.frames[fi].idx += 1;
                }
                Stmt::If(i) => {
                    let mut taken: Option<usize> = None;
                    for (k, (cond, _)) in i.branches.iter().enumerate() {
                        let hit = match cond {
                            Some(c) => matches!(self.eval(c)?, Value::Bool(true)),
                            None => true,
                        };
                        if hit {
                            taken = Some(k);
                            break;
                        }
                    }
                    match taken {
                        Some(k) => {
                            self.frames[fi].idx += 1;
                            let stmt_idx = self.frames[fi].idx - 1;
                            self.frames.push(Frame {
                                stmts: &i.branches[k].1,
                                idx: 0,
                                node: None,
                                fragment: None,
                                locals: Default::default(),
                                src: Some(FrameSrc::IfBranch {
                                    stmt: stmt_idx,
                                    branch: k,
                                }),
                            });
                        }
                        None => {
                            self.frames[fi].idx += 1;
                        }
                    }
                }
                Stmt::Scene(s) => {
                    let parent = self.frames[fi]
                        .node
                        .clone()
                        .unwrap_or_else(|| self.current_node().unwrap_or_default());
                    let full = format!("{parent}.{}", s.name);
                    *self.visits.entry(full.clone()).or_insert(0) += 1;
                    self.frames[fi].idx += 1;
                    self.frames.push(Frame {
                        stmts: &s.body,
                        idx: 0,
                        node: Some(full),
                        src: None,
                        fragment: None,
                        locals: Default::default(),
                    });
                }
                Stmt::Divert(d) => {
                    match &d.target {
                        DivertTarget::End => {
                            self.run_exit_effects(false)?;
                            self.frames.clear();
                            out.push(Output::Ended);
                            return Ok(self.comparison_outcome(budget, out, None, counted_outputs));
                        }
                        DivertTarget::Node(target) => {
                            let current_event = self.current_event_name();
                            let Some(path) = self
                                .symbols
                                .resolve_target(target, current_event.as_deref())
                            else {
                                return Err(RunError {
                                    message: format!("跃迁目标 `{target}` 无法解析"),
                                    node: self.current_node(),
                                    line: Some(stmt_loc_line),
                                });
                            };
                            let entering_event = path.scenes.is_empty()
                                || current_event.as_deref()
                                    != Some(self.program.events[path.event].name.as_str());
                            if entering_event {
                                self.run_exit_effects(false)?;
                                // 源 exit 先于目标准入；失败保留变更并终止源事件。
                                if let Err(error) = self.admit(path.event) {
                                    self.frames.clear();
                                    return Err(error);
                                }
                            }
                            // 漂流:切换故事线 + 锚点记录(记录发生在帧替换前,归属源节点)
                            if d.drift {
                                let sl = self.program.events[path.event].storyline.clone();
                                self.storyline = sl;
                                self.push_anchor(
                                    AnchorKind::Drift,
                                    "漂流",
                                    None,
                                    Some(target.clone()),
                                );
                            }
                            let chain = self.node_frames(path.event, &path.scenes);
                            for frame in chain.iter().skip(usize::from(!entering_event)) {
                                if let Some(node) = &frame.node {
                                    *self.visits.entry(node.clone()).or_insert(0) += 1;
                                }
                            }
                            self.frames = chain;
                            if entering_event {
                                self.run_effects(path.event, EffectWhen::Enter)?;
                            }
                        }
                    }
                }
                Stmt::Change(c) => {
                    let source = self.action_source(c.change.kind, c.change.loc.line);
                    self.apply_change(&c.change, source)?;
                    self.frames[fi].idx += 1;
                }
                Stmt::Anchor(a) => {
                    self.push_anchor(AnchorKind::Manual, &a.name, a.note.clone(), None);
                    self.frames[fi].idx += 1;
                }
                // 解析期应已提取或报错;运行期遇到则跳过
                Stmt::Effect(_) => {
                    self.frames[fi].idx += 1;
                }
                Stmt::Choice(_) => {
                    if self.pause_choices(fi)? {
                        return Ok(self.comparison_outcome(budget, out, None, counted_outputs));
                    }
                }
            }
        }
    }

    /// 玩家做出选择。
    pub fn choose(&mut self, idx: usize) -> Result<(), RunError> {
        let Some(pause) = self.paused.take() else {
            return Err(RunError::new("当前没有待选选择"));
        };
        let Some(view) = pause.choices.get(idx) else {
            self.paused = Some(pause);
            return Err(RunError::new(format!("选择序号 {idx} 超出范围")));
        };
        let selected_id = view.id.clone();
        let offset = view.offset;
        let selected_identity = pause
            .explanations
            .iter()
            .find(|explanation| explanation.available && explanation.choice.id == selected_id)
            .map(|explanation| explanation.choice.clone())
            .ok_or_else(|| RunError::new("内部状态损坏:选择解释丢失"))?;
        let fi = pause.frame_depth;
        let start = pause.start;
        let group_len = pause.group_len;
        let stmts = self.frames[fi].stmts;
        let Some(Stmt::Choice(c)) = stmts.get(start + offset) else {
            return Err(RunError::new("内部状态损坏:选择语句丢失"));
        };
        if c.once {
            let id = self.choice_id(fi, start, offset);
            if !self.taken_once.contains(&id) {
                self.taken_once.push(id);
            }
        }
        self.turns += 1;
        // 组结束位置 = 选择体落回点(隐式汇聚)
        self.frames[fi].idx = start + group_len;
        let body: &'p [Stmt] = &c.body;
        let src = FrameSrc::ChoiceBody {
            stmt: start + offset,
        };
        self.frames.push(Frame {
            stmts: body,
            idx: 0,
            node: None,
            src: Some(src),
            fragment: None,
            locals: Default::default(),
        });
        let coverage = self
            .choice_coverage
            .entry(selected_identity.id.clone())
            .or_insert_with(|| ChoiceCoverage {
                id: selected_identity.id.clone(),
                node: selected_identity.node.clone(),
                label: selected_identity.label.clone(),
                line: selected_identity.line,
                count: 0,
            });
        coverage.count = coverage.count.saturating_add(1);
        self.trace.steps.push(ReplayStep {
            choice: selected_identity,
            observation: None,
        });
        Ok(())
    }

    /// 从头开始(多周目)。
    pub fn restart(&mut self) -> Result<(), RunError> {
        self.failed_explanations = None;
        self.continuation_outputs.clear();
        self.interrupted_outputs.clear();
        self.vars.clear();
        self.visits.clear();
        self.turns = 0;
        self.taken_once.clear();
        self.frames.clear();
        self.glue_pending = false;
        self.paused = None;
        self.met.clear();
        self.anchors.clear();
        self.states.clone_from(&self.initial_states);
        self.state_history.clear();
        self.action_capture = Default::default();
        self.choice_coverage.clear();
        self.rng.set(self.seed);
        self.init_vars()?;
        self.enter_event(&self.program.entry)?;
        self.trace = ReplayTrace::entry(self.fingerprint, self.seed);
        Ok(())
    }

    pub(super) fn current_event_name(&self) -> Option<String> {
        self.frames.first().and_then(|frame| frame.node.clone())
    }

    pub(super) fn choice_id(&self, frame_depth: usize, start: usize, offset: usize) -> String {
        if let Some(id) = self.fragment_choice_id(frame_depth, start, offset) {
            return id;
        }
        let node = self.frames[frame_depth]
            .node
            .clone()
            .or_else(|| self.current_node())
            .unwrap_or_else(|| "?".into());
        format!("{node}:{start}:{offset}")
    }

    pub(super) fn choice_identity(
        &self,
        frame_depth: usize,
        start: usize,
        offset: usize,
        label: String,
    ) -> ChoiceIdentity {
        let frame = &self.frames[frame_depth];
        let Some(Stmt::Choice(choice)) = frame.stmts.get(start + offset) else {
            return ChoiceIdentity {
                id: "invalid-choice".into(),
                node: self.current_node().unwrap_or_default(),
                line: 0,
                offset,
                label,
            };
        };
        let node = frame
            .node
            .clone()
            .or_else(|| self.current_node())
            .unwrap_or_else(|| "?".into());
        let signature = choice_signature(choice);
        let occurrence = frame.stmts[start..start + offset]
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::Choice(previous) if choice_signature(previous) == signature => Some(()),
                _ => None,
            })
            .count();
        ChoiceIdentity {
            id: self
                .fragment_choice_id(frame_depth, start, offset)
                .map(|path| format!("{path}:{:016x}", stable_hash(signature.as_bytes())))
                .unwrap_or_else(|| {
                    format!(
                        "{node}:{:016x}:{occurrence}",
                        stable_hash(signature.as_bytes())
                    )
                }),
            node,
            line: choice.loc.line,
            offset,
            label,
        }
    }
}

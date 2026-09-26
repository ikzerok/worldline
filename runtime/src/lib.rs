//! worldline 运行时 —— 规范见 `worldline/spec/semantics.md`。
//! 确定性状态机:帧栈推进、选择暂停、访问计数、存读档。

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant as MonotonicInstant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant as MonotonicInstant;

use worldline_core::ast::{
    BinOp, Change, ChangeKind, DivertTarget, EffectWhen, Expr, Program, Stmt, TextPart, UnOp,
};
use worldline_core::Analysis;

mod model;
mod replay;

pub use model::{AnchorKind, AnchorRecord, ChoiceView, Output, RunError, StateRecord, Value};
use model::{FrameSave, FrameSrc, SaveState};
pub use replay::{
    AccessCoverage, ChoiceCoverage, ChoiceExplanation, ChoiceIdentity, ConditionExplanation,
    ReplayBudget, ReplayCancellation, ReplayCheckpoint, ReplayObservation, ReplayOrigin,
    ReplayResult, ReplayStatus, ReplayStep, ReplayTrace, REPLAY_SCHEMA_VERSION,
};

// ---------------------------------------------------------------------------
// 帧栈
// ---------------------------------------------------------------------------

struct Frame<'p> {
    stmts: &'p [Stmt],
    idx: usize,
    /// Some = 节点帧(事件或场景全名)。
    node: Option<String>,
    src: Option<FrameSrc>,
}

// ---------------------------------------------------------------------------
// Story
// ---------------------------------------------------------------------------

struct Pause {
    frame_depth: usize,
    start: usize,
    group_len: usize,
    choices: Vec<ChoiceView>,
    explanations: Vec<ChoiceExplanation>,
    rng_before: u64,
}

struct ContinueOutcome {
    outputs: Vec<Output>,
    stop: Option<ReplayStatus>,
}

struct ReplayExecutionBudget<'a> {
    limits: ReplayBudget,
    cancellation: &'a ReplayCancellation,
    started: MonotonicInstant,
    steps: u64,
}

impl ReplayExecutionBudget<'_> {
    fn consume_step(&mut self) -> Result<(), ReplayStatus> {
        if self.cancellation.is_cancelled() {
            return Err(ReplayStatus::Cancelled);
        }
        if self.started.elapsed() >= Duration::from_millis(self.limits.time_budget_ms) {
            return Err(ReplayStatus::TimeBudgetExceeded);
        }
        if self.steps >= self.limits.max_steps {
            return Err(ReplayStatus::StepBudgetExceeded);
        }
        self.steps += 1;
        Ok(())
    }
}

/// 故事实例:消费 Program,持全部可变状态。
pub struct Story<'p> {
    program: &'p Program,
    symbols: &'p worldline_core::Symbols,
    vars: HashMap<String, Value>,
    visits: HashMap<String, u32>,
    turns: u32,
    taken_once: Vec<String>,
    frames: Vec<Frame<'p>>,
    glue_pending: bool,
    paused: Option<Box<Pause>>,
    rng: Cell<u64>,
    seed: u64,
    fingerprint: u64,
    // v1.5 状态
    storyline: String,
    met: HashSet<String>,
    anchors: Vec<AnchorRecord>,
    initial_states: BTreeMap<String, Vec<String>>,
    states: BTreeMap<String, Vec<String>>,
    state_history: Vec<StateRecord>,
    choice_coverage: BTreeMap<String, ChoiceCoverage>,
    trace: ReplayTrace,
}

impl<'p> Story<'p> {
    /// 新建故事(调用方须保证编译无 error 诊断)。
    pub fn new(program: &'p Program, analysis: &'p Analysis) -> Result<Self, RunError> {
        Self::new_with_seed(program, analysis, seed_now())
    }

    /// 使用显式随机种子新建故事，供可复现测试、调试和重放使用。
    pub fn new_with_seed(
        program: &'p Program,
        analysis: &'p Analysis,
        seed: u64,
    ) -> Result<Self, RunError> {
        if program.events.is_empty() {
            return Err(RunError::new("工程没有可运行入口"));
        }
        let seed = normalize_seed(seed);
        let entry_idx = analysis
            .symbols
            .events
            .get(&program.entry)
            .map(|p| p.event)
            .unwrap_or(0);
        let storyline = program
            .events
            .get(entry_idx)
            .map(|e| e.storyline.clone())
            .unwrap_or_else(|| "main".into());
        let initial_states = initial_states(analysis);
        let mut story = Story {
            program,
            symbols: &analysis.symbols,
            vars: HashMap::new(),
            visits: HashMap::new(),
            turns: 0,
            taken_once: Vec::new(),
            frames: Vec::new(),
            glue_pending: false,
            paused: None,
            rng: Cell::new(seed),
            seed,
            fingerprint: analysis.fingerprint,
            storyline,
            met: HashSet::new(),
            anchors: Vec::new(),
            states: initial_states.clone(),
            initial_states,
            state_history: Vec::new(),
            choice_coverage: BTreeMap::new(),
            trace: ReplayTrace::entry(analysis.fingerprint, seed),
        };
        story.init_vars()?;
        story.enter_event(&program.entry)?;
        Ok(story)
    }

    fn init_vars(&mut self) -> Result<(), RunError> {
        for l in &self.program.lets {
            let v = self.eval(&l.expr).map_err(|e| RunError {
                message: format!("初始化变量 `{}` 失败:{}", l.name, e.message),
                node: None,
                line: Some(l.loc.line),
            })?;
            self.vars.insert(l.name.clone(), v);
        }
        Ok(())
    }

    /// 进入事件:准入校验 → 故事线归属 → 计数 → 帧链 → enter 效果。
    fn enter_event(&mut self, name: &str) -> Result<(), RunError> {
        let Some(path) = self.symbols.events.get(name).cloned() else {
            return Err(RunError::new(format!("入口事件 `{name}` 不存在")));
        };
        self.admit(path.event)?;
        self.storyline = self.program.events[path.event].storyline.clone();
        let chain = self.node_frames(path.event, &path.scenes);
        let root_name = chain[0].node.clone().unwrap_or_default();
        *self.visits.entry(root_name).or_insert(0) += 1;
        self.frames = chain;
        self.run_effects(path.event, EffectWhen::Enter)?;
        Ok(())
    }

    /// 构造节点帧链:父帧 idx 指向对应 Scene 语句之后。
    fn node_frames(&self, event_idx: usize, scenes: &[String]) -> Vec<Frame<'p>> {
        let event = &self.program.events[event_idx];
        let mut frames = vec![Frame {
            stmts: &event.body,
            idx: 0,
            node: Some(event.name.clone()),
            src: None,
        }];
        let mut body: &[Stmt] = &event.body;
        let mut prefix = event.name.clone();
        for leaf in scenes {
            let Some(pos) = body
                .iter()
                .position(|s| matches!(s, Stmt::Scene(sc) if sc.name == *leaf))
            else {
                break;
            };
            let Stmt::Scene(sc) = &body[pos] else {
                unreachable!()
            };
            frames.last_mut().expect("根帧必然存在").idx = pos + 1;
            prefix = format!("{prefix}.{leaf}");
            frames.push(Frame {
                stmts: &sc.body,
                idx: 0,
                node: Some(prefix.clone()),
                src: None,
            });
            body = &sc.body;
        }
        frames
    }

    // -- 状态查询 -----------------------------------------------------------

    pub fn is_ended(&self) -> bool {
        self.frames.is_empty() && self.paused.is_none()
    }

    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    pub fn choices(&self) -> &[ChoiceView] {
        self.paused
            .as_ref()
            .map(|p| p.choices.as_slice())
            .unwrap_or(&[])
    }

    pub fn turns(&self) -> u32 {
        self.turns
    }

    pub fn vars(&self) -> &HashMap<String, Value> {
        &self.vars
    }

    pub fn visits(&self) -> &HashMap<String, u32> {
        &self.visits
    }

    /// 当前节点名(最内层节点帧)。
    pub fn current_node(&self) -> Option<String> {
        self.frames.iter().rev().find_map(|f| f.node.clone())
    }

    /// 主角当前故事线。
    pub fn storyline(&self) -> &str {
        &self.storyline
    }

    /// 锚点记录(按发生序)。
    pub fn anchors(&self) -> &[AnchorRecord] {
        &self.anchors
    }

    /// 当前各状态的标签集合。
    pub fn states(&self) -> &BTreeMap<String, Vec<String>> {
        &self.states
    }

    /// 状态替换记录，按实际发生顺序排列。
    pub fn state_history(&self) -> &[StateRecord] {
        &self.state_history
    }

    /// 已获权限(排序)。
    pub fn perm_list(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .program
            .permission_migration
            .as_ref()
            .map(|m| {
                self.states
                    .get(&m.state)
                    .into_iter()
                    .flatten()
                    .filter_map(|tag| m.permission(tag).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// 已登场人物(排序)。
    pub fn met_list(&self) -> Vec<String> {
        let mut v: Vec<String> = self.met.iter().cloned().collect();
        v.sort();
        v
    }

    /// 全量状态视图(规范 agent-protocol.md §2.5):
    /// CLI `play --json` 与 `wl-agent` 共用的机器快照,不含选项列表。
    pub fn state_view(&self) -> serde_json::Value {
        serde_json::json!({
            "turns": self.turns,
            "storyline": self.storyline,
            "current_node": self.current_node(),
            "vars": self.vars,
            "visits": self.visits,
            "perms": self.perm_list(),
            "met": self.met_list(),
            "anchors": self.anchors,
            "states": self.states,
            "state_history": self.state_history,
            "coverage": self.access_coverage(),
            "paused": self.is_paused(),
            "ended": self.is_ended(),
        })
    }

    /// 已实际访问节点与选择的覆盖投影；未访问项目不表示不可达。
    pub fn access_coverage(&self) -> AccessCoverage {
        AccessCoverage {
            visited_nodes: self
                .visits
                .iter()
                .map(|(key, value)| (key.clone(), *value))
                .collect(),
            selected_choices: self.choice_coverage.values().cloned().collect(),
        }
    }

    /// 当前捕获的可序列化重放轨迹。
    pub fn replay_trace(&self) -> ReplayTrace {
        self.trace.clone()
    }

    /// 从当前状态开始新的 trace，并把完整 runtime 存档记录为 checkpoint origin。
    pub fn start_trace_from_here(&mut self) -> Result<(), RunError> {
        let checkpoint = self.checkpoint()?;
        self.trace = ReplayTrace::checkpoint(checkpoint);
        if self.paused.is_some() {
            self.trace.initial_observation = Some(self.observation(&[]));
        }
        Ok(())
    }

    /// Explain the current choice group without changing runtime state or its random stream.
    pub fn explain_choices(&self) -> Result<Vec<ChoiceExplanation>, RunError> {
        if let Some(pause) = &self.paused {
            return Ok(pause.explanations.clone());
        }
        let Some(fi) = self.frames.len().checked_sub(1) else {
            return Ok(Vec::new());
        };
        let frame = &self.frames[fi];
        let start = frame.idx;
        let Some(Stmt::Choice(_)) = frame.stmts.get(start) else {
            return Ok(Vec::new());
        };
        let mut rng = self.rng.get();
        let mut explanations = Vec::new();
        let mut offset = 0;
        while let Some(Stmt::Choice(choice)) = frame.stmts.get(start + offset) {
            let identity = self.choice_identity(fi, start, offset, choice.label_raw.clone());
            let condition = choice.cond.as_ref().map(|expression| {
                match self.eval_with_rng(expression, &mut rng) {
                    Ok(Value::Bool(result)) => ConditionExplanation {
                        expression: expression_source(expression),
                        result: Some(result),
                        error: None,
                    },
                    Ok(_) => ConditionExplanation {
                        expression: expression_source(expression),
                        result: Some(false),
                        error: None,
                    },
                    Err(error) => ConditionExplanation {
                        expression: expression_source(expression),
                        result: None,
                        error: Some(error.message),
                    },
                }
            });
            let condition_failed = condition
                .as_ref()
                .is_some_and(|value| value.result != Some(true));
            let already_taken =
                choice.once && self.taken_once.contains(&self.choice_id(fi, start, offset));
            let unavailable_reason = if condition
                .as_ref()
                .is_some_and(|value| value.error.is_some())
            {
                Some("条件求值失败".into())
            } else if condition
                .as_ref()
                .is_some_and(|value| value.result == Some(false))
            {
                Some("条件求值为 false".into())
            } else if already_taken {
                Some("once 选择已使用".into())
            } else {
                None
            };
            explanations.push(ChoiceExplanation {
                choice: identity,
                available: !condition_failed && !already_taken,
                condition,
                unavailable_reason,
            });
            offset += 1;
        }
        Ok(explanations)
    }

    fn observation(&self, outputs: &[Output]) -> ReplayObservation {
        let choices = self
            .paused
            .as_ref()
            .into_iter()
            .flat_map(|pause| pause.explanations.iter())
            .filter(|explanation| explanation.available)
            .map(|explanation| explanation.choice.clone())
            .collect();
        ReplayObservation {
            outputs: outputs
                .iter()
                .map(|output| serde_json::to_value(output).unwrap_or(serde_json::Value::Null))
                .collect(),
            choices,
            state: self.state_view(),
        }
    }

    fn record_continuation(&mut self, outputs: &[Output]) {
        let observation = self.observation(outputs);
        if self.trace.initial_observation.is_none() {
            self.trace.initial_observation = Some(observation.clone());
        } else if let Some(step) = self.trace.steps.last_mut() {
            if step.observation.is_none() {
                step.observation = Some(observation);
            }
        }
        self.trace.complete = self.is_ended();
    }

    /// 创建绑定 runtime/schema 与程序 fingerprint 的调试检查点。
    pub fn checkpoint(&self) -> Result<ReplayCheckpoint, RunError> {
        let mut state: serde_json::Value = serde_json::from_str(&self.save()?)
            .map_err(|error| RunError::new(format!("检查点状态编码失败:{error}")))?;
        // 暂停组条件和标签的随机表达式在初次呈现时已消耗 RNG；恢复时从组开始状态
        // 重算，保证同一个检查点重新呈现同一组选择。
        if let Some(pause) = &self.paused {
            state["rng"] = serde_json::json!(pause.rng_before);
        }
        let state = serde_json::to_string(&state)
            .map_err(|error| RunError::new(format!("检查点序列化失败:{error}")))?;
        Ok(ReplayCheckpoint {
            schema_version: REPLAY_SCHEMA_VERSION,
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            fingerprint: self.fingerprint,
            seed: self.seed,
            state,
        })
    }

    /// 从严格匹配版本和 fingerprint 的检查点恢复 Story。
    pub fn from_checkpoint(
        program: &'p Program,
        analysis: &'p Analysis,
        checkpoint: &ReplayCheckpoint,
    ) -> Result<Self, RunError> {
        if checkpoint.schema_version != REPLAY_SCHEMA_VERSION {
            return Err(RunError::new("检查点 schema_version 不兼容"));
        }
        if checkpoint.runtime_version != env!("CARGO_PKG_VERSION") {
            return Err(RunError::new("检查点 runtime_version 不兼容"));
        }
        if checkpoint.fingerprint != analysis.fingerprint {
            return Err(RunError::new("检查点程序 fingerprint 不匹配"));
        }
        Self::load(program, analysis, &checkpoint.state)
    }

    // -- v1.5:准入、效果、变动 ------------------------------------------------

    /// 事件准入闸门:perm 权限 + after 前置(规范 semantics.md §8.2)。
    fn admit(&self, event_idx: usize) -> Result<(), RunError> {
        let ev = &self.program.events[event_idx];
        if let Some(m) = &self.program.permission_migration {
            if let Some(tag) = m.gates.get(&ev.name) {
                // 作者删除或改写准入表达式后，旧迁移元数据不得继续施加隐藏闸门。
                let gate = match ev.after.as_ref() {
                    Some(Expr::Binary {
                        op: BinOp::And,
                        lhs,
                        ..
                    }) => Some(lhs.as_ref()),
                    other => other,
                };
                let applies = matches!(gate, Some(Expr::Call { name, args, .. })
                    if name == "has" && matches!(args.as_slice(), [state, value]
                        if static_name(state) == Some(m.state.as_str()) && static_name(value) == Some(tag.as_str())));
                if applies
                    && !self
                        .states
                        .get(&m.state)
                        .is_some_and(|tags| tags.contains(tag))
                {
                    return Err(RunError {
                        message: format!(
                            "无法进入节点 `{}`:缺少身份权限标签 `{}`",
                            ev.name,
                            m.permission(tag).unwrap_or(tag)
                        ),
                        node: self.current_node(),
                        line: Some(ev.loc.line),
                    });
                }
            }
        }
        if let Some(cond) = &ev.after {
            match self.eval(cond)? {
                Value::Bool(true) => {}
                Value::Bool(false) => {
                    return Err(RunError {
                        message: format!(
                            "无法进入节点 `{}`:前置条件未满足(after，含身份权限状态要求)",
                            ev.name
                        ),
                        node: self.current_node(),
                        line: Some(ev.loc.line),
                    });
                }
                other => {
                    return Err(RunError {
                        message: format!("前置条件结果不是布尔:{}", other.kind_label()),
                        node: self.current_node(),
                        line: Some(ev.loc.line),
                    });
                }
            }
        }
        Ok(())
    }

    /// 执行某事件的指定时机效果(条件为假整块跳过)。
    fn run_effects(&mut self, event_idx: usize, when: EffectWhen) -> Result<(), RunError> {
        let effects = self.program.events[event_idx].effects.clone();
        for fx in effects {
            if fx.when != when {
                continue;
            }
            if let Some(cond) = &fx.cond {
                if !matches!(self.eval(cond)?, Value::Bool(true)) {
                    continue;
                }
            }
            for a in &fx.actions {
                self.apply_change(a)?;
            }
        }
        Ok(())
    }

    /// 保留源节点上下文执行离开效果；失败后终止，避免重复执行已发生的动作。
    fn run_exit_effects(&mut self, natural: bool) -> Result<(), RunError> {
        let Some(idx) = self
            .current_event_name()
            .and_then(|name| self.symbols.events.get(&name).map(|path| path.event))
        else {
            return Ok(());
        };
        let result = if natural {
            self.run_effects(idx, EffectWhen::Done)
        } else {
            Ok(())
        }
        .and_then(|()| self.run_effects(idx, EffectWhen::Exit));
        if result.is_err() {
            self.frames.clear();
        }
        result
    }

    /// 应用一次变动并写入对应历史。
    fn apply_change(&mut self, a: &Change) -> Result<(), RunError> {
        match a.kind {
            ChangeKind::Become | ChangeKind::AddTags | ChangeKind::RemoveTags => {
                let before = self.states.get(&a.id).cloned().ok_or_else(|| RunError {
                    message: format!("状态 `{}` 未定义", a.id),
                    node: self.current_node(),
                    line: Some(a.loc.line),
                })?;
                let mut after = match a.kind {
                    ChangeKind::AddTags => {
                        unique_tags(&[before.as_slice(), a.tags.as_slice()].concat())
                    }
                    ChangeKind::RemoveTags => before
                        .iter()
                        .filter(|t| !a.tags.contains(t))
                        .cloned()
                        .collect(),
                    _ => unique_tags(&a.tags),
                };
                after.sort();
                self.states.insert(a.id.clone(), after.clone());
                self.state_history.push(StateRecord {
                    kind: a.kind,
                    state: a.id.clone(),
                    before,
                    after,
                    event: self.current_event_name(),
                    node: self.current_node(),
                    note: a.note.clone(),
                    turn: self.turns,
                });
                // 旧锚点机器字段派生自身份状态动作，绝不另存一份权限集合。
                if let Some(m) = &self.program.permission_migration {
                    if a.id == m.state
                        && matches!(a.kind, ChangeKind::AddTags | ChangeKind::RemoveTags)
                    {
                        let permissions: Vec<_> = a
                            .tags
                            .iter()
                            .filter_map(|t| m.permission(t).map(str::to_string))
                            .collect();
                        for p in permissions {
                            let (kind, name) = if a.kind == ChangeKind::AddTags {
                                (AnchorKind::Grant, "权限授予")
                            } else {
                                (AnchorKind::Revoke, "权限吊销")
                            };
                            self.push_anchor(kind, name, a.note.clone(), Some(p));
                        }
                    }
                }
            }
            ChangeKind::Grant | ChangeKind::Revoke => {
                return Err(RunError::new("旧权限动作尚未由编译器归一化"));
            }
            ChangeKind::Meet => {
                self.met.insert(a.id.clone());
                self.push_anchor(
                    AnchorKind::Meet,
                    "人物登场",
                    a.note.clone(),
                    Some(a.id.clone()),
                );
            }
            ChangeKind::Part => {
                self.met.remove(&a.id);
                self.push_anchor(
                    AnchorKind::Part,
                    "人物离场",
                    a.note.clone(),
                    Some(a.id.clone()),
                );
            }
            ChangeKind::To => {
                let Some(sl) = a.to_storyline.clone() else {
                    return Err(RunError {
                        message: "to 动作缺少目标故事线".into(),
                        node: self.current_node(),
                        line: Some(a.loc.line),
                    });
                };
                self.storyline = sl.clone();
                self.push_anchor(AnchorKind::Shift, "主线变动", a.note.clone(), Some(sl));
            }
        }
        Ok(())
    }

    fn push_anchor(
        &mut self,
        kind: AnchorKind,
        name: &str,
        note: Option<String>,
        detail: Option<String>,
    ) {
        self.anchors.push(AnchorRecord {
            kind,
            name: name.to_string(),
            note,
            detail,
            node: self.current_node(),
            storyline: self.storyline.clone(),
            turn: self.turns,
        });
    }

    // -- 推进 ---------------------------------------------------------------

    /// 推进到暂停(选择)或结束;返回本轮输出。
    pub fn continue_story(&mut self) -> Result<Vec<Output>, RunError> {
        let outcome = self.continue_story_inner(None)?;
        self.record_continuation(&outcome.outputs);
        Ok(outcome.outputs)
    }

    fn continue_story_inner(
        &mut self,
        mut budget: Option<&mut ReplayExecutionBudget<'_>>,
    ) -> Result<ContinueOutcome, RunError> {
        let mut out = Vec::new();
        if self.paused.is_some() {
            return Ok(ContinueOutcome {
                outputs: out,
                stop: None,
            });
        }
        loop {
            if let Some(run_budget) = budget.as_deref_mut() {
                if let Err(status) = run_budget.consume_step() {
                    return Ok(ContinueOutcome {
                        outputs: out,
                        stop: Some(status),
                    });
                }
            }
            let Some(fi) = self.frames.len().checked_sub(1) else {
                out.push(Output::Ended);
                return Ok(ContinueOutcome {
                    outputs: out,
                    stop: None,
                });
            };
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
                Stmt::Text(t) => {
                    let (content, links) = self.render_parts(&t.parts)?;
                    let tags = t.tags.clone();
                    let new_line = !self.glue_pending;
                    self.glue_pending = false;
                    if !content.is_empty() {
                        out.push(Output::Text {
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
                    let v = self.eval(&l.expr)?;
                    self.vars.insert(l.name.clone(), v);
                    self.frames[fi].idx += 1;
                }
                Stmt::Set(s) => {
                    let v = self.eval(&s.expr)?;
                    self.vars.insert(s.name.clone(), v);
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
                    });
                }
                Stmt::Divert(d) => {
                    match &d.target {
                        DivertTarget::End => {
                            self.run_exit_effects(false)?;
                            self.frames.clear();
                            out.push(Output::Ended);
                            return Ok(ContinueOutcome {
                                outputs: out,
                                stop: None,
                            });
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
                    self.apply_change(&c.change)?;
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
                    // 选择组 = 连续 Choice 语句
                    let stmts = self.frames[fi].stmts;
                    let start = self.frames[fi].idx;
                    let mut group_len = 0usize;
                    while matches!(stmts.get(start + group_len), Some(Stmt::Choice(_))) {
                        group_len += 1;
                    }
                    let mut choices = Vec::new();
                    let mut explanations = Vec::with_capacity(group_len);
                    let mut offset = 0usize;
                    let rng_before = self.rng.get();
                    while let Some(Stmt::Choice(c)) = stmts.get(start + offset) {
                        let mut identity =
                            self.choice_identity(fi, start, offset, c.label_raw.clone());
                        let condition = if let Some(cond) = &c.cond {
                            let result = matches!(self.eval(cond)?, Value::Bool(true));
                            Some(ConditionExplanation {
                                expression: expression_source(cond),
                                result: Some(result),
                                error: None,
                            })
                        } else {
                            None
                        };
                        if condition
                            .as_ref()
                            .is_some_and(|value| value.result == Some(false))
                        {
                            explanations.push(ChoiceExplanation {
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
                                    choice: identity,
                                    available: false,
                                    condition,
                                    unavailable_reason: Some("once 选择已使用".into()),
                                });
                                offset += 1;
                                continue;
                            }
                        }
                        let (label, links) = self.render_parts(&c.label)?;
                        identity.label = label.clone();
                        choices.push(ChoiceView {
                            id: identity.id.clone(),
                            label,
                            links,
                            line: c.loc.line,
                            offset,
                        });
                        explanations.push(ChoiceExplanation {
                            choice: identity,
                            available: true,
                            condition,
                            unavailable_reason: None,
                        });
                        offset += 1;
                    }
                    if choices.is_empty() {
                        // 组耗尽:落穿到组后(隐式汇聚)
                        self.frames[fi].idx = start + group_len;
                        continue;
                    }
                    self.paused = Some(Box::new(Pause {
                        frame_depth: fi,
                        start,
                        group_len,
                        choices,
                        explanations,
                        rng_before,
                    }));
                    return Ok(ContinueOutcome {
                        outputs: out,
                        stop: None,
                    });
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
        self.choice_coverage.clear();
        self.rng.set(self.seed);
        self.init_vars()?;
        self.enter_event(&self.program.entry)?;
        self.trace = ReplayTrace::entry(self.fingerprint, self.seed);
        Ok(())
    }

    fn current_event_name(&self) -> Option<String> {
        self.frames.first().and_then(|frame| frame.node.clone())
    }

    fn choice_id(&self, frame_depth: usize, start: usize, offset: usize) -> String {
        let node = self.frames[frame_depth]
            .node
            .clone()
            .or_else(|| self.current_node())
            .unwrap_or_else(|| "?".into());
        format!("{node}:{start}:{offset}")
    }

    fn choice_identity(
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
            id: format!(
                "{node}:{:016x}:{occurrence}",
                stable_hash(signature.as_bytes())
            ),
            node,
            line: choice.loc.line,
            offset,
            label,
        }
    }

    // -- 求值 ---------------------------------------------------------------

    fn render_parts(
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

    fn eval(&self, e: &Expr) -> Result<Value, RunError> {
        let mut rng = self.rng.get();
        let result = self.eval_with_rng(e, &mut rng);
        self.rng.set(rng);
        result
    }

    fn eval_with_rng(&self, e: &Expr, rng: &mut u64) -> Result<Value, RunError> {
        match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Var { name, loc } => self.vars.get(name).cloned().ok_or_else(|| RunError {
                message: format!("变量 `{name}` 未定义"),
                node: self.current_node(),
                line: Some(loc.line),
            }),
            Expr::Unary { op, expr } => {
                let v = self.eval_with_rng(expr, rng)?;
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
                let l = self.eval_with_rng(lhs, rng)?;
                let r = self.eval_with_rng(rhs, rng)?;
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
                    let (Value::Num(lo), Value::Num(hi)) =
                        (self.eval_with_rng(a, rng)?, self.eval_with_rng(b, rng)?)
                    else {
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
        }
    }

    // -- 存读档 ---------------------------------------------------------------

    /// 序列化当前状态为 JSON(规范 semantics.md §7)。
    pub fn save(&self) -> Result<String, RunError> {
        let state = SaveState {
            fingerprint: self.fingerprint,
            vars: self.vars.clone(),
            visits: self.visits.clone(),
            turns: self.turns,
            taken_once: self.taken_once.clone(),
            frames: self
                .frames
                .iter()
                .map(|f| FrameSave {
                    node: f.node.clone(),
                    idx: f.idx,
                    src: f.src,
                })
                .collect(),
            glue_pending: self.glue_pending,
            paused: self.paused.is_some(),
            rng: self.rng.get(),
            seed: self.seed,
            choice_coverage: self.choice_coverage.clone(),
            storyline: self.storyline.clone(),
            perms: None,
            met: self.met_list(),
            anchors: self.anchors.clone(),
            states: self.states.clone(),
            state_history: self.state_history.clone(),
        };
        serde_json::to_string_pretty(&state)
            .map_err(|e| RunError::new(format!("存档序列化失败:{e}")))
    }

    /// 从 JSON 恢复(指纹不匹配视为不兼容)。
    pub fn load(
        program: &'p Program,
        analysis: &'p Analysis,
        json: &str,
    ) -> Result<Self, RunError> {
        if program.events.is_empty() {
            return Err(RunError::new("工程没有可运行入口"));
        }
        let mut state: SaveState =
            serde_json::from_str(json).map_err(|e| RunError::new(format!("存档解析失败:{e}")))?;
        let legacy = state.fingerprint != analysis.fingerprint;
        if legacy
            && !program
                .permission_migration
                .as_ref()
                .is_some_and(|m| m.accepts_legacy(state.fingerprint, analysis.fingerprint))
        {
            return Err(RunError::new(
                "程序内容已变化,存档不兼容(改稿后旧档不保证可续玩)",
            ));
        }
        // 旧档权限注入身份状态；新档的 perms 只是派生快照，冲突必须显式拒绝。
        if let Some(m) = &program.permission_migration {
            let tags: Vec<_> = state
                .perms
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|p| {
                    m.tags.get(p).cloned().ok_or_else(|| {
                        RunError::new(format!("旧存档权限 `{p}` 无法映射为状态标签"))
                    })
                })
                .collect::<Result<_, _>>()?;
            let mut tags = unique_tags(&tags);
            tags.sort();
            if legacy {
                if state.states.contains_key(&m.state) {
                    return Err(RunError::new(
                        "旧存档同时包含迁移身份状态，无法确定权限来源",
                    ));
                }
                state.states.insert(m.state.clone(), tags);
            } else if state.perms.is_some() {
                let current: Vec<_> = state
                    .states
                    .get(&m.state)
                    .into_iter()
                    .flatten()
                    .filter(|t| m.permission(t).is_some())
                    .cloned()
                    .collect();
                if unique_tags(&current) != tags {
                    return Err(RunError::new("存档的权限兼容字段与身份状态不一致"));
                }
            }
        } else if state.perms.as_ref().is_some_and(|p| !p.is_empty()) {
            return Err(RunError::new("存档含有旧权限，但程序没有身份状态映射"));
        }
        for id in state.states.keys() {
            if !analysis.catalog.states.contains_key(id) {
                return Err(RunError::new(format!("存档包含未定义状态 `{id}`")));
            }
        }
        for id in analysis.catalog.states.keys() {
            if !state.states.contains_key(id) {
                return Err(RunError::new(format!("存档缺少状态 `{id}`，拒绝静默重置")));
            }
        }
        let entry_idx = analysis
            .symbols
            .events
            .get(&program.entry)
            .map(|p| p.event)
            .unwrap_or(0);
        let entry_storyline = program
            .events
            .get(entry_idx)
            .map(|e| e.storyline.clone())
            .unwrap_or_else(|| "main".into());
        let seed = normalize_seed(if state.seed == 0 {
            state.rng
        } else {
            state.seed
        });
        state.seed = seed;
        let saved_frames = state.frames.clone();
        let restore_pause = state.paused;
        let checkpoint = ReplayCheckpoint {
            schema_version: REPLAY_SCHEMA_VERSION,
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            fingerprint: analysis.fingerprint,
            seed,
            state: serde_json::to_string(&state)
                .map_err(|error| RunError::new(format!("存档序列化失败:{error}")))?,
        };
        let mut story = Story {
            program,
            symbols: &analysis.symbols,
            vars: state.vars,
            visits: state.visits,
            turns: state.turns,
            taken_once: state.taken_once,
            frames: Vec::new(),
            glue_pending: state.glue_pending,
            paused: None,
            rng: Cell::new(state.rng),
            seed,
            fingerprint: analysis.fingerprint,
            storyline: if state.storyline.is_empty() {
                entry_storyline
            } else {
                state.storyline
            },
            met: state.met.into_iter().collect(),
            anchors: state.anchors,
            initial_states: initial_states(analysis),
            states: state.states,
            state_history: state.state_history,
            choice_coverage: state.choice_coverage,
            trace: ReplayTrace::checkpoint(checkpoint),
        };
        story.frames = story.rebuild_frames(&saved_frames)?;
        if restore_pause {
            let _ = story.continue_story()?;
            if !story.is_paused() {
                return Err(RunError::new("存档标记为暂停，但无法重建选择组"));
            }
        }
        Ok(story)
    }

    fn rebuild_frames(&self, saves: &[FrameSave]) -> Result<Vec<Frame<'p>>, RunError> {
        let mut frames: Vec<Frame<'p>> = Vec::new();
        for sv in saves {
            match &sv.node {
                Some(name) => {
                    if frames.is_empty() {
                        // 根帧:名字解析出完整链
                        let Some(path) = self.symbols.resolve_node(name) else {
                            return Err(RunError::new(format!("存档引用的节点 `{name}` 不存在")));
                        };
                        let mut chain = self.node_frames(path.event, &path.scenes);
                        if let Some(last) = chain.last_mut() {
                            last.idx = sv.idx;
                        }
                        frames.extend(chain);
                    } else {
                        // 深一层场景帧:父帧 stmts 中定位 Scene 语句
                        let leaf = name.rsplit('.').next().unwrap_or(name);
                        let parent_stmts = frames.last().expect("父帧存在").stmts;
                        let Some(pos) = frames
                            .last()
                            .map(|f| f.idx)
                            .and_then(|idx| idx.checked_sub(1))
                            .filter(|&p| matches!(parent_stmts.get(p), Some(Stmt::Scene(sc)) if sc.name == leaf))
                        else {
                            return Err(RunError::new(format!("存档与程序结构不符(场景 `{name}`)")));
                        };
                        let Stmt::Scene(sc) = &parent_stmts[pos] else {
                            unreachable!()
                        };
                        frames.push(Frame {
                            stmts: &sc.body,
                            idx: sv.idx,
                            node: Some(name.clone()),
                            src: None,
                        });
                    }
                }
                None => {
                    let Some(src) = &sv.src else {
                        return Err(RunError::new("存档缺少内联帧来源"));
                    };
                    let parent = frames
                        .last()
                        .ok_or_else(|| RunError::new("存档首帧不能是内联帧"))?;
                    let parent_stmts = parent.stmts;
                    let expect_parent_idx = match src {
                        FrameSrc::IfBranch { stmt, .. } => stmt + 1,
                        FrameSrc::ChoiceBody { stmt } => stmt + 1,
                    };
                    if parent.idx != expect_parent_idx {
                        return Err(RunError::new("存档与程序结构不符(内联帧错位)"));
                    }
                    let stmt_idx = expect_parent_idx - 1;
                    let stmts: &'p [Stmt] = match (&parent_stmts[stmt_idx], src) {
                        (Stmt::If(i), FrameSrc::IfBranch { branch, .. }) => {
                            let Some((_, body)) = i.branches.get(*branch) else {
                                return Err(RunError::new("存档与程序结构不符(分支不存在)"));
                            };
                            body
                        }
                        (Stmt::Choice(c), FrameSrc::ChoiceBody { .. }) => &c.body,
                        _ => return Err(RunError::new("存档与程序结构不符(帧来源不匹配)")),
                    };
                    frames.push(Frame {
                        stmts,
                        idx: sv.idx,
                        node: None,
                        src: Some(*src),
                    });
                }
            }
        }
        Ok(frames)
    }
}

impl ReplayTrace {
    /// Replay a recorded input sequence against a compiled story. Entry traces may be
    /// checked against changed source; checkpoint traces require an exact fingerprint.
    pub fn replay(
        program: &Program,
        analysis: &Analysis,
        trace: &ReplayTrace,
        limits: ReplayBudget,
        cancellation: &ReplayCancellation,
    ) -> Result<ReplayResult, RunError> {
        if trace.schema_version != REPLAY_SCHEMA_VERSION {
            return Err(RunError::new("重放 trace schema_version 不兼容"));
        }
        if trace.runtime_version != env!("CARGO_PKG_VERSION") {
            return Err(RunError::new("重放 trace runtime_version 不兼容"));
        }
        let mut story = match &trace.origin {
            ReplayOrigin::Entry { seed } => Story::new_with_seed(program, analysis, *seed)?,
            ReplayOrigin::Checkpoint { checkpoint } => {
                if checkpoint.fingerprint != trace.fingerprint {
                    return Err(RunError::new("trace 与检查点 fingerprint 不一致"));
                }
                Story::from_checkpoint(program, analysis, checkpoint)?
            }
        };
        let mut budget = ReplayExecutionBudget {
            limits,
            cancellation,
            started: MonotonicInstant::now(),
            steps: 0,
        };
        let mut initial_state = story.state_view();

        if cancellation.is_cancelled() {
            return Ok(make_replay_result(
                ReplayStatus::Cancelled,
                0,
                0,
                trace.fingerprint,
                &story,
                &initial_state,
            ));
        }

        let initial_actual = if story.is_paused() {
            story.observation(&[])
        } else {
            match story.continue_story_inner(Some(&mut budget))? {
                outcome => {
                    story.record_continuation(&outcome.outputs);
                    if let Some(status) = outcome.stop {
                        return Ok(make_replay_result(
                            status,
                            budget.steps,
                            0,
                            trace.fingerprint,
                            &story,
                            &initial_state,
                        ));
                    }
                    story.observation(&outcome.outputs)
                }
            }
        };
        initial_state = initial_actual.state.clone();
        if let Some(expected) = &trace.initial_observation {
            if !observations_match(expected, &initial_actual) {
                return Ok(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index: 0,
                        reason: "初始输出、状态或选择组不匹配".into(),
                        expected_choice: None,
                        actual_choices: initial_actual.choices,
                    },
                    budget.steps,
                    0,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
        }

        let mut completed_choices = 0;
        for (step_index, step) in trace.steps.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Ok(make_replay_result(
                    ReplayStatus::Cancelled,
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
            let Some(choice_index) = story
                .choices()
                .iter()
                .position(|choice| choice.id == step.choice.id)
            else {
                return Ok(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index,
                        reason: "记录的选择在当前暂停组中不存在".into(),
                        expected_choice: Some(step.choice.clone()),
                        actual_choices: story.observation(&[]).choices,
                    },
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            };
            if let Err(error) = story.choose(choice_index) {
                return Ok(make_replay_result(
                    ReplayStatus::StoryFailed {
                        message: error.message,
                        node: error.node,
                        line: error.line,
                    },
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
            completed_choices += 1;
            let Some(expected_observation) = &step.observation else {
                return Ok(make_replay_result(
                    ReplayStatus::IncompleteTrace,
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            };
            let outcome = match story.continue_story_inner(Some(&mut budget)) {
                Ok(outcome) => outcome,
                Err(error) => {
                    return Ok(make_replay_result(
                        ReplayStatus::StoryFailed {
                            message: error.message,
                            node: error.node,
                            line: error.line,
                        },
                        budget.steps,
                        completed_choices,
                        trace.fingerprint,
                        &story,
                        &initial_state,
                    ));
                }
            };
            story.record_continuation(&outcome.outputs);
            if let Some(status) = outcome.stop {
                return Ok(make_replay_result(
                    status,
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
            let actual = story.observation(&outcome.outputs);
            if !observations_match(expected_observation, &actual) {
                return Ok(make_replay_result(
                    ReplayStatus::Diverged {
                        step_index: step_index + 1,
                        reason: "选择后的输出、状态或选择组不匹配".into(),
                        expected_choice: Some(step.choice.clone()),
                        actual_choices: actual.choices,
                    },
                    budget.steps,
                    completed_choices,
                    trace.fingerprint,
                    &story,
                    &initial_state,
                ));
            }
        }

        let status = ReplayStatus::Replayed {
            ended: story.is_ended(),
            complete: trace.complete && story.is_ended(),
        };
        Ok(make_replay_result(
            status,
            budget.steps,
            completed_choices,
            trace.fingerprint,
            &story,
            &initial_state,
        ))
    }
}

fn observations_match(expected: &ReplayObservation, actual: &ReplayObservation) -> bool {
    expected.outputs == actual.outputs
        && expected
            .choices
            .iter()
            .map(|choice| (&choice.id, &choice.label))
            .eq(actual
                .choices
                .iter()
                .map(|choice| (&choice.id, &choice.label)))
        && semantic_state(&expected.state) == semantic_state(&actual.state)
}

fn semantic_state(value: &serde_json::Value) -> serde_json::Value {
    let mut value = value.clone();
    if let Some(choices) = value
        .get_mut("coverage")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|coverage| coverage.get_mut("selected_choices"))
        .and_then(serde_json::Value::as_array_mut)
    {
        for choice in choices {
            if let Some(choice) = choice.as_object_mut() {
                choice.remove("line");
            }
        }
    }
    value
}

fn make_replay_result(
    status: ReplayStatus,
    executed_steps: u64,
    completed_choices: usize,
    original_fingerprint: u64,
    story: &Story<'_>,
    initial_state: &serde_json::Value,
) -> ReplayResult {
    let current_state = story.state_view();
    let mut state_diff = BTreeMap::new();
    if let (Some(before), Some(after)) = (initial_state.as_object(), current_state.as_object()) {
        for key in before.keys().chain(after.keys()) {
            let before_value = before.get(key);
            let after_value = after.get(key);
            if before_value != after_value {
                state_diff.insert(
                    key.clone(),
                    after_value.cloned().unwrap_or(serde_json::Value::Null),
                );
            }
        }
    }
    ReplayResult {
        status,
        executed_steps,
        completed_choices,
        source_fingerprint: story.fingerprint,
        original_fingerprint,
        current_node: story.current_node(),
        current_state,
        state_diff,
        coverage: story.access_coverage(),
    }
}

fn static_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Var { name, .. } | Expr::Str(name) => Some(name),
        _ => None,
    }
}

fn unique_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.iter()
        .filter(|tag| seen.insert(*tag))
        .cloned()
        .collect()
}

fn initial_states(analysis: &Analysis) -> BTreeMap<String, Vec<String>> {
    analysis
        .catalog
        .states
        .iter()
        .map(|(id, state)| (id.clone(), unique_tags(&state.tags)))
        .collect()
}

fn normalize_seed(seed: u64) -> u64 {
    if seed == 0 {
        0x9E37_79B9_7F4A_7C15
    } else {
        seed
    }
}

fn next_rnd(rng: &mut u64) -> u64 {
    let mut value = normalize_seed(*rng);
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *rng = value;
    value
}

fn choice_signature(choice: &worldline_core::ast::ChoiceStmt) -> String {
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

fn stable_hash(bytes: &[u8]) -> u64 {
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

fn expression_source(expression: &Expr) -> String {
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

fn num_op(l: &Value, r: &Value, line: u32, f: impl Fn(f64, f64) -> f64) -> Result<Value, RunError> {
    match (l, r) {
        (Value::Num(a), Value::Num(b)) => Ok(Value::Num(f(*a, *b))),
        _ => Err(RunError {
            message: format!("算术运算不接受 {} 与 {}", l.kind_label(), r.kind_label()),
            node: None,
            line: Some(line),
        }),
    }
}

fn cmp_op(
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

fn stmt_line(s: &Stmt) -> u32 {
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

fn expr_loc_line(e: &Expr) -> u32 {
    match e {
        Expr::Var { loc, .. } | Expr::Call { loc, .. } => loc.line,
        Expr::Unary { expr, .. } => expr_loc_line(expr),
        Expr::Binary { lhs, .. } => expr_loc_line(lhs),
        _ => 0,
    }
}

fn seed_now() -> u64 {
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

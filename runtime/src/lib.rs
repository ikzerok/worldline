//! worldline 运行时 —— 规范见 `worldline/spec/semantics.md`。
//! 确定性状态机:帧栈推进、选择暂停、访问计数、存读档。

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};

use worldline_core::ast::{EffectWhen, Program, Stmt, TextPart};
use worldline_core::Analysis;

mod bounded;
mod choices;
mod comparison_boundary;
mod effects;
mod evidence;
mod execution;
mod expression;
mod language;
mod language_expression;
mod language_persistence;
mod model;
mod persistence;
mod random;
mod replay;
mod replay_runner;
mod route_comparison;
mod state_actions;
mod util;
mod variable_validation;

pub use bounded::{
    BoundedContinuation, ContinuationOutcome, BOUNDED_CONTINUE_CAPABILITY,
    DEFAULT_CONTINUATION_BUDGET,
};
pub use choices::CHOICE_PRESENTATION_CAPABILITY;
pub use evidence::{ConditionEvidence, EvidenceNode, EvidenceOutcome};
use model::FrameSrc;
pub use model::{
    AnchorKind, AnchorRecord, ChoicePresentation, ChoiceView, Output, RunError, StateRecord, Value,
};
pub use replay::{
    AccessCoverage, ChoiceCoverage, ChoiceExplanation, ChoiceIdentity, ConditionExplanation,
    ReplayBudget, ReplayCancellation, ReplayCheckpoint, ReplayObservation, ReplayOrigin,
    ReplayResult, ReplayStatus, ReplayStep, ReplayTrace, REPLAY_SCHEMA_VERSION,
};
pub use replay_runner::ReplaySession;
pub use route_comparison::*;
pub use state_actions::{StateActionEvidence, StateActionRecord};
use util::{expression_source, initial_states, normalize_seed, seed_now};

// ---------------------------------------------------------------------------
// 帧栈
// ---------------------------------------------------------------------------

struct Frame<'p> {
    fragment: Option<String>,
    locals: BTreeMap<String, Value>,
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
    presentations: Vec<ChoicePresentation>,
    explanations: Vec<ChoiceExplanation>,
    rng_before: u64,
}

/// 故事实例:消费 Program,持全部可变状态。
pub struct Story<'p> {
    program: &'p Program,
    symbols: &'p worldline_core::Symbols,
    catalog: &'p worldline_core::Catalog,
    state_actions: state_actions::StateActionCapture,
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
    failed_explanations: Option<Vec<ChoiceExplanation>>,
    continuation_budget: ReplayBudget,
    continuation_outputs: Vec<Output>,
    interrupted_outputs: Vec<Output>,
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
            catalog: &analysis.catalog,
            state_actions: Default::default(),
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
            failed_explanations: None,
            continuation_budget: DEFAULT_CONTINUATION_BUDGET,
            continuation_outputs: Vec::new(),
            interrupted_outputs: Vec::new(),
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
            fragment: None,
            locals: BTreeMap::new(),
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
                fragment: None,
                locals: BTreeMap::new(),
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
        self.frames.iter().rev().find_map(|f| {
            f.fragment
                .as_ref()
                .map(|n| format!("fragment:{n}"))
                .or_else(|| f.node.clone())
        })
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
        let mut view = serde_json::json!({
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
        });
        if worldline_core::language::uses_new_features(self.program) {
            view["calls"] = serde_json::json!(self.call_view());
        }
        view
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
        self.continuation_outputs.clear();
        if self.paused.is_some() {
            self.trace.initial_observation = Some(self.observation(&[]));
        }
        Ok(())
    }

    /// Actual evidence only: never evaluates, predicts, or advances the story.
    pub fn choice_evidence(&self) -> Option<&[ChoiceExplanation]> {
        self.paused
            .as_ref()
            .map(|pause| pause.explanations.as_slice())
            .or(self.failed_explanations.as_deref())
    }

    /// Explain the current choice group without changing runtime state or its random stream.
    pub fn explain_choices(&self) -> Result<Vec<ChoiceExplanation>, RunError> {
        if let Some(pause) = &self.paused {
            let mut explanations = pause.explanations.clone();
            for choice in &mut explanations {
                choice.source = None;
                if let Some(condition) = &mut choice.condition {
                    condition.evidence = None;
                }
                if let Some(condition) = &mut choice.enable_condition {
                    condition.evidence = None;
                }
            }
            return Ok(explanations);
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
                        evidence: None,
                    },
                    Ok(_) => ConditionExplanation {
                        expression: expression_source(expression),
                        result: Some(false),
                        error: None,
                        evidence: None,
                    },
                    Err(error) => ConditionExplanation {
                        expression: expression_source(expression),
                        result: None,
                        error: Some(error.message),
                        evidence: None,
                    },
                }
            });
            let condition_failed = condition
                .as_ref()
                .is_some_and(|value| value.result != Some(true));
            let already_taken =
                choice.once && self.taken_once.contains(&self.choice_id(fi, start, offset));
            let mut unavailable_reason = if condition
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
            let visible = !condition_failed && !already_taken;
            let enable_condition = if visible {
                choice.enable.as_ref().map(|expression| {
                    match self.eval_with_rng(expression, &mut rng) {
                        Ok(value) => ConditionExplanation {
                            expression: expression_source(expression),
                            result: Some(matches!(value, Value::Bool(true))),
                            error: None,
                            evidence: None,
                        },
                        Err(error) => ConditionExplanation {
                            expression: expression_source(expression),
                            result: None,
                            error: Some(error.message),
                            evidence: None,
                        },
                    }
                })
            } else {
                None
            };
            let enabled = enable_condition
                .as_ref()
                .is_none_or(|v| v.result == Some(true));
            if visible && !enabled {
                unavailable_reason = Some("可选条件未满足或求值失败".into());
            }
            let available = visible && enabled;
            if visible && !enable_condition.as_ref().is_some_and(|v| v.error.is_some()) {
                for part in &choice.label {
                    if let TextPart::Expr(expression) = part {
                        // Mirror ordinary label rendering on the copied stream so later
                        // conditions see the same random values without consuming Story RNG.
                        self.eval_with_rng(expression, &mut rng)?;
                    }
                }
            }
            explanations.push(ChoiceExplanation {
                source: None,
                choice: identity,
                available,
                condition,
                unavailable_reason,
                enable_condition,
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
            choice_presentation: if choices::uses_presentation(self.program) {
                self.choice_presentations()
                    .iter()
                    .map(|v| serde_json::to_value(v).unwrap())
                    .collect()
            } else {
                Vec::new()
            },
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
        let story = Self::load(program, analysis, &checkpoint.state)?;
        if story.seed != normalize_seed(checkpoint.seed) {
            return Err(RunError::new("检查点 seed 与 runtime 状态不一致"));
        }
        Ok(story)
    }
}

use std::cell::Cell;

use worldline_core::ast::{Program, Stmt};
use worldline_core::Analysis;

use super::model::{FrameSave, FrameSrc, SaveState};
use super::replay::{ReplayCheckpoint, REPLAY_SCHEMA_VERSION};
use super::util::{initial_states, normalize_seed, unique_tags};
use super::{Frame, ReplayTrace, RunError, Story};

impl<'p> Story<'p> {
    // -- 存读档 ---------------------------------------------------------------

    /// 序列化当前状态为 JSON(规范 semantics.md §7)。
    pub fn save(&self) -> Result<String, RunError> {
        let state = SaveState {
            required_features: required_features(self.program),
            fingerprint: self.fingerprint,
            vars: self.vars.clone(),
            visits: self.visits.clone(),
            turns: self.turns,
            taken_once: self.taken_once.clone(),
            frames: self
                .frames
                .iter()
                .map(|f| FrameSave {
                    fragment: f.fragment.clone(),
                    locals: f.locals.clone(),
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
        let value = worldline_core::parse_unique_json(json.as_bytes())
            .map_err(|e| RunError::new(format!("存档解析失败:{e}")))?;
        let mut state: SaveState = serde_json::from_value(value)
            .map_err(|e| RunError::new(format!("存档解析失败:{e}")))?;
        let mut required = required_features(program);
        let mut supplied = state.required_features.clone();
        required.sort();
        supplied.sort();
        if required != supplied {
            return Err(RunError::new("存档必需语言能力缺失或不受支持"));
        }
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
        super::variable_validation::validate_saved_vars(program, analysis, &state.vars)?;
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
        for (id, tags) in &state.states {
            if tags.iter().collect::<std::collections::HashSet<_>>().len() != tags.len() {
                return Err(RunError::new(format!("存档状态 `{id}` 含重复标签")));
            }
            if tags
                .iter()
                .any(|tag| !analysis.catalog.tags.get(tag).is_some_and(|t| t.declared))
            {
                return Err(RunError::new(format!("存档状态 `{id}` 含未知标签身份")));
            }
            if !analysis.catalog.states.contains_key(id) {
                return Err(RunError::new(format!("存档包含未定义状态 `{id}`")));
            }
        }
        for id in analysis.catalog.states.keys() {
            if !state.states.contains_key(id) {
                return Err(RunError::new(format!("存档缺少状态 `{id}`，拒绝静默重置")));
            }
        }
        for record in &state.state_history {
            if !analysis.catalog.states.contains_key(&record.state)
                || !matches!(
                    record.kind,
                    worldline_core::ast::ChangeKind::Become
                        | worldline_core::ast::ChangeKind::AddTags
                        | worldline_core::ast::ChangeKind::RemoveTags
                )
            {
                return Err(RunError::new("存档状态历史包含未知状态或非法操作"));
            }
            for tags in [&record.before, &record.after] {
                if tags
                    .iter()
                    .any(|id| !analysis.catalog.tags.get(id).is_some_and(|t| t.declared))
                    || tags.iter().collect::<std::collections::HashSet<_>>().len() != tags.len()
                {
                    return Err(RunError::new("存档状态历史包含未知或重复标签"));
                }
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
            inspection: Default::default(),
            program,
            symbols: &analysis.symbols,
            catalog: &analysis.catalog,
            action_capture: Default::default(),
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
            failed_explanations: None,
            continuation_budget: super::DEFAULT_CONTINUATION_BUDGET,
            continuation_outputs: Vec::new(),
            interrupted_outputs: Vec::new(),
        };
        story.frames = story.rebuild_frames(&saved_frames, analysis)?;
        for frame in &mut story.frames {
            if frame.fragment.is_some() {
                frame.inspection_call_id = story.inspection.call_id();
            }
        }
        if restore_pause {
            let _ = story.continue_story()?;
            if !story.is_paused() {
                return Err(RunError::new("存档标记为暂停，但无法重建选择组"));
            }
        }
        Ok(story)
    }

    fn rebuild_frames(
        &self,
        saves: &[FrameSave],
        analysis: &Analysis,
    ) -> Result<Vec<Frame<'p>>, RunError> {
        let mut frames: Vec<Frame<'p>> = Vec::new();
        if worldline_core::language::uses_new_features(self.program) && saves.len() > 1024 {
            return Err(RunError::new("存档帧栈超过1024层"));
        }
        for sv in saves {
            if let Some(frame) = self.rebuild_fragment_frame(sv, &frames, analysis)? {
                frames.push(frame);
                continue;
            }
            if !sv.locals.is_empty() {
                return Err(RunError::new("非片段帧不能携带局部值"));
            }
            match &sv.node {
                Some(name) => {
                    if sv.src.is_some() {
                        return Err(RunError::new("节点帧不能携带内联调用来源"));
                    }
                    if frames.is_empty() {
                        // 根帧:名字解析出完整链
                        let Some(path) = self.symbols.resolve_node(name) else {
                            return Err(RunError::new(format!("存档引用的节点 `{name}` 不存在")));
                        };
                        if !path.scenes.is_empty() {
                            return Err(RunError::new("存档根帧必须是事件"));
                        }
                        let mut chain = self.node_frames(path.event, &path.scenes);
                        if let Some(last) = chain.last_mut() {
                            last.idx = sv.idx;
                        }
                        frames.extend(chain);
                    } else {
                        // 深一层场景帧:父帧 stmts 中定位 Scene 语句
                        let leaf = name.rsplit('.').next().unwrap_or(name);
                        let parent_node = frames
                            .iter()
                            .rev()
                            .find_map(|f| f.node.as_deref())
                            .unwrap_or("");
                        if name != &format!("{parent_node}.{leaf}") {
                            return Err(RunError::new("存档场景身份与父节点不符"));
                        }
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
                            inspection_call_id: 0,
                            stmts: &sc.body,
                            idx: sv.idx,
                            node: Some(name.clone()),
                            src: None,
                            fragment: None,
                            locals: Default::default(),
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
                    let stmt_idx = match src {
                        FrameSrc::IfBranch { stmt, .. } | FrameSrc::ChoiceBody { stmt } => *stmt,
                        FrameSrc::FragmentCall { .. } => {
                            return Err(RunError::new("片段帧缺少片段身份"))
                        }
                    };
                    let Some(source_stmt) = parent_stmts.get(stmt_idx) else {
                        return Err(RunError::new("存档与程序结构不符(内联帧来源越界)"));
                    };
                    let expect_parent_idx = match (src, source_stmt) {
                        (FrameSrc::IfBranch { .. }, Stmt::If(_)) => stmt_idx + 1,
                        (FrameSrc::ChoiceBody { .. }, Stmt::Choice(_)) => {
                            let mut end = stmt_idx + 1;
                            while matches!(parent_stmts.get(end), Some(Stmt::Choice(_))) {
                                end += 1;
                            }
                            end
                        }
                        _ => return Err(RunError::new("存档与程序结构不符(帧来源不匹配)")),
                    };
                    if parent.idx != expect_parent_idx {
                        return Err(RunError::new("存档与程序结构不符(内联帧错位)"));
                    }
                    let stmts: &'p [Stmt] = match (source_stmt, src) {
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
                        inspection_call_id: 0,
                        stmts,
                        idx: sv.idx,
                        node: None,
                        src: Some(*src),
                        fragment: None,
                        locals: Default::default(),
                    });
                }
            }
        }
        if frames.iter().any(|f| f.idx > f.stmts.len()) {
            return Err(RunError::new("存档语句位置越界"));
        }
        for (i, frame) in frames.iter().enumerate() {
            if let Some(root) = frames[..=i].iter().rev().find(|f| f.fragment.is_some()) {
                for stmt in &frame.stmts[..frame.idx] {
                    if let Stmt::Local(local) = stmt {
                        if !root.locals.contains_key(&local.name) {
                            return Err(RunError::new(format!(
                                "存档缺少已执行的local `{}`",
                                local.name
                            )));
                        }
                    }
                }
            }
        }
        Ok(frames)
    }
}

fn required_features(program: &Program) -> Vec<String> {
    let mut features = Vec::new();
    if worldline_core::language::uses_new_features(program) {
        features.push("runtime.language_1_11.v1".into());
    }
    if super::choices::uses_presentation(program) {
        features.push(super::CHOICE_PRESENTATION_CAPABILITY.into());
    }
    features
}

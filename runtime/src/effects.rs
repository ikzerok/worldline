use worldline_core::ast::{BinOp, Change, ChangeKind, EffectWhen, Expr};

use super::util::{static_name, unique_tags};
use super::{AnchorKind, AnchorRecord, RunError, StateRecord, Story, Value};

impl<'p> Story<'p> {
    // -- v1.5:准入、效果、变动 ------------------------------------------------

    /// 事件准入闸门:perm 权限 + after 前置(规范 semantics.md §8.2)。
    pub(super) fn admit(&self, event_idx: usize) -> Result<(), RunError> {
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
            match self.eval_global(cond)? {
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
    pub(super) fn run_effects(
        &mut self,
        event_idx: usize,
        when: EffectWhen,
    ) -> Result<(), RunError> {
        let effects = self.program.events[event_idx].effects.clone();
        for fx in effects {
            if fx.when != when {
                continue;
            }
            if let Some(cond) = &fx.cond {
                if !matches!(self.eval_global(cond)?, Value::Bool(true)) {
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
    pub(super) fn run_exit_effects(&mut self, natural: bool) -> Result<(), RunError> {
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
    pub(super) fn apply_change(&mut self, a: &Change) -> Result<(), RunError> {
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

    pub(super) fn push_anchor(
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
}

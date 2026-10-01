//! 时间前驱表单的只读投影与内容基线保护；不更改草稿或推断时间边。
use super::{qualified, EventDraft};
use crate::project::Project;
use crate::timeline::{scope_rejection, TemporalOrderScope, TimelineStatus};
use crate::CompileResult;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct EventPredecessorOption {
    pub id: String,
    pub display: String,
    pub period: Option<String>,
    pub root: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub selected: bool,
    /// None 表示可选；全局阻断也逐项反映，已选非法项仍可显式取消。
    pub rejection: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventPredecessorOptions {
    pub order_scope: TemporalOrderScope,
    pub period: Option<String>,
    pub root: Option<String>,
    pub blocked_reason: Option<String>,
    pub entries: Vec<EventPredecessorOption>,
}

impl Project {
    /// 显示全部已选关系（含缺失目标），合法性只由 core 的完整当前分析决定。
    pub fn event_predecessor_options(
        &self,
        path: &Path,
        original: Option<&str>,
        draft: &EventDraft,
        expected_baseline: &str,
    ) -> EventPredecessorOptions {
        let result = self.compile_current();
        let timeline = &result.analysis.timeline;
        let blocked_reason = if expected_baseline != self.content_baseline() {
            Some("事件草稿基线已过期，请保留输入并重新打开表单".into())
        } else {
            self.validate_event_edit(path, original, draft, &result)
                .err()
        };
        let roots: BTreeMap<_, _> = timeline
            .periods
            .iter()
            .map(|period| (period.id.as_str(), period.root.as_deref()))
            .collect();
        let root = draft
            .period
            .as_deref()
            .and_then(|period| roots.get(period).copied().flatten());
        // 移除正在编辑的入边后，能从当前事件到达的事件均不能成为新前驱。
        // 全部草稿边都指向同一事件，互相不会生成另一个独立的环判定。
        let descendants = descendants(&draft.id, &timeline.edges);
        let mut entries = BTreeMap::new();
        for (index, event) in result.program.events.iter().enumerate() {
            let event_root = event
                .period
                .as_deref()
                .and_then(|period| roots.get(period).copied().flatten());
            let rejection = if event.name == draft.id {
                Some("事件不能以自身为前驱".into())
            } else if let Some(required) = scope_rejection(
                timeline.order_scope,
                draft.period.as_deref(),
                root,
                event.period.as_deref(),
                event_root,
            ) {
                Some(format!("时间约束必须{required}"))
            } else if descendants.contains(event.name.as_str()) {
                Some("该事件已是当前事件的后继，选为前驱会形成时间约束环".into())
            } else {
                blocked_reason.clone()
            };
            entries.insert(
                event.name.clone(),
                EventPredecessorOption {
                    id: event.name.clone(),
                    display: event.summary.clone().unwrap_or_else(|| event.name.clone()),
                    period: event.period.clone(),
                    root: event_root.map(str::to_owned),
                    file: result.program.event_files.get(index).cloned(),
                    line: Some(event.loc.line),
                    selected: draft.predecessors.contains(&event.name),
                    rejection,
                },
            );
        }
        for id in &draft.predecessors {
            entries
                .entry(id.clone())
                .or_insert_with(|| EventPredecessorOption {
                    id: id.clone(),
                    display: id.clone(),
                    period: None,
                    root: None,
                    file: None,
                    line: None,
                    selected: true,
                    rejection: Some(if id == &draft.id {
                        "事件不能以自身为前驱".into()
                    } else {
                        "前驱事件不存在或当前分析无法确定其身份".into()
                    }),
                });
        }
        EventPredecessorOptions {
            order_scope: timeline.order_scope,
            period: draft.period.clone(),
            root: root.map(str::to_owned),
            blocked_reason,
            entries: entries.into_values().collect(),
        }
    }

    /// 使用打开表单时的基线；成功仅应用缓冲，保存与撤销由调用方显式处理。
    pub fn write_event_at_baseline(
        &mut self,
        path: &Path,
        original: Option<&str>,
        draft: &EventDraft,
        expected_baseline: &str,
    ) -> Result<(), String> {
        if expected_baseline != self.content_baseline() {
            return Err("事件草稿基线已过期，请保留输入并重新打开表单".into());
        }
        self.write_event(path, original, draft)
    }

    pub(super) fn validate_event_edit(
        &self,
        path: &Path,
        original: Option<&str>,
        draft: &EventDraft,
        result: &CompileResult,
    ) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        let path = crate::file_access::within(&self.root, path)?;
        self.document(&path)?;
        if self
            .source_selection()
            .is_some_and(|selection| !selection.is_active(&path))
        {
            return Err("事件只能写入当前活动源码".into());
        }
        if timeline_incomplete(result) {
            return Err("当前工程分析不完整，请先修复源码诊断；事件草稿未改变".into());
        }
        qualified(&draft.id)?;
        if let Some(original) = original {
            if original != draft.id {
                return Err("事件 ID 是稳定引用,修改事件内容时请保留 ID".into());
            }
            let index = result.program.event_index(original).ok_or("事件不存在")?;
            if Path::new(&result.program.event_files[index]) != path {
                return Err("事件不存在于目标文件".into());
            }
        } else if result.program.event_index(&draft.id).is_some() {
            return Err("事件 ID 已存在".into());
        }
        Ok(())
    }
}

fn timeline_incomplete(result: &CompileResult) -> bool {
    result.has_errors() || result.analysis.timeline.status != TimelineStatus::Complete
}

fn descendants<'a>(id: &str, edges: &'a [crate::timeline::TemporalEdge]) -> BTreeSet<&'a str> {
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in edges.iter().filter(|edge| edge.after != id) {
        outgoing.entry(&edge.before).or_default().push(&edge.after);
    }
    let mut pending = outgoing.get(id).cloned().unwrap_or_default();
    let mut visited = BTreeSet::new();
    while let Some(next) = pending.pop() {
        if visited.insert(next) {
            pending.extend(outgoing.get(next).into_iter().flatten().copied());
        }
    }
    visited
}

//! 世界时间关系：直接时段归属与显式偏序，独立于运行控制流。
use crate::{Diagnostic, RelationGraph, Severity};
use serde::Serialize;
use std::collections::HashSet;

mod analyze;
pub(crate) use analyze::analyze;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineStatus {
    Complete,
    #[default]
    Partial,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalOrderScope {
    #[default]
    DirectPeriod,
    RootPeriod,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeriodInfo {
    pub parent: Option<String>,
    pub root: Option<String>,
    pub id: String,
    pub display: String,
    pub file: String,
    pub line: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct TemporalEvent {
    pub event: String,
    pub period: String,
    /// 只使用同一直接时段内部的边，错误快照须结合 status 判断。
    pub rank: u32,
    pub root: Option<String>,
    pub order_scope: Option<String>,
    pub root_rank: Option<u32>,
    pub status: TimelineStatus,
}
#[derive(Debug, Clone, Serialize)]
pub struct TemporalEdge {
    pub before: String,
    pub after: String,
    pub root: Option<String>,
    pub order_scope: String,
    pub file: String,
    pub line: u32,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Timeline {
    pub periods: Vec<PeriodInfo>,
    pub events: Vec<TemporalEvent>,
    pub edges: Vec<TemporalEdge>,
    pub order_scope: TemporalOrderScope,
    pub status: TimelineStatus,
}

/// 编译与作者候选投影共用的比较范围规则；根由 analyze 唯一解析。
pub(crate) fn scope_rejection(
    scope: TemporalOrderScope,
    after_period: Option<&str>,
    after_root: Option<&str>,
    before_period: Option<&str>,
    before_root: Option<&str>,
) -> Option<&'static str> {
    match scope {
        TemporalOrderScope::DirectPeriod
            if after_period.is_none() || after_period != before_period =>
        {
            Some("位于同一时段")
        }
        TemporalOrderScope::RootPeriod if after_root.is_none() || after_root != before_root => {
            Some("共享唯一明确的已声明时间根")
        }
        _ => None,
    }
}

impl Timeline {
    /// 完整编译诊断汇总后再调用，解析失败/缺失节点也不伪装成完整空图。
    pub(crate) fn mark_incomplete(&mut self, diagnostics: &[Diagnostic]) {
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            self.status = TimelineStatus::Partial;
            for event in &mut self.events {
                event.status = TimelineStatus::Partial;
                event.root_rank = None;
            }
        }
    }

    /// 只投影当前模式中可信的拓扑层级，不推断全序或日期。
    pub fn event_rank(&self, event: &TemporalEvent) -> Option<u32> {
        if self.status != TimelineStatus::Complete || event.status != TimelineStatus::Complete {
            return None;
        }
        match self.order_scope {
            TemporalOrderScope::DirectPeriod => Some(event.rank),
            TemporalOrderScope::RootPeriod => event.root_rank,
        }
    }

    /// 稳定的父先子后遍历；错误输入中的孤立节点也只返回一次。
    pub fn period_order(&self) -> Vec<(&PeriodInfo, usize)> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut pending: Vec<_> = self
            .periods
            .iter()
            .filter(|p| p.parent.is_none())
            .rev()
            .map(|p| (p, 0))
            .collect();
        for fallback in &self.periods {
            if pending.is_empty() && !seen.contains(&fallback.id) {
                pending.push((fallback, 0));
            }
            while let Some((period, depth)) = pending.pop() {
                if !seen.insert(period.id.clone()) {
                    continue;
                }
                out.push((period, depth));
                pending.extend(
                    self.periods
                        .iter()
                        .rev()
                        .filter(|p| p.parent.as_deref() == Some(&period.id))
                        .map(|p| (p, depth + 1)),
                );
            }
        }
        out
    }

    pub fn to_mermaid(&self, graph: &RelationGraph) -> String {
        let status = if self.status == TimelineStatus::Complete {
            "完整偏序；不构成全序，同层不代表同时"
        } else {
            "不完整时间线；存在编译错误，层级不可作为可信时间顺序"
        };
        if self.periods.is_empty() {
            return format!(
                "%% {status}；无世界时段，以下为故事线过程流\n{}",
                graph.to_timeline_mermaid()
            );
        }
        let scope = match self.order_scope {
            TemporalOrderScope::DirectPeriod => "直接时段内",
            TemporalOrderScope::RootPeriod => "同一声明根内；独立根不可比",
        };
        let mut out = format!("flowchart LR\n  %% {status}；比较范围：{scope}\n");
        let safe = |s: &str| {
            s.replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\n', " ")
        };
        let mut grouped = HashSet::new();
        let mut open = 0;
        for (period, depth) in self.period_order() {
            while open > depth {
                out.push_str("  end\n");
                open -= 1;
            }
            let i = self.periods.iter().position(|p| p.id == period.id).unwrap();
            open += 1;
            out.push_str(&format!(
                "  subgraph p{i}[\"{} · 部分顺序\"]\n",
                safe(&period.display)
            ));
            for event in self.events.iter().filter(|e| e.period == period.id) {
                if let Some(id) = graph.ids.get(&event.event) {
                    grouped.insert(event.event.clone());
                    let order = self.event_rank(event).map_or_else(
                        || "层级未知".into(),
                        |rank| {
                            format!(
                                "范围 {} · 层 {rank}",
                                event.order_scope.as_deref().unwrap_or("未知")
                            )
                        },
                    );
                    out.push_str(&format!(
                        "    n{id}[\"{} · {}\"]\n",
                        safe(&event.event),
                        safe(&order)
                    ));
                }
            }
        }
        for _ in 0..open {
            out.push_str("  end\n");
        }
        for (i, node) in graph
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_event && !grouped.contains(&n.name))
        {
            out.push_str(&format!("  n{i}[\"{}\"]\n", safe(&node.name)));
        }
        for edge in &self.edges {
            if let (Some(from), Some(to)) =
                (graph.ids.get(&edge.before), graph.ids.get(&edge.after))
            {
                out.push_str(&format!("  n{from} -->|先于| n{to}\n"));
            }
        }
        out
    }
}

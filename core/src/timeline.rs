//! 世界时间关系:无序事件组与显式先后约束,独立于运行控制流。
use crate::{Diagnostic, Program, RelationGraph, Span};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize)]
pub struct PeriodInfo {
    pub parent: Option<String>,
    pub id: String,
    pub display: String,
    pub file: String,
    pub line: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct TemporalEvent {
    pub event: String,
    pub period: String,
    pub rank: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct TemporalEdge {
    pub before: String,
    pub after: String,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Timeline {
    pub periods: Vec<PeriodInfo>,
    pub events: Vec<TemporalEvent>,
    pub edges: Vec<TemporalEdge>,
}

pub(crate) fn analyze(program: &Program, diagnostics: &mut Vec<Diagnostic>) -> Timeline {
    let mut timeline = Timeline::default();
    let mut periods = HashSet::new();
    for period in &program.periods {
        if !periods.insert(period.name.clone()) {
            diagnostics.push(Diagnostic::error(
                "A104",
                &period.file,
                Span::new(period.loc.line, 1, 6),
                format!("时段 `{}` 重复定义", period.name),
            ));
        } else {
            timeline.periods.push(PeriodInfo {
                parent: period.parent.clone(),
                id: period.name.clone(),
                display: period
                    .display
                    .clone()
                    .unwrap_or_else(|| period.name.clone()),
                file: period.file.clone(),
                line: period.loc.line,
            });
        }
    }
    for period in &timeline.periods {
        let mut seen = HashSet::from([period.id.as_str()]);
        let mut parent = period.parent.as_deref();
        while let Some(id) = parent {
            let next = timeline.periods.iter().find(|p| p.id == id);
            if !seen.insert(id) || next.is_none() {
                diagnostics.push(Diagnostic::error(
                    "A219",
                    &period.file,
                    Span::new(period.line, 1, 6),
                    format!("时段 `{}` 的上级不存在或包含关系有环：{id}", period.id),
                ));
                break;
            }
            parent = next.and_then(|p| p.parent.as_deref());
        }
    }
    let ids: HashMap<_, _> = program
        .events
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.as_str(), i))
        .collect();
    let mut adjacency = vec![Vec::new(); program.events.len()];
    let mut degree = vec![0usize; program.events.len()];
    let mut rank = vec![0u32; program.events.len()];
    for (i, event) in program.events.iter().enumerate() {
        let mut report = |message| {
            diagnostics.push(Diagnostic::error(
                "A213",
                &program.event_files[i],
                Span::new(event.loc.line, 1, 5),
                message,
            ))
        };
        if event.period.as_ref().is_some_and(|p| !periods.contains(p)) {
            report(format!("事件 `{}` 引用未定义时段", event.name));
        }
        let mut seen = HashSet::new();
        for predecessor in &event.predecessors {
            if !seen.insert(predecessor) {
                continue;
            }
            let Some(&before) = ids.get(predecessor.as_str()) else {
                report(format!("前驱事件 `{predecessor}` 不存在"));
                continue;
            };
            if event.period.is_none() || event.period != program.events[before].period {
                report(format!(
                    "时间约束 `{predecessor}` → `{}` 必须位于同一时段",
                    event.name
                ));
                continue;
            }
            adjacency[before].push(i);
            degree[i] += 1;
            timeline.edges.push(TemporalEdge {
                before: predecessor.clone(),
                after: event.name.clone(),
            });
        }
    }
    let mut queue: VecDeque<_> = degree
        .iter()
        .enumerate()
        .filter(|(_, d)| **d == 0)
        .map(|(i, _)| i)
        .collect();
    while let Some(i) = queue.pop_front() {
        for &next in &adjacency[i] {
            rank[next] = rank[next].max(rank[i] + 1);
            degree[next] -= 1;
            if degree[next] == 0 {
                queue.push_back(next);
            }
        }
    }
    for (i, event) in program.events.iter().enumerate() {
        if degree[i] > 0 {
            diagnostics.push(Diagnostic::error(
                "A213",
                &program.event_files[i],
                Span::new(event.loc.line, 1, 5),
                format!("事件 `{}` 的时间约束有环或依赖环路", event.name),
            ));
        }
        if let Some(period) = &event.period {
            timeline.events.push(TemporalEvent {
                event: event.name.clone(),
                period: period.clone(),
                rank: rank[i],
            });
        }
    }
    timeline
}

impl Timeline {
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
        if self.periods.is_empty() {
            return graph.to_timeline_mermaid();
        }
        let mut out = String::from("flowchart LR\n");
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
                    out.push_str(&format!("    n{id}[\"{}\"]\n", safe(&event.event)));
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

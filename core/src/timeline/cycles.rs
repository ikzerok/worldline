use super::{evidence::EdgeGraph, TemporalEdge};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemporalCycle {
    /// 当前快照中该强连通分量的最小成员 ID。
    pub id: String,
    pub members: Vec<String>,
    pub witness: Vec<TemporalEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemporalBlockedEvent {
    pub event: String,
    pub cycle_ids: Vec<String>,
}

pub(super) fn analyze_cycles(
    edges: &[TemporalEdge],
) -> (Vec<TemporalCycle>, Vec<TemporalBlockedEvent>) {
    let graph = EdgeGraph::new(edges);
    let mut components = components(&graph);
    components.retain(|members| {
        members.len() > 1
            || graph.outgoing[members[0]]
                .iter()
                .any(|&edge| graph.after(edge) == members[0])
    });
    components.sort_by_key(|members| members[0]);
    let mut cyclic = vec![false; graph.ids.len()];
    let cycles: Vec<_> = components
        .iter()
        .map(|members| {
            let mut allowed = vec![false; graph.ids.len()];
            for &member in members {
                cyclic[member] = true;
                allowed[member] = true;
            }
            TemporalCycle {
                id: graph.ids[members[0]].into(),
                members: members.iter().map(|&i| graph.ids[i].into()).collect(),
                // 真环 SCC 中必然存在从任一成员回到自身的非空路径。
                witness: graph
                    .path(members[0], members[0], Some(&allowed))
                    .expect("cyclic component must contain a closed path"),
            }
        })
        .collect();
    let mut blocked: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (members, cycle) in components.iter().zip(&cycles) {
        let mut seen = vec![false; graph.ids.len()];
        let mut pending = VecDeque::from([members[0]]);
        seen[members[0]] = true;
        while let Some(current) = pending.pop_front() {
            if !cyclic[current] {
                blocked
                    .entry(graph.ids[current])
                    .or_default()
                    .push(cycle.id.clone());
            }
            for &edge in &graph.outgoing[current] {
                let next = graph.after(edge);
                if !seen[next] {
                    seen[next] = true;
                    pending.push_back(next);
                }
            }
        }
    }
    let blocked = blocked
        .into_iter()
        .map(|(event, cycle_ids)| TemporalBlockedEvent {
            event: event.into(),
            cycle_ids,
        })
        .collect();
    (cycles, blocked)
}

/// 迭代 Kosaraju，避免长链导致 Rust 调用栈溢出。
fn components(graph: &EdgeGraph<'_>) -> Vec<Vec<usize>> {
    let mut visited = vec![false; graph.ids.len()];
    let mut finish = Vec::new();
    for first in 0..graph.ids.len() {
        if visited[first] {
            continue;
        }
        visited[first] = true;
        let mut pending = vec![(first, 0)];
        while let Some((current, child)) = pending.last_mut() {
            if let Some(&edge) = graph.outgoing[*current].get(*child) {
                *child += 1;
                let next = graph.after(edge);
                if !visited[next] {
                    visited[next] = true;
                    pending.push((next, 0));
                }
            } else {
                finish.push(*current);
                pending.pop();
            }
        }
    }
    visited.fill(false);
    let mut result = Vec::new();
    for first in finish.into_iter().rev() {
        if visited[first] {
            continue;
        }
        visited[first] = true;
        let mut members = Vec::new();
        let mut pending = vec![first];
        while let Some(current) = pending.pop() {
            members.push(current);
            for &edge in &graph.incoming[current] {
                let next = graph.before(edge);
                if !visited[next] {
                    visited[next] = true;
                    pending.push(next);
                }
            }
        }
        members.sort_unstable();
        result.push(members);
    }
    result
}

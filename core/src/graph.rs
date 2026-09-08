//! 关系图数据模型与导出。

use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Divert,
    Choice,
    Enter,
    /// 漂流跃迁 `->>`:跨故事线转移(v1.5)。
    Drift,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub name: String,
    pub is_event: bool,
    pub file: String,
    pub line: u32,
    pub choice_count: u32,
    pub word_count: u32,
    pub storyline: String,
    /// 故事线内序号,1 起。
    pub seq: u32,
    pub summary: Option<String>,
    pub characters: Vec<String>,
    pub perm: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge {
    pub contexts: Vec<crate::relation_context::TransitionContext>,
    pub target_requirement: Option<String>,
    pub from: u32,
    pub to: u32,
    pub kind: EdgeKind,
    pub label: Option<String>,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelationGraph {
    pub nodes: Vec<GraphNode>,
    pub ids: HashMap<String, u32>,
    pub edges: Vec<GraphEdge>,
    pub entry: u32,
    /// 从 entry 沿边最短路径;不可达 = u32::MAX。
    pub depth: Vec<u32>,
    /// 故事线 (id, 显示名),按声明序(隐式线随后)。
    pub storyline_order: Vec<(String, String)>,
}

/// 源码中的锚点声明(时间线打点用)。
#[derive(Debug, Clone, Serialize)]
pub struct AnchorDecl {
    /// 所属节点(事件或场景全名)。
    pub node: String,
    pub name: String,
    pub note: Option<String>,
    pub file: String,
    pub line: u32,
}

impl RelationGraph {
    pub fn adjacency(&self) -> Vec<Vec<u32>> {
        let mut adj = vec![Vec::new(); self.nodes.len()];
        for edge in &self.edges {
            adj[edge.from as usize].push(edge.to);
        }
        adj
    }

    /// Mermaid flowchart 导出(规范 relations.md §3)。
    pub fn to_mermaid(&self) -> String {
        let mut out = String::from("flowchart TD\n");
        for (index, node) in self.nodes.iter().enumerate() {
            let label = if node.word_count > 0 {
                format!("{} · {}字", node.name, node.word_count)
            } else {
                node.name.clone()
            };
            if node.is_event {
                out.push_str(&format!("    n{index}[\"{label}\"]\n"));
            } else {
                out.push_str(&format!("    n{index}>(\"{label}\")\n"));
            }
        }
        for edge in &self.edges {
            let from = format!("n{}", edge.from);
            let to = format!("n{}", edge.to);
            match edge.kind {
                EdgeKind::Divert => out.push_str(&format!("    {from} --> {to}\n")),
                EdgeKind::Drift => out.push_str(&format!("    {from} ==>|\"漂流\"| {to}\n")),
                EdgeKind::Choice => {
                    let label = edge.label.as_deref().unwrap_or("");
                    out.push_str(&format!("    {from} -. \"{label}\" .-> {to}\n"));
                }
                EdgeKind::Enter => out.push_str(&format!("    {from} -.-> {to}\n")),
            }
        }
        out
    }

    /// 时间线投影的 Mermaid 导出(规范 relations.md §3):
    /// 每条故事线一个 subgraph 泳道,事件按序号排布,漂流跨泳道。
    pub fn to_timeline_mermaid(&self) -> String {
        let mut out = String::from("flowchart LR\n");
        let mut lanes: Vec<Vec<usize>> = vec![Vec::new(); self.storyline_order.len()];
        let mut lane_of: HashMap<&str, usize> = HashMap::new();
        for (lane_index, (id, _)) in self.storyline_order.iter().enumerate() {
            lane_of.insert(id.as_str(), lane_index);
        }
        for (index, node) in self.nodes.iter().enumerate() {
            if !node.is_event {
                continue;
            }
            if let Some(&lane_index) = lane_of.get(node.storyline.as_str()) {
                lanes[lane_index].push(index);
            } else {
                lane_of.insert(node.storyline.as_str(), lanes.len());
                lanes.push(vec![index]);
            }
        }
        for lane in &mut lanes {
            lane.sort_by_key(|&index| (self.nodes[index].seq, self.nodes[index].name.clone()));
        }
        for (lane_index, members) in lanes.iter().enumerate() {
            let (_, display) =
                &self.storyline_order[lane_index.min(self.storyline_order.len().saturating_sub(1))];
            out.push_str(&format!(
                "  subgraph S{lane_index}[\"▶ 故事线 · {display}\"]\n    direction LR\n"
            ));
            for &index in members {
                let node = &self.nodes[index];
                let mut label = format!("{}. {}", node.seq, node.name);
                if let Some(summary) = &node.summary {
                    label.push_str(&format!(" · {}", summary.replace('"', "'")));
                }
                if node.perm.is_some() {
                    label.push_str(" 🔒");
                }
                out.push_str(&format!("    n{index}[\"{label}\"]\n"));
            }
            out.push_str("  end\n");
        }

        let root_of = |id: u32| -> u32 {
            let name = &self.nodes[id as usize].name;
            if self.nodes[id as usize].is_event {
                id
            } else {
                let root = name.split('.').next().unwrap_or(name);
                self.ids.get(root).copied().unwrap_or(id)
            }
        };
        for edge in &self.edges {
            match edge.kind {
                EdgeKind::Drift => {
                    let from = root_of(edge.from);
                    let to = root_of(edge.to);
                    out.push_str(&format!("  n{from} ==>|\"漂流\"| n{to}\n"));
                }
                EdgeKind::Divert => {
                    let (from, to) = (root_of(edge.from), root_of(edge.to));
                    if from != to
                        && self.nodes[from as usize].is_event
                        && self.nodes[to as usize].is_event
                        && self.nodes[from as usize].storyline == self.nodes[to as usize].storyline
                    {
                        out.push_str(&format!("  n{from} --> n{to}\n"));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{compile_source, EdgeKind};

    #[test]
    fn adjacency_contains_divert_destination() {
        let result = compile_source(
            "test.wl",
            "event start\n  -> ending\nevent ending\n  -> END\n",
        );
        let graph = &result.analysis.graph;
        let start = graph.ids["start"];
        let ending = graph.ids["ending"];
        assert!(graph.adjacency()[start as usize].contains(&ending));
        assert!(graph.edges.iter().any(|edge| edge.kind == EdgeKind::Divert));
    }

    #[test]
    fn mermaid_export_has_stable_header_and_nodes() {
        let result = compile_source("test.wl", "event start\n  -> END\n");
        let mermaid = result.analysis.graph.to_mermaid();
        assert!(mermaid.starts_with("flowchart TD\n"));
        assert!(mermaid.contains("start"));
    }
}

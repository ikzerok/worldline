use super::TemporalEdge;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// 只索引真实显式边；所有遍历共享同一稳定边序，不依赖声明/哈希顺序。
pub(super) struct EdgeGraph<'a> {
    pub ids: Vec<&'a str>,
    pub indices: BTreeMap<&'a str, usize>,
    pub edges: Vec<&'a TemporalEdge>,
    pub outgoing: Vec<Vec<usize>>,
    pub incoming: Vec<Vec<usize>>,
}

impl<'a> EdgeGraph<'a> {
    pub fn new(edges: &'a [TemporalEdge]) -> Self {
        let ids: Vec<_> = edges
            .iter()
            .flat_map(|e| [e.before.as_str(), e.after.as_str()])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let indices: BTreeMap<_, _> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        let mut edges: Vec<_> = edges.iter().collect();
        edges.sort_by(|a, b| {
            a.before
                .cmp(&b.before)
                .then(a.after.cmp(&b.after))
                .then(a.file.cmp(&b.file))
                .then(a.line.cmp(&b.line))
                .then(a.order_scope.cmp(&b.order_scope))
                .then(a.root.cmp(&b.root))
        });
        let mut outgoing = vec![Vec::new(); ids.len()];
        let mut incoming = vec![Vec::new(); ids.len()];
        for (i, edge) in edges.iter().enumerate() {
            outgoing[indices[edge.before.as_str()]].push(i);
            incoming[indices[edge.after.as_str()]].push(i);
        }
        Self {
            ids,
            indices,
            edges,
            outgoing,
            incoming,
        }
    }

    pub fn before(&self, edge: usize) -> usize {
        self.indices[self.edges[edge].before.as_str()]
    }

    pub fn after(&self, edge: usize) -> usize {
        self.indices[self.edges[edge].after.as_str()]
    }

    /// 返回非空最短路径；同长取逐边字典序。from == to 用于真实闭环见证。
    pub fn path(
        &self,
        from: usize,
        to: usize,
        allowed: Option<&[bool]>,
    ) -> Option<Vec<TemporalEdge>> {
        let mut seen = vec![false; self.ids.len()];
        let mut previous: Vec<Option<(usize, usize)>> = vec![None; self.ids.len()];
        let mut pending = VecDeque::from([from]);
        seen[from] = true;
        while let Some(current) = pending.pop_front() {
            for &edge in &self.outgoing[current] {
                let next = self.after(edge);
                if allowed.is_some_and(|members| !members[next]) {
                    continue;
                }
                if next == to {
                    let mut path = vec![self.edges[edge].clone()];
                    let mut cursor = current;
                    while cursor != from {
                        let (parent, step) = previous[cursor]?;
                        path.push(self.edges[step].clone());
                        cursor = parent;
                    }
                    path.reverse();
                    return Some(path);
                }
                if !seen[next] {
                    seen[next] = true;
                    previous[next] = Some((current, edge));
                    pending.push_back(next);
                }
            }
        }
        None
    }
}

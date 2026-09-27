use super::{
    LegacyRelationHandle, RelationDirection, RelationQueryContinuation, RelationQueryDirection,
    RelationQueryEdge, RelationQueryNode, RelationQueryOptions, RelationQueryResult,
    SemanticRelationInfo,
};
use crate::catalog::{Catalog, TargetRef};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

impl Catalog {
    /// 在同一目录快照上读取下一页，沿用前页的目标和筛选。
    pub fn continue_relations(
        &self,
        continuation: &RelationQueryContinuation,
    ) -> RelationQueryResult {
        self.query_relations(
            &continuation.target,
            RelationQueryOptions {
                offset: continuation.offset,
                depth: continuation.depth,
                relation_type: continuation.relation_type.clone(),
                relation_types: continuation.relation_types.clone(),
                scope_refs: continuation.scope_refs.clone(),
                include_unscoped: continuation.include_unscoped,
                direction: continuation.direction,
                ..Default::default()
            },
        )
    }

    /// 查询独立关系；不生成反向关系、传递关系或推断路径。
    pub fn query_relations(
        &self,
        target: &TargetRef,
        options: RelationQueryOptions,
    ) -> RelationQueryResult {
        let options = options.bounded();
        let mut nodes = Vec::new();
        let mut node_depth = BTreeMap::<TargetRef, u8>::from([(target.clone(), 0)]);
        let mut discovered = BTreeMap::<TargetRef, u8>::from([(target.clone(), 0)]);
        let mut queue = VecDeque::from([(target.clone(), 0u8)]);
        let mut edges = Vec::new();
        let mut emitted = BTreeSet::new();
        let mut frontier = BTreeSet::new();
        let mut truncated = false;
        let mut position = 0usize;

        'traverse: while let Some((current, depth)) = queue.pop_front() {
            if depth >= options.depth {
                continue;
            }
            let candidates = self
                .relation_index
                .get(&current)
                .into_iter()
                .flat_map(|ids| ids.iter())
                .filter_map(|id| self.relations.get(id))
                .filter(|relation| {
                    let undirected = self
                        .relation_types
                        .get(&relation.relation_type)
                        .is_some_and(|info| info.direction == RelationDirection::Undirected);
                    options
                        .relation_type
                        .as_deref()
                        .is_none_or(|kind| relation.relation_type == kind)
                        && (options.relation_types.is_empty()
                            || options.relation_types.contains(&relation.relation_type))
                        && relation_matches_scope(self, relation, &options)
                        && (undirected
                            || ((relation.from_ref == current
                                && options.direction != RelationQueryDirection::Incoming)
                                || (relation.to_ref == current
                                    && options.direction != RelationQueryDirection::Outgoing)))
                });
            for relation in candidates {
                if !emitted.insert(relation.id.clone()) {
                    continue;
                }
                let (next, reverse) = if relation.from_ref == current {
                    (&relation.to_ref, false)
                } else {
                    (&relation.from_ref, true)
                };
                let next_depth = depth + 1;
                // 翻页跳过显示时仍沿原快照遍历，避免遗漏第二层的连接。
                if !discovered.contains_key(next) {
                    discovered.insert(next.clone(), next_depth);
                    queue.push_back((next.clone(), next_depth));
                }
                if position < options.offset {
                    position += 1;
                    continue;
                }
                let additional = usize::from(!node_depth.contains_key(&current))
                    + usize::from(next != &current && !node_depth.contains_key(next));
                if edges.len() >= options.max_edges
                    || node_depth.len() + additional > options.max_nodes
                {
                    truncated = true;
                    frontier.insert(current.clone());
                    frontier.insert(next.clone());
                    break 'traverse;
                }
                node_depth.insert(current.clone(), discovered[&current]);
                node_depth.insert(next.clone(), discovered[next]);
                let type_info = self.relation_types.get(&relation.relation_type);
                let label = if reverse {
                    type_info
                        .and_then(|info| info.inverse_display.clone())
                        .unwrap_or_else(|| {
                            type_info
                                .map(|info| info.display.clone())
                                .unwrap_or_else(|| relation.relation_type.clone())
                        })
                } else {
                    type_info
                        .map(|info| info.display.clone())
                        .unwrap_or_else(|| relation.relation_type.clone())
                };
                edges.push(RelationQueryEdge {
                    id: relation.id.clone(),
                    relation_type: relation.relation_type.clone(),
                    from_ref: relation.from_ref.clone(),
                    to_ref: relation.to_ref.clone(),
                    label,
                    direction: type_info
                        .map(|info| info.direction)
                        .unwrap_or(RelationDirection::Directed),
                    source_note: relation.source_note.clone(),
                    file: relation.file.clone(),
                    line: relation.line,
                });
                position += 1;
            }
        }
        for (node, depth) in node_depth {
            nodes.push(RelationQueryNode {
                target: node,
                depth,
            });
        }
        nodes.sort_by(|a, b| (a.depth, &a.target).cmp(&(b.depth, &b.target)));
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        RelationQueryResult {
            schema_version: 1,
            target: target.clone(),
            depth: options.depth,
            nodes,
            edges,
            truncated,
            continuation: truncated.then(|| RelationQueryContinuation {
                offset: position,
                target: target.clone(),
                depth: options.depth,
                relation_type: options.relation_type.clone(),
                relation_types: options.relation_types.clone(),
                scope_refs: options.scope_refs.clone(),
                include_unscoped: options.include_unscoped,
                direction: options.direction,
                frontier: frontier.into_iter().collect(),
            }),
        }
    }

    pub fn legacy_relation_handles(&self) -> Vec<LegacyRelationHandle> {
        self.legacy_relations
            .iter()
            .map(|relation| relation.handle.clone())
            .collect()
    }
}

fn scope_dimension(catalog: &Catalog, target: &TargetRef) -> String {
    match target.kind.as_str() {
        "period" => "period".into(),
        "event" | "scene" => "story".into(),
        "entity"
            if catalog
                .entities
                .get(&target.id)
                .is_some_and(|entity| entity.entity_type == "version") =>
        {
            "version".into()
        }
        _ => target.kind.clone(),
    }
}

pub(crate) fn relation_matches_scope(
    catalog: &Catalog,
    relation: &SemanticRelationInfo,
    options: &RelationQueryOptions,
) -> bool {
    if options.scope_refs.is_empty() {
        return true;
    }
    if relation.scope_refs.is_empty() {
        return options.include_unscoped;
    }
    let mut requested = BTreeMap::<String, Vec<&TargetRef>>::new();
    for scope in &options.scope_refs {
        requested
            .entry(scope_dimension(catalog, scope))
            .or_default()
            .push(scope);
    }
    requested.into_iter().all(|(dimension, selected)| {
        relation
            .scope_refs
            .iter()
            .any(|scope| scope_dimension(catalog, scope) == dimension && selected.contains(&scope))
    })
}

/// 根据时段树显式扩展查询范围；默认调用方传 false 时不做任何继承或插值。
pub fn expand_period_scope_refs(
    timeline: &crate::timeline::Timeline,
    selected: &[TargetRef],
    include_children: bool,
) -> Vec<TargetRef> {
    let mut out = selected.iter().cloned().collect::<BTreeSet<_>>();
    if !include_children {
        return out.into_iter().collect();
    }
    let mut queue = selected
        .iter()
        .filter(|target| target.kind == "period")
        .map(|target| target.id.clone())
        .collect::<VecDeque<_>>();
    while let Some(parent) = queue.pop_front() {
        for child in timeline
            .periods
            .iter()
            .filter(|period| period.parent.as_deref() == Some(parent.as_str()))
        {
            let target = TargetRef::new("period", &child.id);
            if out.insert(target) {
                queue.push_back(child.id.clone());
            }
        }
    }
    out.into_iter().collect()
}

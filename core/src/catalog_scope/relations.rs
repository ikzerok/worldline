use super::*;
use crate::relations::{
    RelationQueryContinuation, RelationQueryDirection, RelationQueryEdge, RelationQueryNode,
    RelationQueryOptions, RelationQueryResult,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

impl CatalogScopeSnapshot {
    /// 只遍历冻结正式邻接索引。普通提及、地图几何及旧显示句柄不参与。
    pub fn query_relations(
        &self,
        target: &TargetRef,
        options: RelationQueryOptions,
    ) -> RelationQueryResult {
        let options = options.bounded();
        let mut nodes = BTreeMap::from([(target.clone(), 0u8)]);
        let mut discovered = nodes.clone();
        let mut queue = VecDeque::from([(target.clone(), 0u8)]);
        let mut emitted = BTreeSet::new();
        let mut edges = Vec::new();
        let mut frontier = BTreeSet::new();
        let mut position = 0;
        let mut truncated = false;
        'walk: while let Some((current, depth)) = queue.pop_front() {
            if depth >= options.depth {
                continue;
            }
            let Some(object) = self.object(&current) else {
                continue;
            };
            for index in &object.relations {
                let Some(relation) = self.relations.get(*index) else {
                    continue;
                };
                if !self.relation_matches(relation, &current, &options) || !emitted.insert(*index) {
                    continue;
                }
                let reverse = relation.from_ref != current;
                let next = if reverse {
                    &relation.from_ref
                } else {
                    &relation.to_ref
                };
                if !discovered.contains_key(next) {
                    discovered.insert(next.clone(), depth + 1);
                    queue.push_back((next.clone(), depth + 1));
                }
                if position < options.offset {
                    position += 1;
                    continue;
                }
                let additional = usize::from(!nodes.contains_key(&current))
                    + usize::from(next != &current && !nodes.contains_key(next));
                if edges.len() >= options.max_edges || nodes.len() + additional > options.max_nodes
                {
                    truncated = true;
                    frontier.insert(current.clone());
                    frontier.insert(next.clone());
                    break 'walk;
                }
                nodes.insert(current.clone(), discovered[&current]);
                nodes.insert(next.clone(), discovered[next]);
                edges.push(RelationQueryEdge {
                    id: relation.id.clone(),
                    relation_type: relation.relation_type.clone(),
                    from_ref: relation.from_ref.clone(),
                    to_ref: relation.to_ref.clone(),
                    label: if reverse {
                        relation.inverse_label.clone()
                    } else {
                        relation.label.clone()
                    },
                    direction: relation.direction,
                    source_note: relation.source_note.clone(),
                    file: relation.source.file.clone(),
                    line: relation.source.line,
                });
                position += 1;
            }
        }
        let mut nodes = nodes
            .into_iter()
            .map(|(target, depth)| RelationQueryNode { target, depth })
            .collect::<Vec<_>>();
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
    fn relation_matches(
        &self,
        edge: &ScopeRelation,
        current: &TargetRef,
        options: &RelationQueryOptions,
    ) -> bool {
        if options
            .relation_type
            .as_ref()
            .is_some_and(|kind| kind != &edge.relation_type)
            || (!options.relation_types.is_empty()
                && !options.relation_types.contains(&edge.relation_type))
        {
            return false;
        }
        if edge.direction != RelationDirection::Undirected
            && !((edge.from_ref == *current
                && options.direction != RelationQueryDirection::Incoming)
                || (edge.to_ref == *current
                    && options.direction != RelationQueryDirection::Outgoing))
        {
            return false;
        }
        if options.scope_refs.is_empty() {
            return true;
        }
        if edge.scope_refs.is_empty() {
            return options.include_unscoped;
        }
        let dimension = |target: &TargetRef| {
            self.object(target).map_or_else(
                || {
                    if matches!(target.kind.as_str(), "event" | "scene") {
                        "story".into()
                    } else {
                        target.kind.clone()
                    }
                },
                |object| object.scope_dimension.clone(),
            )
        };
        let mut requested = BTreeMap::<String, Vec<&TargetRef>>::new();
        for target in &options.scope_refs {
            requested.entry(dimension(target)).or_default().push(target);
        }
        requested.into_iter().all(|(kind, targets)| {
            edge.scope_refs
                .iter()
                .any(|target| dimension(target) == kind && targets.contains(&target))
        })
    }
}

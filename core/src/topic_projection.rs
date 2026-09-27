use crate::analysis::Analysis;
use crate::catalog::TargetRef;
use crate::relations::{
    expand_period_scope_refs, relation_matches_scope, RelationQueryContinuation,
    RelationQueryDirection, RelationQueryNode, RelationQueryOptions,
};
use crate::timeline::TemporalEdge;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TopicProjectionOptions {
    /// Explicit relation_type ID -> caller-owned role label mapping.
    pub role_mapping: BTreeMap<String, String>,
    pub offset: usize,
    pub history_offset: usize,
    pub depth: u8,
    pub direction: RelationQueryDirection,
    pub scope_refs: Vec<TargetRef>,
    pub include_unscoped: bool,
    pub include_period_children: bool,
    pub max_nodes: usize,
    pub max_edges: usize,
}

impl Default for TopicProjectionOptions {
    fn default() -> Self {
        Self {
            role_mapping: BTreeMap::new(),
            offset: 0,
            history_offset: 0,
            depth: 1,
            direction: RelationQueryDirection::Both,
            scope_refs: Vec::new(),
            include_unscoped: false,
            include_period_children: false,
            max_nodes: 250,
            max_edges: 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopicProjectionError {
    UnknownTarget(TargetRef),
    UnknownScope(TargetRef),
    UnknownRelationType(String),
    EmptyRoleLabel(String),
}

impl TopicProjectionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownTarget(_) => "UNKNOWN_TARGET",
            Self::UnknownScope(_) => "UNKNOWN_SCOPE",
            Self::UnknownRelationType(_) => "UNKNOWN_RELATION_TYPE",
            Self::EmptyRoleLabel(_) => "EMPTY_ROLE_LABEL",
        }
    }
}

impl fmt::Display for TopicProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTarget(target) => {
                write!(f, "目标对象不存在 {}:{}", target.kind, target.id)
            }
            Self::UnknownScope(scope) => {
                write!(f, "范围对象不存在 {}:{}", scope.kind, scope.id)
            }
            Self::UnknownRelationType(id) => write!(f, "关系类型 `{id}` 不存在"),
            Self::EmptyRoleLabel(id) => write!(f, "关系类型 `{id}` 的角色标签不能为空"),
        }
    }
}

impl std::error::Error for TopicProjectionError {}

#[derive(Debug, Clone, Serialize)]
pub struct TopicProjectionEdge {
    pub id: String,
    pub relation_type: String,
    pub role: String,
    pub scope_refs: Vec<TargetRef>,
    pub from_ref: TargetRef,
    pub to_ref: TargetRef,
    pub label: String,
    pub direction: crate::relations::RelationDirection,
    pub source_note: Option<String>,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopicProjectionContinuation {
    pub relation: RelationQueryContinuation,
    pub role_mapping: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopicProjectionRelationResult {
    pub depth: u8,
    pub nodes: Vec<RelationQueryNode>,
    pub edges: Vec<TopicProjectionEdge>,
    pub cycle_hint: bool,
    pub truncated: bool,
    pub continuation: Option<TopicProjectionContinuation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TopicProjectionHistorySource {
    With,
    Relation {
        id: String,
        relation_type: String,
        role: String,
        scope_refs: Vec<TargetRef>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct TopicProjectionHistoryItem {
    pub event: TargetRef,
    pub source: TopicProjectionHistorySource,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicProjectionTimeStatus {
    Unknown,
    PeriodRanked,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopicProjectionHistoryEvent {
    pub target: TargetRef,
    pub file: String,
    pub line: u32,
    pub time_status: TopicProjectionTimeStatus,
    pub period: Option<TargetRef>,
    pub rank: Option<u32>,
    pub anchors: Vec<TargetRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopicProjectionHistory {
    pub items: Vec<TopicProjectionHistoryItem>,
    pub events: Vec<TopicProjectionHistoryEvent>,
    pub temporal_edges: Vec<TemporalEdge>,
    pub parallel_groups: Vec<Vec<TargetRef>>,
    pub target_anchors: Vec<TargetRef>,
    pub offset: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopicProjectionResult {
    pub schema_version: u32,
    pub target: TargetRef,
    pub relations: TopicProjectionRelationResult,
    pub history: TopicProjectionHistory,
    pub truncated: bool,
}

struct HistoryCandidate {
    event: TargetRef,
    source: TopicProjectionHistorySource,
    file: String,
    line: u32,
    relation_id: Option<String>,
}

impl Analysis {
    /// Combines explicitly mapped semantic relations with explicit character/place history.
    pub fn query_topic_projection(
        &self,
        target: &TargetRef,
        options: TopicProjectionOptions,
    ) -> Result<TopicProjectionResult, TopicProjectionError> {
        if self.catalog.object(target).is_none() {
            return Err(TopicProjectionError::UnknownTarget(target.clone()));
        }
        for scope in &options.scope_refs {
            if self.catalog.object(scope).is_none() {
                return Err(TopicProjectionError::UnknownScope(scope.clone()));
            }
        }
        for (relation_type, role) in &options.role_mapping {
            if !self.catalog.relation_types.contains_key(relation_type) {
                return Err(TopicProjectionError::UnknownRelationType(
                    relation_type.clone(),
                ));
            }
            if role.trim().is_empty() {
                return Err(TopicProjectionError::EmptyRoleLabel(relation_type.clone()));
            }
        }

        let scope_refs = expand_period_scope_refs(
            &self.timeline,
            &options.scope_refs,
            options.include_period_children,
        );
        let relation_options = RelationQueryOptions {
            offset: options.offset,
            depth: options.depth,
            relation_types: options.role_mapping.keys().cloned().collect(),
            scope_refs,
            include_unscoped: options.include_unscoped,
            direction: options.direction,
            max_nodes: options.max_nodes,
            max_edges: options.max_edges,
            ..RelationQueryOptions::default()
        }
        .bounded();

        let relation_query = if options.role_mapping.is_empty() {
            crate::relations::RelationQueryResult {
                schema_version: 1,
                target: target.clone(),
                depth: relation_options.depth,
                nodes: vec![RelationQueryNode {
                    target: target.clone(),
                    depth: 0,
                }],
                edges: Vec::new(),
                truncated: false,
                continuation: None,
            }
        } else {
            self.catalog
                .query_relations(target, relation_options.clone())
        };
        let relation_edges: Vec<_> = relation_query
            .edges
            .into_iter()
            .map(|edge| {
                let relation = self
                    .catalog
                    .relations
                    .get(&edge.id)
                    .expect("关系查询边必须属于当前目录");
                TopicProjectionEdge {
                    role: options.role_mapping[&edge.relation_type].clone(),
                    scope_refs: relation.scope_refs.clone(),
                    id: edge.id,
                    relation_type: edge.relation_type,
                    from_ref: edge.from_ref,
                    to_ref: edge.to_ref,
                    label: edge.label,
                    direction: edge.direction,
                    source_note: edge.source_note,
                    file: edge.file,
                    line: edge.line,
                }
            })
            .collect();
        let cycle_hint =
            directed_cycle_hint(&relation_edges) || undirected_cycle_hint(&relation_edges);
        let relations = TopicProjectionRelationResult {
            depth: relation_query.depth,
            nodes: relation_query.nodes,
            edges: relation_edges,
            cycle_hint,
            truncated: relation_query.truncated,
            continuation: relation_query
                .continuation
                .filter(|continuation| continuation.offset > options.offset)
                .map(|relation| TopicProjectionContinuation {
                    relation,
                    role_mapping: options.role_mapping.clone(),
                }),
        };
        let history = self.query_history(target, &options, &relation_options);
        let truncated = relations.truncated || history.truncated;
        Ok(TopicProjectionResult {
            schema_version: 1,
            target: target.clone(),
            relations,
            history,
            truncated,
        })
    }

    fn query_history(
        &self,
        target: &TargetRef,
        options: &TopicProjectionOptions,
        relation_options: &RelationQueryOptions,
    ) -> TopicProjectionHistory {
        let mut candidates = Vec::new();
        if target.kind == "character" {
            if relation_options.scope_refs.is_empty() || relation_options.include_unscoped {
                for node in &self.graph.nodes {
                    if node.is_event && node.characters.iter().any(|id| id == &target.id) {
                        let event = TargetRef::new("event", &node.name);
                        candidates.push(HistoryCandidate {
                            event,
                            source: TopicProjectionHistorySource::With,
                            file: node.file.clone(),
                            line: node.line,
                            relation_id: None,
                        });
                    }
                }
            }
        } else if target.kind == "entity"
            && self
                .catalog
                .entities
                .get(&target.id)
                .is_some_and(|entity| entity.entity_type == "place")
        {
            for relation in self.catalog.relations.values() {
                let Some(role) = options.role_mapping.get(&relation.relation_type) else {
                    continue;
                };
                if relation.from_ref.kind != "event"
                    || relation.to_ref != *target
                    || !relation_matches_scope(&self.catalog, relation, relation_options)
                {
                    continue;
                }
                candidates.push(HistoryCandidate {
                    event: relation.from_ref.clone(),
                    source: TopicProjectionHistorySource::Relation {
                        id: relation.id.clone(),
                        relation_type: relation.relation_type.clone(),
                        role: role.clone(),
                        scope_refs: relation.scope_refs.clone(),
                    },
                    file: relation.file.clone(),
                    line: relation.line,
                    relation_id: Some(relation.id.clone()),
                });
            }
        }
        candidates.sort_by(|left, right| {
            (&left.event, &left.relation_id).cmp(&(&right.event, &right.relation_id))
        });

        let max_nodes = relation_options.max_nodes;
        let max_edges = relation_options.max_edges;
        let mut items = Vec::new();
        let mut event_targets = BTreeSet::new();
        let mut consumed = options.history_offset;
        let mut truncated = false;
        for candidate in candidates.iter().skip(options.history_offset) {
            if items.len() >= max_edges {
                truncated = true;
                break;
            }
            if !event_targets.contains(&candidate.event) && event_targets.len() >= max_nodes {
                truncated = true;
                break;
            }
            event_targets.insert(candidate.event.clone());
            items.push(TopicProjectionHistoryItem {
                event: candidate.event.clone(),
                source: candidate.source.clone(),
                file: candidate.file.clone(),
                line: candidate.line,
            });
            consumed += 1;
        }
        let next_offset = (truncated && consumed > options.history_offset).then_some(consumed);
        let event_ids: BTreeSet<_> = event_targets
            .iter()
            .map(|target| target.id.as_str())
            .collect();
        let mut temporal_by_event = HashMap::with_capacity(event_ids.len());
        for temporal in &self.timeline.events {
            if event_ids.contains(temporal.event.as_str()) {
                temporal_by_event.insert(temporal.event.as_str(), temporal);
            }
        }
        let mut event_data = BTreeMap::new();
        for target in &event_targets {
            let Some(object) = self.catalog.object(target) else {
                continue;
            };
            let temporal = temporal_by_event.get(target.id.as_str()).copied();
            let (time_status, period, rank) = match temporal {
                Some(temporal) => (
                    TopicProjectionTimeStatus::PeriodRanked,
                    Some(TargetRef::new("period", &temporal.period)),
                    Some(temporal.rank),
                ),
                None => (TopicProjectionTimeStatus::Unknown, None, None),
            };
            let mut anchors: Vec<_> = self
                .catalog
                .anchors_for(target)
                .into_iter()
                .map(|anchor| TargetRef::new("anchor", &anchor.id))
                .collect();
            anchors.sort();
            event_data.insert(
                target.clone(),
                TopicProjectionHistoryEvent {
                    target: target.clone(),
                    file: object.file.clone(),
                    line: object.line,
                    time_status,
                    period,
                    rank,
                    anchors,
                },
            );
        }
        let mut temporal_edges: Vec<_> = self
            .timeline
            .edges
            .iter()
            .filter(|edge| {
                event_ids.contains(edge.before.as_str()) && event_ids.contains(edge.after.as_str())
            })
            .cloned()
            .collect();
        temporal_edges
            .sort_by(|left, right| (&left.before, &left.after).cmp(&(&right.before, &right.after)));

        let mut parallel = BTreeMap::<(String, u32), Vec<TargetRef>>::new();
        for event in event_data.values() {
            if let (Some(period), Some(rank)) = (&event.period, event.rank) {
                parallel
                    .entry((period.id.clone(), rank))
                    .or_default()
                    .push(event.target.clone());
            }
        }
        let parallel_groups = parallel
            .into_values()
            .filter(|events| events.len() > 1)
            .collect();
        let mut target_anchors: Vec<_> = self
            .catalog
            .anchors_for(target)
            .into_iter()
            .map(|anchor| TargetRef::new("anchor", &anchor.id))
            .collect();
        target_anchors.sort();

        TopicProjectionHistory {
            items,
            events: event_data.into_values().collect(),
            temporal_edges,
            parallel_groups,
            target_anchors,
            offset: options.history_offset,
            truncated,
            next_offset,
        }
    }
}

fn directed_cycle_hint(edges: &[TopicProjectionEdge]) -> bool {
    let mut adjacency = BTreeMap::<TargetRef, BTreeSet<TargetRef>>::new();
    for edge in edges
        .iter()
        .filter(|edge| edge.direction == crate::relations::RelationDirection::Directed)
    {
        adjacency
            .entry(edge.from_ref.clone())
            .or_default()
            .insert(edge.to_ref.clone());
        adjacency.entry(edge.to_ref.clone()).or_default();
    }

    fn visit(
        node: &TargetRef,
        adjacency: &BTreeMap<TargetRef, BTreeSet<TargetRef>>,
        colors: &mut BTreeMap<TargetRef, u8>,
    ) -> bool {
        colors.insert(node.clone(), 1);
        if let Some(next_nodes) = adjacency.get(node) {
            for next in next_nodes {
                match colors.get(next).copied() {
                    Some(1) => return true,
                    Some(2) => continue,
                    None if visit(next, adjacency, colors) => return true,
                    _ => {}
                }
            }
        }
        colors.insert(node.clone(), 2);
        false
    }

    let mut colors = BTreeMap::new();
    adjacency
        .keys()
        .any(|node| !colors.contains_key(node) && visit(node, &adjacency, &mut colors))
}

fn undirected_cycle_hint(edges: &[TopicProjectionEdge]) -> bool {
    let mut adjacency = BTreeMap::<TargetRef, BTreeSet<TargetRef>>::new();
    for edge in edges {
        adjacency
            .entry(edge.from_ref.clone())
            .or_default()
            .insert(edge.to_ref.clone());
        adjacency
            .entry(edge.to_ref.clone())
            .or_default()
            .insert(edge.from_ref.clone());
    }

    fn visit(
        node: &TargetRef,
        parent: Option<&TargetRef>,
        adjacency: &BTreeMap<TargetRef, BTreeSet<TargetRef>>,
        visited: &mut BTreeSet<TargetRef>,
    ) -> bool {
        visited.insert(node.clone());
        if let Some(neighbors) = adjacency.get(node) {
            for next in neighbors {
                if Some(next) == parent {
                    continue;
                }
                if visited.contains(next) || visit(next, Some(node), adjacency, visited) {
                    return true;
                }
            }
        }
        false
    }

    let mut visited = BTreeSet::new();
    adjacency
        .keys()
        .any(|node| !visited.contains(node) && visit(node, None, &adjacency, &mut visited))
}

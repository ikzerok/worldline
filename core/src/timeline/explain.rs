use super::{evidence::EdgeGraph, TemporalEdge, Timeline, TimelineStatus};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalRelation {
    SameEvent,
    Before,
    After,
    UnorderedSameRoot,
    DifferentRoots,
    Invalid,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalComparisonReason {
    PartialTimeline,
    UnknownEvent,
    MissingTime,
    InvalidTimeRoot,
    DifferentOrderScopes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemporalComparison {
    pub left: String,
    pub right: String,
    pub relation: TemporalRelation,
    pub reason: Option<TemporalComparisonReason>,
    /// 始终按真实 before→after 方向；After 时从 right 到 left。
    pub evidence: Vec<TemporalEdge>,
    pub status: TimelineStatus,
}

impl Timeline {
    /// 只读解释当前编译快照；源码改变后调用方必须重新编译，不能沿用旧证据。
    pub fn compare(&self, left: &str, right: &str) -> TemporalComparison {
        use TemporalComparisonReason as Reason;
        use TemporalRelation as Relation;
        let mut result = TemporalComparison {
            left: left.into(),
            right: right.into(),
            relation: Relation::Invalid,
            reason: Some(Reason::PartialTimeline),
            evidence: Vec::new(),
            status: self.status,
        };
        if self.status != TimelineStatus::Complete {
            return result;
        }
        let left_event = self.events.iter().find(|e| e.event == left);
        let right_event = self.events.iter().find(|e| e.event == right);
        let known = |id: &str| self.unplaced_events.iter().any(|e| e == id);
        if (left_event.is_none() && !known(left)) || (right_event.is_none() && !known(right)) {
            result.relation = Relation::Unknown;
            result.reason = Some(Reason::UnknownEvent);
            return result;
        }
        if left == right {
            result.relation = Relation::SameEvent;
            result.reason = None;
            return result;
        }
        let (Some(left), Some(right)) = (left_event, right_event) else {
            result.relation = Relation::Unknown;
            result.reason = Some(Reason::MissingTime);
            return result;
        };
        if left.status != TimelineStatus::Complete || right.status != TimelineStatus::Complete {
            return result;
        }
        if left.root.is_none()
            || right.root.is_none()
            || left.order_scope.is_none()
            || right.order_scope.is_none()
        {
            result.reason = Some(Reason::InvalidTimeRoot);
            return result;
        }
        result.reason = None;
        if left.root != right.root {
            result.relation = Relation::DifferentRoots;
        } else if left.order_scope != right.order_scope {
            result.relation = Relation::Unknown;
            result.reason = Some(Reason::DifferentOrderScopes);
        } else {
            result.relation = Relation::UnorderedSameRoot;
            let graph = EdgeGraph::new(&self.edges);
            if let (Some(&a), Some(&b)) = (
                graph.indices.get(left.event.as_str()),
                graph.indices.get(right.event.as_str()),
            ) {
                if let Some(path) = graph.path(a, b, None) {
                    result.relation = Relation::Before;
                    result.evidence = path;
                } else if let Some(path) = graph.path(b, a, None) {
                    result.relation = Relation::After;
                    result.evidence = path;
                }
            }
        }
        result
    }
}

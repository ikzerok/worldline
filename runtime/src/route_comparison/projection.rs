use super::*;
use crate::{AccessCoverage, ReplayOrigin, ReplayStatus, ReplayTrace, Story};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use worldline_core::{evidence_source::resolve_evidence_sources, CompileResult};

pub(super) fn digest(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}
pub(super) fn snapshot_digest(snapshot: &CompileResult) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for (path, text) in &snapshot.sources {
        for bytes in [path.to_string_lossy().as_bytes(), text.as_bytes()] {
            for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
        }
    }
    format!("{hash:016x}")
}
pub(super) fn origin(trace: &ReplayTrace) -> RouteOriginSummary {
    match &trace.origin {
        ReplayOrigin::Entry { seed } => RouteOriginSummary {
            kind: "entry".into(),
            seed: crate::util::normalize_seed(*seed),
            checkpoint_fingerprint: None,
            checkpoint_digest: None,
        },
        ReplayOrigin::Checkpoint { checkpoint } => RouteOriginSummary {
            kind: "checkpoint".into(),
            seed: crate::util::normalize_seed(checkpoint.seed),
            checkpoint_fingerprint: Some(checkpoint.fingerprint),
            checkpoint_digest: Some(digest(checkpoint.state.as_bytes())),
        },
    }
}
pub(super) fn empty_result(
    trace: &ReplayTrace,
    status: RouteStatus,
    detail: Option<String>,
) -> RouteSideResult {
    RouteSideResult {
        origin: origin(trace),
        original_fingerprint: trace.fingerprint,
        status,
        ended: false,
        complete: false,
        executed_steps: 0,
        completed_choices: 0,
        current_node: None,
        detail,
        divergence_step: None,
        states: None,
        vars: None,
        coverage: RouteCoverage::default(),
        state_actions: StateActionEvidence::default(),
        variable_writes: VariableWriteEvidence::default(),
        omitted: false,
    }
}
pub(super) fn status(status: ReplayStatus) -> (RouteStatus, Option<String>, Option<usize>, bool) {
    match status {
        ReplayStatus::Replayed { complete, .. } => (RouteStatus::Replayed, None, None, complete),
        ReplayStatus::Diverged {
            reason, step_index, ..
        } => (RouteStatus::Diverged, Some(reason), Some(step_index), false),
        ReplayStatus::StepBudgetExceeded => (RouteStatus::StepBudgetExceeded, None, None, false),
        ReplayStatus::TimeBudgetExceeded => (RouteStatus::TimeBudgetExceeded, None, None, false),
        ReplayStatus::Cancelled => (RouteStatus::Cancelled, None, None, false),
        ReplayStatus::IncompleteTrace => (RouteStatus::IncompleteTrace, None, None, false),
        ReplayStatus::StoryFailed { message, .. } => {
            (RouteStatus::StoryFailed, Some(message), None, false)
        }
    }
}
pub(super) fn result(
    trace: &ReplayTrace,
    story: &Story<'_>,
    inherited: &AccessCoverage,
    outcome: (RouteStatus, Option<String>, Option<usize>, bool),
    steps: u64,
    choices: usize,
) -> RouteSideResult {
    let total = story.access_coverage();
    let mut executed = total.clone();
    for (node, count) in &mut executed.visited_nodes {
        *count = count.saturating_sub(*inherited.visited_nodes.get(node).unwrap_or(&0));
    }
    executed.visited_nodes.retain(|_, count| *count > 0);
    for choice in &mut executed.selected_choices {
        choice.count = choice.count.saturating_sub(
            inherited
                .selected_choices
                .iter()
                .find(|old| old.id == choice.id)
                .map_or(0, |old| old.count),
        );
    }
    executed.selected_choices.retain(|choice| choice.count > 0);
    RouteSideResult {
        origin: origin(trace),
        original_fingerprint: trace.fingerprint,
        status: outcome.0,
        ended: story.is_ended(),
        complete: outcome.3,
        executed_steps: steps,
        completed_choices: choices,
        current_node: story.current_node(),
        detail: outcome.1,
        divergence_step: outcome.2,
        states: Some(story.states().clone()),
        vars: Some(
            story
                .vars()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
        coverage: RouteCoverage {
            inherited: inherited.clone(),
            executed,
            total,
        },
        state_actions: story.state_action_evidence().clone(),
        variable_writes: story.variable_write_evidence().clone(),
        omitted: story.state_action_evidence().omitted || story.variable_write_evidence().omitted,
    }
}
pub(super) fn differences<T: Serialize + PartialEq>(
    left: Option<&BTreeMap<String, T>>,
    right: Option<&BTreeMap<String, T>>,
) -> Vec<RouteValueDifference> {
    let (Some(left), Some(right)) = (left, right) else {
        return Vec::new();
    };
    left.keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            let (a, b) = (left.get(id), right.get(id));
            (a != b).then(|| RouteValueDifference {
                id: id.clone(),
                left: a.map(|value| serde_json::to_value(value).expect("runtime值可序列化")),
                right: b.map(|value| serde_json::to_value(value).expect("runtime值可序列化")),
            })
        })
        .collect()
}
pub(super) fn alignment(left: &side::Side, right: &side::Side) -> RouteAlignment {
    let a = left.cursor.as_ref();
    let b = right.cursor.as_ref();
    let count_a = a.map_or(0, |cursor| cursor.verified_choices.len());
    let count_b = b.map_or(0, |cursor| cursor.verified_choices.len());
    let compatible = match (&left.trace.origin, &right.trace.origin) {
        (ReplayOrigin::Entry { seed: a }, ReplayOrigin::Entry { seed: b }) => {
            crate::util::normalize_seed(*a) == crate::util::normalize_seed(*b)
        }
        (
            ReplayOrigin::Checkpoint { checkpoint: a },
            ReplayOrigin::Checkpoint { checkpoint: b },
        ) => {
            a.schema_version == b.schema_version
                && a.runtime_version == b.runtime_version
                && a.fingerprint == b.fingerprint
                && crate::util::normalize_seed(a.seed) == crate::util::normalize_seed(b.seed)
                && worldline_core::parse_unique_json(a.state.as_bytes()).ok()
                    == worldline_core::parse_unique_json(b.state.as_bytes()).ok()
        }
        _ => false,
    };
    let verified = a.is_some_and(|cursor| cursor.initial_verified)
        && b.is_some_and(|cursor| cursor.initial_verified);
    let mut alignment = RouteAlignment {
        comparable: compatible && verified,
        reason: if !compatible {
            Some("起点或随机种子不同，只能并列实际结果".into())
        } else if !verified {
            Some("至少一侧尚未验证起点，没有可对齐的真实前缀".into())
        } else {
            None
        },
        common_prefix: 0,
        first_difference: None,
        left_verified_choices: count_a,
        right_verified_choices: count_b,
    };
    if !alignment.comparable {
        return alignment;
    }
    for (index, (a, b)) in a
        .unwrap()
        .verified_choices
        .iter()
        .zip(&b.unwrap().verified_choices)
        .enumerate()
    {
        if a.choice.id != b.choice.id {
            let a = a.clone();
            let b = b.clone();
            alignment.first_difference = Some(RouteChoiceDifference {
                index,
                left: a,
                right: b,
            });
            break;
        }
        alignment.common_prefix += 1;
    }
    alignment
}

pub(super) fn verify_sources(
    snapshot: &CompileResult,
    alignment: &mut RouteAlignment,
    left: &mut RouteSideResult,
    right: &mut RouteSideResult,
) -> Result<(), RouteComparisonError> {
    let mut slots = Vec::new();
    if let Some(difference) = &mut alignment.first_difference {
        slots.push(&mut difference.left.source);
        slots.push(&mut difference.right.source);
    }
    for side in [left, right] {
        for record in &mut side.state_actions.records {
            slots.push(&mut record.source);
        }
        for record in &mut side.variable_writes.records {
            slots.push(&mut record.source);
        }
    }
    let sources = slots
        .iter()
        .filter_map(|source| source.as_ref())
        .collect::<Vec<_>>();
    let results = resolve_evidence_sources(snapshot, &sources)
        .map_err(|message| RouteComparisonError::new("output_limit", message))?;
    drop(sources);
    let mut results = results.into_iter();
    for source in slots {
        if source.is_some() && results.next().expect("批量结果与来源数一致").is_err() {
            *source = None;
        }
    }
    Ok(())
}

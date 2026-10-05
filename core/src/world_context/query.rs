use super::*;
use crate::{wiki::KeywordIndex, CompileResult};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

impl CompileResult {
    pub fn query_world_context(
        &self,
        target: &TargetRef,
        options: WorldContextOptions,
    ) -> Result<WorldContextResult, WorldContextError> {
        self.query_world_context_cancellable(target, options, || false)
    }

    pub fn query_world_context_cancellable<F: FnMut() -> bool>(
        &self,
        target: &TargetRef,
        options: WorldContextOptions,
        mut cancelled: F,
    ) -> Result<WorldContextResult, WorldContextError> {
        options.validate()?;
        if cancelled() {
            return Err(WorldContextError::Cancelled);
        }
        let snapshot = self.world_context_snapshot();
        if options
            .expected_snapshot
            .as_ref()
            .is_some_and(|expected| expected != &snapshot)
        {
            return Err(WorldContextError::StaleSnapshot);
        }
        let object = self.lookup_world_object(target)?;
        let wiki = (options.include_text_mentions
            && (options.kinds.is_empty()
                || options.kinds.contains(&WorldContextKind::TextMention)))
        .then(|| KeywordIndex::new(self));
        let mut depths = BTreeMap::from([(target.clone(), 0u8)]);
        let mut queue = VecDeque::from([(target.clone(), 0u8)]);
        let mut scheduled = BTreeSet::from([target.clone()]);
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        let mut budget_exhausted = false;
        let executable = options.executable_enabled();
        let index = &self.analysis.executable_context;
        let mut source_unavailable = executable && index.source_unavailable;
        let mut source_lines = BTreeMap::new();
        while let Some((current, depth)) = queue.pop_front() {
            if cancelled() {
                return Err(WorldContextError::Cancelled);
            }
            if depth >= options.depth {
                continue;
            }
            let mut was_cancelled = false;
            let mut candidates = Vec::new();
            let mut collect_record = |record: Option<WorldContextRecord>| {
                if cancelled() {
                    was_cancelled = true;
                    return false;
                }
                let Some(record) = record else {
                    return true;
                };
                if !matches_direction(&record, &current, options.direction)
                    || (!options.kinds.is_empty() && !options.kinds.contains(&record.kind))
                    || seen.contains(&record.id)
                {
                    return true;
                }
                if seen.len() >= options.max_candidates {
                    budget_exhausted = true;
                    return false;
                }
                seen.insert(record.id.clone());
                candidates.push(record);
                true
            };
            let finished = collect::visit(self, &current, wiki.as_ref(), &mut collect_record)
                && (!executable
                    || index.visit(
                        self,
                        &current,
                        &mut source_unavailable,
                        &mut source_lines,
                        &mut collect_record,
                    ));
            if was_cancelled {
                return Err(WorldContextError::Cancelled);
            }
            sort_records(&mut candidates);
            for record in candidates {
                for endpoint in [&record.from_ref, &record.to_ref] {
                    if !depths.contains_key(endpoint) {
                        depths.insert(endpoint.clone(), depth + 1);
                    }
                    // 线索节点与语义遍历分别计深度，避免一次文字命中改变后续正式两跳范围。
                    if record.kind != WorldContextKind::TextMention
                        && scheduled.insert(endpoint.clone())
                    {
                        queue.push_back((endpoint.clone(), depth + 1));
                    }
                }
                found.push(record);
            }
            if !finished {
                break;
            }
        }
        let index_limited = executable && index.limited;
        let total =
            (!budget_exhausted && !index_limited && !source_unavailable).then_some(found.len());
        let mut reasons = Vec::new();
        if self.has_errors() {
            reasons.push(WorldContextLimit::InvalidSource);
        }
        if budget_exhausted {
            reasons.push(WorldContextLimit::CandidateBudget);
        }
        if index_limited {
            reasons.push(WorldContextLimit::ExecutableIndexBudget);
        }
        if source_unavailable {
            reasons.push(WorldContextLimit::SourceUnavailable);
        }
        let mut included = BTreeSet::from([target.clone()]);
        let mut records = Vec::new();
        for record in found {
            if records.len() >= options.max_records {
                add_reason(&mut reasons, WorldContextLimit::RecordLimit);
                break;
            }
            let additional = [&record.from_ref, &record.to_ref]
                .into_iter()
                .filter(|endpoint| !included.contains(*endpoint))
                .collect::<BTreeSet<_>>()
                .len();
            if included.len() + additional > options.max_nodes {
                add_reason(&mut reasons, WorldContextLimit::NodeLimit);
                continue;
            }
            included.insert(record.from_ref.clone());
            included.insert(record.to_ref.clone());
            records.push(record);
        }
        sort_records(&mut records);
        let mut nodes: Vec<_> = included
            .into_iter()
            .map(|target| {
                let object = self.analysis.catalog.object(&target);
                WorldContextNode {
                    display: object
                        .map(|object| object.display.clone())
                        .unwrap_or_else(|| target.id.clone()),
                    depth: depths[&target],
                    exists: object.is_some(),
                    file: object.map(|object| object.file.clone()),
                    line: object.map(|object| object.line),
                    target,
                }
            })
            .collect();
        nodes.sort_by(|a, b| (a.depth, &a.target).cmp(&(b.depth, &b.target)));
        let truncated = reasons.iter().any(|reason| {
            !matches!(
                reason,
                WorldContextLimit::InvalidSource
                    | WorldContextLimit::SourceConflict
                    | WorldContextLimit::SourceUnavailable
            )
        });
        Ok(WorldContextResult {
            schema_version: 1,
            target: target.clone(),
            object,
            snapshot,
            content_baseline: None,
            nodes,
            total,
            returned: records.len(),
            records,
            complete: reasons.is_empty(),
            truncated,
            reasons,
            diagnostics: self.diagnostics.clone(),
        })
    }
}

fn matches_direction(
    record: &WorldContextRecord,
    current: &TargetRef,
    direction: RelationQueryDirection,
) -> bool {
    record.direction == RelationDirection::Undirected
        || match direction {
            RelationQueryDirection::Both => true,
            RelationQueryDirection::Outgoing => &record.from_ref == current,
            RelationQueryDirection::Incoming => &record.to_ref == current,
        }
}
fn sort_records(records: &mut [WorldContextRecord]) {
    records.sort_by(|a, b| {
        (
            a.kind,
            &a.source.file,
            a.source.line,
            a.source.column,
            &a.from_ref,
            &a.to_ref,
            &a.id,
        )
            .cmp(&(
                b.kind,
                &b.source.file,
                b.source.line,
                b.source.column,
                &b.from_ref,
                &b.to_ref,
                &b.id,
            ))
    });
}
fn add_reason(reasons: &mut Vec<WorldContextLimit>, reason: WorldContextLimit) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

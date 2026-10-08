use super::{capture::baseline, model::*, search::FoldedSearch};
use crate::{OwnedStory, Story, Value};

impl Story<'_> {
    pub fn inspection_stamp(&self) -> InspectionStamp {
        InspectionStamp {
            run_id: self.inspection.run_id,
            compiled_snapshot: self.inspection.run_id,
            fingerprint: self.fingerprint,
            trace_generation: self.inspection.trace_generation,
            revision: self.inspection.revision,
        }
    }
    pub fn inspect_state(
        &self,
        query: &StateInspectionQuery,
    ) -> Result<StateInspectionPage, StateInspectionError> {
        let stamp = self.inspection_stamp();
        if query.limit == 0 || query.limit > 100 || query.text.chars().count() > 256 {
            return Err(StateInspectionError {
                code: "INVALID_INSPECTION_QUERY".into(),
                message: "状态查询每页须为1–100项，检索词最多256字符".into(),
            });
        }
        if query
            .expected_stamp
            .is_some_and(|expected| expected != stamp)
        {
            return Err(StateInspectionError {
                code: "STALE_INSPECTION".into(),
                message: "运行或观察位置已变化，请重新查询第一页".into(),
            });
        }
        let history = &self.inspection;
        let previous = history.previous();
        let text = FoldedSearch::new(query.text.trim());
        let candidates = self.inspection_candidates();
        let mut page = StateInspectionPage {
            stamp,
            status: history.status,
            first_observation: history.first.as_ref().map(|o| o.number),
            previous_observation: previous.map(|o| o.number),
            current_observation: history
                .latest
                .as_ref()
                .filter(|_| history.at_recorded)
                .map(|o| o.number),
            items: Vec::new(),
            total_items: candidates.len(),
            total_matches: 0,
            incomparable_items: 0,
            offset: query.offset,
            limit: query.limit,
            next_offset: None,
            history_omitted: history
                .first
                .iter()
                .chain(history.previous.iter())
                .chain(history.latest.iter())
                .any(|o| o.omitted),
        };
        for candidate in candidates {
            if query
                .group
                .is_some_and(|group| group != candidate.key.group)
            {
                continue;
            }
            let current = candidate.value.cell();
            let first = baseline(history.first.as_deref(), &candidate.key);
            let previous = baseline(previous, &candidate.key);
            if !text.is_empty()
                && ![
                    candidate.key.name.as_str(),
                    candidate.fragment.unwrap_or(""),
                    &first.display,
                    &previous.display,
                    &current.display,
                ]
                .iter()
                .any(|field| text.contains(field))
                && !candidate.value.matches(&text)
            {
                continue;
            }
            let first_change = compare(&first, &current);
            let previous_change = compare(&previous, &current);
            let selected_change = match query.compare_to {
                InspectionBaseline::First => first_change,
                InspectionBaseline::Previous => previous_change,
            };
            page.incomparable_items +=
                usize::from(selected_change == InspectionChange::NotComparable);
            if query.changed_only && selected_change != InspectionChange::Changed {
                continue;
            }
            let matched_index = page.total_matches;
            page.total_matches += 1;
            if matched_index < query.offset || page.items.len() >= query.limit {
                continue;
            }
            page.items.push(StateInspectionItem {
                key: candidate.key,
                fragment: candidate.fragment.map(str::to_string),
                depth: candidate.depth,
                first,
                previous,
                current,
                first_change,
                previous_change,
                source: candidate.source,
            });
        }
        let next = query.offset.saturating_add(page.items.len());
        page.next_offset = (next < page.total_matches).then_some(next);
        if crate::route_comparison::encoded_size(&page, 1024 * 1024).is_err() {
            return Err(StateInspectionError {
                code: "INSPECTION_OUTPUT_LIMIT".into(),
                message: "检查页超过1MiB输出预算，请减少每页条数或缩小检索".into(),
            });
        }
        Ok(page)
    }
}
impl OwnedStory {
    pub fn inspection_stamp(&self) -> InspectionStamp {
        self.as_story().inspection_stamp()
    }
    pub fn inspect_state(
        &self,
        query: &StateInspectionQuery,
    ) -> Result<StateInspectionPage, StateInspectionError> {
        self.as_story().inspect_state(query)
    }
}
fn compare(before: &InspectionCell, after: &InspectionCell) -> InspectionChange {
    use InspectionCellStatus::{Present, Uninitialized};
    if !matches!(before.status, Present | Uninitialized)
        || !matches!(after.status, Present | Uninitialized)
    {
        return InspectionChange::NotComparable;
    }
    let equal = match (&before.value, &after.value) {
        (Some(Value::TagSet(a)), Some(Value::TagSet(b))) => {
            a.iter().collect::<std::collections::BTreeSet<_>>() == b.iter().collect()
        }
        (a, b) => a == b,
    };
    if equal && before.status == after.status {
        InspectionChange::Unchanged
    } else {
        InspectionChange::Changed
    }
}

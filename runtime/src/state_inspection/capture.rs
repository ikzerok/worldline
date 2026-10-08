use super::model::*;
use crate::{Story, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use worldline_core::state_inspection_source::{DeclarationKind, DeclarationSource};

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);
pub(crate) struct InspectionHistory {
    pub run_id: u64,
    pub trace_generation: u64,
    pub revision: u64,
    pub next_call: u64,
    pub status: InspectionStatus,
    pub at_recorded: bool,
    pub first: Option<Arc<Observation>>,
    pub previous: Option<Arc<Observation>>,
    pub latest: Option<Arc<Observation>>,
}
impl Default for InspectionHistory {
    fn default() -> Self {
        Self {
            run_id: NEXT_RUN.fetch_add(1, Ordering::Relaxed),
            trace_generation: 0,
            revision: 0,
            next_call: 0,
            status: InspectionStatus::Ready,
            at_recorded: false,
            first: None,
            previous: None,
            latest: None,
        }
    }
}
impl InspectionHistory {
    pub(crate) fn call_id(&mut self) -> u64 {
        self.next_call += 1;
        self.next_call
    }
    pub(crate) fn advancing(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.status = InspectionStatus::Advancing;
        self.at_recorded = false;
    }
    pub(crate) fn reset_trace(&mut self) {
        self.trace_generation = self.trace_generation.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        self.at_recorded = false;
        self.first = None;
        self.previous = None;
        self.latest = None;
    }
    pub(super) fn previous(&self) -> Option<&Observation> {
        if self.at_recorded {
            self.previous.as_deref()
        } else {
            self.latest.as_deref()
        }
    }
}
pub(crate) struct Observation {
    pub number: u64,
    pub values: BTreeMap<InspectionKey, InspectionCell>,
    pub calls: BTreeSet<u64>,
    pub omitted: bool,
}
pub(super) enum BorrowedValue<'a> {
    Value(&'a Value),
    Tags(&'a Vec<String>),
    Uninitialized,
}
pub(super) struct Candidate<'a> {
    pub key: InspectionKey,
    pub value: BorrowedValue<'a>,
    pub fragment: Option<&'a str>,
    pub depth: Option<usize>,
    pub source: Option<DeclarationSource>,
}
impl Story<'_> {
    pub(crate) fn inspection_record(&mut self) {
        let mut snapshot = Observation {
            number: self.inspection.latest.as_ref().map_or(1, |o| o.number + 1),
            values: BTreeMap::new(),
            calls: self
                .frames
                .iter()
                .filter(|f| f.fragment.is_some())
                .map(|f| f.inspection_call_id)
                .collect(),
            omitted: false,
        };
        let mut bytes = 0usize;
        for candidate in self.inspection_candidates() {
            if snapshot.values.len() >= MAX_INSPECTION_ROWS {
                snapshot.omitted = true;
                break;
            }
            let cell = candidate.value.cell();
            let size = crate::route_comparison::encoded_size(
                &(&candidate.key, &cell),
                MAX_INSPECTION_HISTORY_BYTES.saturating_sub(bytes),
            );
            let Ok(size) = size else {
                snapshot.omitted = true;
                break;
            };
            bytes += size;
            snapshot.omitted |= cell.status == InspectionCellStatus::Omitted;
            snapshot.values.insert(candidate.key, cell);
        }
        let snapshot = Arc::new(snapshot);
        if self.inspection.first.is_none() {
            self.inspection.first = Some(snapshot.clone());
        }
        self.inspection.previous = self.inspection.latest.take();
        self.inspection.latest = Some(snapshot);
        self.inspection.at_recorded = true;
        self.inspection.status = if self.is_paused() {
            InspectionStatus::Choice
        } else {
            InspectionStatus::Ended
        };
        self.inspection.revision = self.inspection.revision.wrapping_add(1);
    }
    pub(super) fn inspection_candidates(&self) -> Vec<Candidate<'_>> {
        let mut result = Vec::new();
        for (name, info) in &self.symbols.vars {
            result.push(Candidate {
                key: InspectionKey {
                    group: InspectionGroup::Global,
                    name: name.clone(),
                    call_id: None,
                },
                value: self
                    .vars
                    .get(name)
                    .map_or(BorrowedValue::Uninitialized, BorrowedValue::Value),
                fragment: None,
                depth: None,
                source: Some(DeclarationSource {
                    kind: DeclarationKind::GlobalVariable,
                    id: name.clone(),
                    file: info.decl_file.clone(),
                    line: info.decl_span.line,
                }),
            });
        }
        for (depth, frame) in self
            .frames
            .iter()
            .filter(|f| f.fragment.is_some())
            .enumerate()
        {
            let Some(definition) = self
                .program
                .fragments
                .iter()
                .find(|d| Some(&d.name) == frame.fragment.as_ref())
            else {
                continue;
            };
            let names = definition
                .parameters
                .iter()
                .map(|p| p.name.as_str())
                .chain(
                    worldline_core::language::locals(&definition.body)
                        .into_iter()
                        .map(|l| l.name.as_str()),
                )
                .collect::<BTreeSet<_>>();
            for name in names {
                result.push(Candidate {
                    key: InspectionKey {
                        group: InspectionGroup::Local,
                        name: name.into(),
                        call_id: Some(frame.inspection_call_id),
                    },
                    value: frame
                        .locals
                        .get(name)
                        .map_or(BorrowedValue::Uninitialized, BorrowedValue::Value),
                    fragment: frame.fragment.as_deref(),
                    depth: Some(depth + 1),
                    source: None,
                });
            }
        }
        for (name, info) in &self.catalog.states {
            result.push(Candidate {
                key: InspectionKey {
                    group: InspectionGroup::State,
                    name: name.clone(),
                    call_id: None,
                },
                value: self
                    .states
                    .get(name)
                    .map_or(BorrowedValue::Uninitialized, BorrowedValue::Tags),
                fragment: None,
                depth: None,
                source: Some(DeclarationSource {
                    kind: DeclarationKind::State,
                    id: name.clone(),
                    file: info.file.clone(),
                    line: info.line,
                }),
            });
        }
        result.sort_by(|a, b| {
            (&a.key.group, a.key.call_id, &a.key.name).cmp(&(
                &b.key.group,
                b.key.call_id,
                &b.key.name,
            ))
        });
        result
    }
}
impl BorrowedValue<'_> {
    pub(super) fn cell(&self) -> InspectionCell {
        #[derive(serde::Serialize)]
        enum Tags<'a> {
            TagSet(&'a Vec<String>),
        }
        let size = match self {
            Self::Value(value) => {
                crate::route_comparison::encoded_size(value, MAX_INSPECTION_VALUE_BYTES)
            }
            Self::Tags(tags) => crate::route_comparison::encoded_size(
                &Tags::TagSet(tags),
                MAX_INSPECTION_VALUE_BYTES,
            ),
            Self::Uninitialized => {
                return InspectionCell::missing(InspectionCellStatus::Uninitialized)
            }
        };
        if size.is_err() {
            let mut cell = InspectionCell::missing(InspectionCellStatus::Omitted);
            if let Self::Value(Value::Str(value)) = self {
                cell.display = format!(
                    "\"{}…\"（长值省略）",
                    value.chars().take(400).collect::<String>()
                );
            } else {
                cell.display = "长值省略（超过2048字节）".into();
            }
            cell.truncated = true;
            return cell;
        }
        let value = match self {
            Self::Value(value) => (*value).clone(),
            Self::Tags(tags) => Value::TagSet((*tags).clone()),
            Self::Uninitialized => unreachable!(),
        };
        let display = match &value {
            Value::Str(text) => format!("{text:?}"),
            _ => value.display(),
        };
        InspectionCell {
            status: InspectionCellStatus::Present,
            value: Some(value),
            display,
            truncated: false,
        }
    }
}
pub(super) fn baseline(observation: Option<&Observation>, key: &InspectionKey) -> InspectionCell {
    let Some(observation) = observation else {
        return InspectionCell::missing(InspectionCellStatus::Unrecorded);
    };
    if let Some(value) = observation.values.get(key) {
        return value.clone();
    }
    if key
        .call_id
        .is_some_and(|call| !observation.calls.contains(&call))
    {
        InspectionCell::missing(InspectionCellStatus::NotInScope)
    } else {
        InspectionCell::missing(InspectionCellStatus::Omitted)
    }
}

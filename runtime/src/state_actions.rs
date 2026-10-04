//! 有界作者证据，与持久化 StateRecord 完全分离。
use crate::{
    route_comparison::{encoded_size, MAX_ROUTE_EVIDENCE_BYTES, MAX_ROUTE_EVIDENCE_RECORDS},
    StateRecord, Story,
};
use serde::{Deserialize, Serialize};
use worldline_core::{
    ast::ChangeKind,
    evidence_source::{EvidenceSource, EvidenceSourceOwner},
    TargetRef,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateActionRecord {
    pub sequence: u64,
    pub kind: ChangeKind,
    pub state: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub event: Option<String>,
    pub node: Option<String>,
    pub turn: u32,
    pub note: Option<String>,
    pub target: Option<TargetRef>,
    pub source: Option<EvidenceSource>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StateActionEvidence {
    pub records: Vec<StateActionRecord>,
    pub total_actions: u64,
    pub omitted: bool,
}
pub(crate) struct StateActionCapture {
    pub evidence: StateActionEvidence,
    bytes: usize,
    record_limit: usize,
    byte_limit: usize,
}
impl Default for StateActionCapture {
    fn default() -> Self {
        Self {
            evidence: Default::default(),
            bytes: 0,
            record_limit: MAX_ROUTE_EVIDENCE_RECORDS,
            byte_limit: MAX_ROUTE_EVIDENCE_BYTES,
        }
    }
}
impl StateActionCapture {
    pub fn set_limits(&mut self, records: usize, bytes: usize) {
        self.record_limit = records;
        self.byte_limit = bytes;
        self.bytes = 0;
        let mut keep = 0;
        for record in &self.evidence.records {
            let Ok(size) = encoded_size(record, bytes.saturating_sub(self.bytes)) else {
                break;
            };
            if keep >= records {
                break;
            }
            self.bytes += size;
            keep += 1;
        }
        if keep < self.evidence.records.len() {
            self.evidence.omitted = true;
            self.evidence.records.truncate(keep);
        }
    }
    pub fn record(
        &mut self,
        record: &StateRecord,
        target: Option<&TargetRef>,
        source: Option<EvidenceSource>,
    ) {
        self.evidence.total_actions += 1;
        let strings = std::iter::once(record.state.as_str())
            .chain(record.before.iter().map(String::as_str))
            .chain(record.after.iter().map(String::as_str))
            .chain(record.event.as_deref())
            .chain(record.node.as_deref())
            .chain(record.note.as_deref());
        let too_long = strings.into_iter().any(|text| text.len() > 2048)
            || target.is_some_and(|target| target.kind.len() > 2048 || target.id.len() > 2048)
            || source
                .as_ref()
                .is_some_and(|source| encoded_size(source, 2048).is_err());
        #[derive(Serialize)]
        struct Borrowed<'a> {
            sequence: u64,
            kind: ChangeKind,
            state: &'a str,
            before: &'a [String],
            after: &'a [String],
            event: &'a Option<String>,
            node: &'a Option<String>,
            turn: u32,
            note: &'a Option<String>,
            target: Option<&'a TargetRef>,
            source: &'a Option<EvidenceSource>,
        }
        let borrowed = Borrowed {
            sequence: self.evidence.total_actions,
            kind: record.kind,
            state: &record.state,
            before: &record.before,
            after: &record.after,
            event: &record.event,
            node: &record.node,
            turn: record.turn,
            note: &record.note,
            target,
            source: &source,
        };
        if too_long || self.evidence.records.len() >= self.record_limit {
            self.evidence.omitted = true;
            return;
        }
        let Ok(size) = encoded_size(&borrowed, self.byte_limit.saturating_sub(self.bytes)) else {
            self.evidence.omitted = true;
            return;
        };
        self.bytes += size;
        self.evidence.records.push(StateActionRecord {
            sequence: self.evidence.total_actions,
            kind: record.kind,
            state: record.state.clone(),
            before: record.before.clone(),
            after: record.after.clone(),
            event: record.event.clone(),
            node: record.node.clone(),
            turn: record.turn,
            note: record.note.clone(),
            target: target.cloned(),
            source,
        });
    }
}
impl Story<'_> {
    pub fn state_action_evidence(&self) -> &StateActionEvidence {
        &self.state_actions.evidence
    }
    pub(crate) fn action_source(&self, kind: ChangeKind, line: u32) -> Option<EvidenceSource> {
        let node = self.current_node()?;
        if node.len() > 2048 {
            return None;
        }
        worldline_core::evidence_source::state_action_source(
            self.program,
            line,
            &EvidenceSourceOwner::StateAction {
                node,
                action: kind,
                timing: "during".into(),
                effect_index: None,
                action_index: None,
            },
        )
    }
}

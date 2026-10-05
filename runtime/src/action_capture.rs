//! 两类瞬态动作证据按真实发生顺序共享配额，不进入持久化状态。
use crate::{
    route_comparison::{encoded_size, MAX_ROUTE_EVIDENCE_BYTES, MAX_ROUTE_EVIDENCE_RECORDS},
    StateActionEvidence, VariableWriteEvidence,
};
use serde::Serialize;

pub(crate) struct ActionCapture {
    pub states: StateActionEvidence,
    pub variables: VariableWriteEvidence,
    bytes: usize,
    record_limit: usize,
    byte_limit: usize,
}
impl Default for ActionCapture {
    fn default() -> Self {
        Self {
            states: Default::default(),
            variables: Default::default(),
            bytes: 0,
            record_limit: MAX_ROUTE_EVIDENCE_RECORDS,
            byte_limit: MAX_ROUTE_EVIDENCE_BYTES,
        }
    }
}
impl ActionCapture {
    /// 只在比较侧首次建立后调用；起点 enter 效果先占额，正文变量尚未执行。
    pub fn enable_variable_writes(&mut self, records: usize, bytes: usize) {
        debug_assert!(!self.variables.captured);
        self.record_limit = records;
        self.byte_limit = bytes;
        self.bytes = 0;
        let mut keep = 0;
        for record in &self.states.records {
            let Ok(size) = encoded_size(record, bytes.saturating_sub(self.bytes)) else {
                break;
            };
            if keep >= records {
                break;
            }
            self.bytes += size;
            keep += 1;
        }
        if keep < self.states.records.len() {
            self.states.omitted = true;
            self.states.records.truncate(keep);
        }
        self.variables.captured = true;
    }

    /// 单条序列化字节在保留前已计量，只补有界外壳与数组逗号。
    pub fn encoded_size_bound(&self) -> usize {
        self.bytes
            .saturating_add(self.states.records.len())
            .saturating_add(self.variables.records.len())
            .saturating_add(256)
    }

    pub fn has_capacity(&self) -> bool {
        self.states.records.len() + self.variables.records.len() < self.record_limit
            && self.bytes < self.byte_limit
    }

    /// 输入必须借用真实值；只有预算确认后调用方才可复制保留记录。
    pub fn reserve(&mut self, record: &impl Serialize) -> bool {
        if self.states.records.len() + self.variables.records.len() >= self.record_limit {
            return false;
        }
        let Ok(size) = encoded_size(record, self.byte_limit.saturating_sub(self.bytes)) else {
            return false;
        };
        self.bytes += size;
        true
    }
}

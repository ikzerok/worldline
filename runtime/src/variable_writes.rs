//! 只记录独立路线验证中成功执行的全局赋值，不进入普通存档或重放。
use crate::{route_comparison::encoded_size, Story, Value};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use worldline_core::evidence_source::{
    variable_write_source, variable_write_source_file, EvidenceSource, EvidenceSourceOwner,
    VariableWriteOperation,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VariableWriteRecord {
    pub sequence: u64,
    pub operation: VariableWriteOperation,
    pub variable: String,
    pub before: Option<Value>,
    pub after: Value,
    pub event: Option<String>,
    pub node: Option<String>,
    pub turn: u32,
    pub source: Option<EvidenceSource>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct VariableWriteEvidence {
    pub captured: bool,
    pub records: Vec<VariableWriteRecord>,
    pub total_writes: u64,
    pub omitted: bool,
}

#[derive(Serialize)]
struct BorrowedWrite<'a> {
    sequence: u64,
    operation: VariableWriteOperation,
    variable: &'a str,
    before: Option<&'a Value>,
    after: &'a Value,
    event: Option<&'a str>,
    node: Option<&'a str>,
    turn: u32,
    source: &'a Option<EvidenceSource>,
}

fn value_too_long(value: &Value) -> bool {
    match value {
        Value::Str(text) | Value::Tag(text) | Value::StateRef(text) => text.len() > 2048,
        Value::TagSet(tags) => tags.iter().any(|tag| tag.len() > 2048),
        Value::Num(_) | Value::Bool(_) => false,
    }
}

impl Story<'_> {
    /// 普通 Story 未开启捕获时 captured=false；只读访问不触发历史重建。
    pub fn variable_write_evidence(&self) -> &VariableWriteEvidence {
        &self.action_capture.variables
    }

    pub(crate) fn write_variable(
        &mut self,
        variable: &str,
        value: Value,
        operation: VariableWriteOperation,
        line: u32,
    ) {
        // 保持原来的写入目标与顺序；旧值由 insert 移出，不为证据提前复制。
        let before = self.vars.insert(variable.to_owned(), value);
        if !self.action_capture.variables.captured {
            return;
        }
        self.action_capture.variables.total_writes += 1;
        let after = self.vars.get(variable).expect("变量刚成功写入");
        let event = self.frames.first().and_then(|frame| frame.node.as_deref());
        let frame = self
            .frames
            .iter()
            .rev()
            .find(|frame| frame.fragment.is_some() || frame.node.is_some());
        let node_too_long = frame.is_some_and(|frame| {
            frame
                .fragment
                .as_ref()
                .is_some_and(|name| name.len() > 2039)
                || frame.node.as_ref().is_some_and(|name| name.len() > 2048)
        });
        if !self.action_capture.has_capacity()
            || variable.len() > 2048
            || event.is_some_and(|name| name.len() > 2048)
            || node_too_long
            || before.as_ref().is_some_and(value_too_long)
            || value_too_long(after)
        {
            self.action_capture.variables.omitted = true;
            return;
        }
        let node = frame.and_then(|frame| {
            frame
                .fragment
                .as_ref()
                .map(|name| Cow::Owned(format!("fragment:{name}")))
                .or_else(|| frame.node.as_deref().map(Cow::Borrowed))
        });
        let owner = node
            .as_deref()
            .map(|node| EvidenceSourceOwner::VariableWrite {
                node: node.into(),
                variable: variable.into(),
                operation,
            });
        if owner.as_ref().is_some_and(|owner| {
            variable_write_source_file(self.program, line, owner)
                .is_some_and(|file| file.len() > 2048)
        }) {
            self.action_capture.variables.omitted = true;
            return;
        }
        let source = owner
            .as_ref()
            .and_then(|owner| variable_write_source(self.program, line, owner));
        if source
            .as_ref()
            .is_some_and(|source| encoded_size(source, 2048).is_err())
        {
            self.action_capture.variables.omitted = true;
            return;
        }
        let borrowed = BorrowedWrite {
            sequence: self.action_capture.variables.total_writes,
            operation,
            variable,
            before: before.as_ref(),
            after,
            event,
            node: node.as_deref(),
            turn: self.turns,
            source: &source,
        };
        if !self.action_capture.reserve(&borrowed) {
            self.action_capture.variables.omitted = true;
            return;
        }
        self.action_capture
            .variables
            .records
            .push(VariableWriteRecord {
                sequence: borrowed.sequence,
                operation,
                variable: variable.into(),
                before,
                after: after.clone(),
                event: event.map(str::to_owned),
                node: node.map(Cow::into_owned),
                turn: self.turns,
                source,
            });
    }
}

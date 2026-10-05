//! 变量写入单独保留名字、操作和场景路径，不改变原语句来源的歧义规则。
use super::{SourceOwner, SourceProvenance};
use crate::ast::{Loc, Stmt};
use crate::evidence_source::VariableWriteOperation;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VariableWriteOriginKey {
    owner: SourceOwner,
    scenes: Vec<String>,
    line: u32,
    column: u32,
    variable: String,
    operation: VariableWriteOperation,
}

impl VariableWriteOperation {
    pub(crate) fn statement(statement: &Stmt) -> Option<(Loc, &str, Self)> {
        match statement {
            Stmt::Let(statement) => Some((
                statement.loc,
                &statement.name,
                if statement.is_const {
                    Self::Const
                } else {
                    Self::Let
                },
            )),
            Stmt::Set(statement) => Some((statement.loc, &statement.name, Self::Set)),
            _ => None,
        }
    }
}

impl SourceProvenance {
    pub(crate) fn record_variable_write(
        &mut self,
        owner: &SourceOwner,
        scenes: &[String],
        statement: &Stmt,
        file: &str,
    ) {
        let Some((loc, variable, operation)) = VariableWriteOperation::statement(statement) else {
            return;
        };
        let key = VariableWriteOriginKey {
            owner: owner.clone(),
            scenes: scenes.to_vec(),
            line: loc.line,
            column: loc.column,
            variable: variable.into(),
            operation,
        };
        self.variable_write_origins
            .entry(key)
            .and_modify(|known| {
                if known.as_deref() != Some(file) {
                    *known = None;
                }
            })
            .or_insert_with(|| Some(file.into()));
    }

    /// 按借用身份查找，不能为了拒绝巨大来源字段而先复制其路径。
    pub(crate) fn variable_write_file(
        &self,
        owner: (&str, u32),
        scenes: &[String],
        loc: Loc,
        variable: &str,
        operation: VariableWriteOperation,
    ) -> Option<&str> {
        self.variable_write_origins
            .iter()
            .find(|(key, _)| {
                key.owner.file == owner.0
                    && key.owner.line == owner.1
                    && key.scenes == scenes
                    && key.line == loc.line
                    && key.column == loc.column
                    && key.variable == variable
                    && key.operation == operation
            })?
            .1
            .as_deref()
    }
}

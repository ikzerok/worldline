//! 语句的非语义文件归属，按正式 parser 的根 owner、Loc 和语句种类绑定。
use super::SourceProvenance;
use crate::ast::{Loc, Stmt};
use crate::diagnostic::{Diagnostic, DiagnosticSourceRole};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SourceOwner {
    pub file: String,
    pub line: u32,
}
impl SourceOwner {
    pub fn new(file: &str, line: u32) -> Self {
        Self {
            file: file.into(),
            line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StatementKind {
    Local,
    Call,
    Return,
    Say,
    DynamicChange,
    Text,
    Divert,
    Choice,
    If,
    Let,
    Set,
    Scene,
    Change,
    Anchor,
    Effect,
}
impl StatementKind {
    pub fn of(statement: &Stmt) -> Self {
        match statement {
            Stmt::Local(_) => Self::Local,
            Stmt::Call(_) => Self::Call,
            Stmt::Return(_) => Self::Return,
            Stmt::Say(_) => Self::Say,
            Stmt::DynamicChange(_) => Self::DynamicChange,
            Stmt::Text(_) => Self::Text,
            Stmt::Divert(_) => Self::Divert,
            Stmt::Choice(_) => Self::Choice,
            Stmt::If(_) => Self::If,
            Stmt::Let(_) => Self::Let,
            Stmt::Set(_) => Self::Set,
            Stmt::Scene(_) => Self::Scene,
            Stmt::Change(_) => Self::Change,
            Stmt::Anchor(_) => Self::Anchor,
            Stmt::Effect(_) => Self::Effect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StatementOriginKey(SourceOwner, u32, u32, StatementKind);

impl SourceProvenance {
    pub(crate) fn record_statement(
        &mut self,
        owner: &SourceOwner,
        loc: Loc,
        kind: StatementKind,
        file: &str,
    ) {
        self.statement_origins
            .entry(StatementOriginKey(
                owner.clone(),
                loc.line,
                loc.column,
                kind,
            ))
            .and_modify(|known| {
                if known.as_deref() != Some(file) {
                    *known = None;
                }
            })
            .or_insert_with(|| Some(file.into()));
    }

    pub(crate) fn statement_file(
        &self,
        owner: &SourceOwner,
        loc: Loc,
        kind: StatementKind,
    ) -> Option<&str> {
        self.statement_origins
            .get(&StatementOriginKey(
                owner.clone(),
                loc.line,
                loc.column,
                kind,
            ))?
            .as_deref()
    }
}

/// 只处理此生产作用域新建、尚未被嵌套作用域确认的诊断；不更改诊断事实。
pub(crate) fn bind_diagnostics(diagnostics: &mut [Diagnostic], file: Option<&str>) {
    for diagnostic in diagnostics {
        if diagnostic.source_bound {
            continue;
        }
        if let Some(file) = file {
            diagnostic.file = file.into();
        } else {
            diagnostic.source_role = Some(DiagnosticSourceRole::Unavailable);
        }
        diagnostic.source_bound = true;
    }
}

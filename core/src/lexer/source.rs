//! 行片段与物理原稿的显式边界；LineKind 的旧局部坐标仍供重构消费者使用。
use super::{Line, LineKind};
use crate::diagnostic::{DiagnosticSourceRole, Span};
use crate::source_provenance::StatementSource;

#[derive(Debug, Clone, Default)]
pub(crate) struct LineSource {
    pub base: u32,
    pub length: u32,
    pub value: u32,
    pub condition: u32,
    pub enable: u32,
    pub remainder: u32,
    pub text: u32,
    pub label: u32,
    pub label_boundaries: Vec<usize>,
}

impl Line {
    pub(crate) fn statement_source(&self) -> StatementSource {
        let role = match self.kind {
            LineKind::Let { .. } | LineKind::Const { .. } | LineKind::Event { .. }
            | LineKind::Storyline { .. } | LineKind::Character { .. } | LineKind::Entity { .. }
            | LineKind::RelationType { .. } | LineKind::RelationDef { .. } | LineKind::World { .. }
            | LineKind::Period { .. } | LineKind::Catalog(_) | LineKind::Schema112 { .. }
            | LineKind::Include { .. } => DiagnosticSourceRole::Declaration,
            LineKind::Language111 { ref keyword, .. } if matches!(keyword.as_str(), "rule" | "fragment") => DiagnosticSourceRole::Declaration,
            _ => DiagnosticSourceRole::Statement,
        };
        StatementSource { span: Span::new(self.no, self.source.base + 1, self.source.length), role }
    }

    /// 仅 Parser 调用：在物理输入边界转换一次，不改变片段/重构的 LineKind 合同。
    pub(crate) fn physical(&self) -> Self {
        let mut line = self.clone();
        let base = line.source.base;
        match &mut line.kind {
            LineKind::Schema112 { loc, .. } | LineKind::Language111 { loc, .. }
            | LineKind::RelationField { loc, .. } | LineKind::Property { loc, .. }
            | LineKind::Description { loc, .. } | LineKind::Relation { loc, .. }
            | LineKind::Effect { loc, .. } | LineKind::ChangeLine { loc, .. }
            | LineKind::ToLine { loc, .. } | LineKind::Anchor { loc, .. }
            | LineKind::If { loc, .. } | LineKind::ElseIf { loc, .. }
            | LineKind::Else { loc } | LineKind::Text { loc, .. } => loc.column += base,
            LineKind::Let { loc, name_span, .. } | LineKind::Const { loc, name_span, .. }
            | LineKind::Set { loc, name_span, .. } => { loc.column += base; name_span.column += base; }
            LineKind::Event { loc, .. } | LineKind::Storyline { loc, .. }
            | LineKind::Character { loc, .. } | LineKind::Entity { loc, .. }
            | LineKind::RelationType { loc, .. } | LineKind::RelationDef { loc, .. }
            | LineKind::World { loc, .. } | LineKind::Period { loc, .. }
            | LineKind::Scene { loc, .. } => loc.column += base,
            LineKind::Include { span, .. } | LineKind::Divert { span, .. } => span.column += base,
            LineKind::Choice { loc, label_span, disabled_span, .. } => {
                loc.column += base;
                label_span.column += base;
                if let Some(span) = disabled_span { span.column += base; }
            }
            LineKind::Become(change) => change.loc.column += base,
            LineKind::Catalog(declaration) => match declaration {
                crate::catalog::CatalogDecl::Tag(value) | crate::catalog::CatalogDecl::Anchor(value) => value.loc.column += base,
                crate::catalog::CatalogDecl::Asset(value) => value.loc.column += base,
                _ => {}
            },
        }
        line
    }
}

/// 与 decode_escapes 相同的已有解码步骤，仅记录每个解码 scalar 的原稿边界。
pub(super) fn decoded_boundaries(raw: &[char]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        positions.push(at);
        if raw[at] == '\\' && at + 1 < raw.len()
            && matches!(raw[at + 1], 'n' | 't' | '"' | '\\' | '{' | '}' | '~' | '#') {
            at += 2;
        } else { at += 1; }
    }
    positions.push(raw.len());
    positions
}

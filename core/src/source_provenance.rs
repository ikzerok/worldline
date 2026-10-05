//! 正式解析的非语义来源侧表；不进入作品指纹、运行签名或持久身份。
use crate::ast::Expr;
use crate::diagnostic::{Diagnostic, DiagnosticSourceRole, Span};
use std::collections::BTreeMap;
mod origin;
pub(crate) use origin::{bind_diagnostics, SourceOwner, StatementKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ExpressionSlot {
    Value,
    Condition(u32),
    Enable,
    Text(u32),
    After,
    Rule,
    State,
    Tags,
    Call,
}

/// children 顺序与正式 Expr 的 Unary/Binary/Call 子项完全相同。
#[derive(Debug, Clone, Default)]
pub(crate) struct ExpressionSource {
    pub span: Option<Span>,
    pub file: String,
    pub children: Vec<ExpressionSource>,
}

impl ExpressionSource {
    pub fn leaf(span: Span, file: &str) -> Self {
        Self {
            span: Some(span),
            file: file.into(),
            children: Vec::new(),
        }
    }

    pub fn map_boundaries(&mut self, base: u32, positions: &[usize]) {
        if let Some(span) = &mut self.span {
            map_span(span, base, positions);
        }
        for child in &mut self.children {
            child.map_boundaries(base, positions);
        }
    }

    pub fn bind(&self, expression: &Expr, output: &mut BTreeMap<usize, (String, Span)>) {
        if let Some(span) = self.span {
            output.insert(
                expression as *const Expr as usize,
                (self.file.clone(), span),
            );
        }
        match expression {
            Expr::Unary { expr, .. } => {
                if let [source] = self.children.as_slice() {
                    source.bind(expr, output);
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                if let [left, right] = self.children.as_slice() {
                    left.bind(lhs, output);
                    right.bind(rhs, output);
                }
            }
            Expr::Call { args, .. } if args.len() == self.children.len() => {
                for (arg, source) in args.iter().zip(&self.children) {
                    source.bind(arg, output);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn map_span(span: &mut Span, base: u32, positions: &[usize]) {
    let Some(start) = span.column.checked_sub(base + 1) else {
        return;
    };
    let Some(end) = start.checked_add(span.length) else {
        return;
    };
    if let (Some(&start), Some(&end)) = (positions.get(start as usize), positions.get(end as usize))
    {
        span.column = base + start as u32 + 1;
        span.length = end.saturating_sub(start) as u32;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StatementSource {
    pub span: Span,
    pub role: DiagnosticSourceRole,
}

#[derive(Debug, Clone, Default)]
pub struct SourceProvenance {
    /// 最后消费的物理语句行；只用于声明结构范围，不参与语义。
    pub(crate) block_ends: BTreeMap<(String, u32), u32>,
    pub(crate) statement_origins: BTreeMap<origin::StatementOriginKey, Option<String>>,
    pub(crate) statements: BTreeMap<(String, u32), StatementSource>,
    /// 正式 parser 消费的 if/else if/else 头，键为首 if 文件/行及分支序号。
    pub(crate) branch_headers: BTreeMap<(String, u32, usize), (String, Span)>,
    pub(crate) expressions: BTreeMap<(String, u32, ExpressionSlot), ExpressionSource>,
}

impl SourceProvenance {
    pub(crate) fn insert_expression(
        &mut self,
        file: &str,
        owner: u32,
        slot: ExpressionSlot,
        source: ExpressionSource,
    ) {
        self.expressions.insert((file.into(), owner, slot), source);
    }

    pub(crate) fn expression(
        &self,
        file: &str,
        owner: u32,
        slot: ExpressionSlot,
    ) -> Option<&ExpressionSource> {
        self.expressions.get(&(file.into(), owner, slot))
    }

    /// 未专门承诺 target/expression 的语言生产者使用正式语句/声明上下文。
    pub(crate) fn resolve_diagnostics(&self, diagnostics: &mut [Diagnostic]) {
        for diagnostic in diagnostics {
            self.resolve(
                &diagnostic.file,
                &mut diagnostic.span,
                &mut diagnostic.source_role,
            );
            diagnostic
                .related_source_roles
                .resize(diagnostic.related.len(), None);
            for ((file, span), role) in diagnostic
                .related
                .iter_mut()
                .zip(&mut diagnostic.related_source_roles)
            {
                self.resolve(file, span, role);
            }
        }
    }

    fn resolve(&self, file: &str, span: &mut Span, role: &mut Option<DiagnosticSourceRole>) {
        if *role == Some(DiagnosticSourceRole::Unavailable) {
            *span = Span::new(0, 1, 0);
            return;
        }
        if role.is_some() {
            return;
        }
        if let Some(source) = self.statements.get(&(file.into(), span.line)) {
            *span = source.span;
            *role = Some(source.role);
        } else {
            *role = Some(DiagnosticSourceRole::Document);
        }
    }
}

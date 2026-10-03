//! 分析克隆 AST 时按 parser 的 owner/slot 绑定，不按语义值或名称猜位置。
use super::Ctx;
use crate::ast::{Expr, Loc, TextPart, ValueKind};
use crate::diagnostic::DiagnosticSourceRole;
use crate::source_provenance::{bind_diagnostics, ExpressionSlot, StatementKind};

pub(super) struct DiagnosticScope {
    previous_file: Option<String>,
    file: Option<String>,
    start: usize,
}

impl Ctx<'_> {
    pub(super) fn enter_source(&mut self, loc: Loc, kind: StatementKind) -> DiagnosticScope {
        let file = self
            .source_owner
            .as_ref()
            .and_then(|owner| {
                self.program
                    .source_provenance
                    .statement_file(owner, loc, kind)
            })
            .map(str::to_owned);
        let previous_file = std::mem::replace(&mut self.source_file, file.clone());
        DiagnosticScope {
            previous_file,
            file,
            start: self.diags.len(),
        }
    }

    pub(super) fn leave_source(&mut self, scope: DiagnosticScope) {
        bind_diagnostics(&mut self.diags[scope.start..], scope.file.as_deref());
        self.source_file = scope.previous_file;
    }

    pub(super) fn check_at(
        &mut self,
        expression: &Expr,
        expected: Option<ValueKind>,
        owner: u32,
        slot: ExpressionSlot,
    ) -> Option<ValueKind> {
        self.expression_sources.clear();
        if let Some(source) = self
            .source_file
            .as_deref()
            .and_then(|file| self.program.source_provenance.expression(file, owner, slot))
        {
            source.bind(expression, &mut self.expression_sources);
        }
        self.check_expr(expression, expected)
    }

    pub(super) fn bind_arguments(&mut self, args: &[Expr], owner: u32) {
        self.expression_sources.clear();
        if let Some(source) = self.source_file.as_deref().and_then(|file| {
            self.program
                .source_provenance
                .expression(file, owner, ExpressionSlot::Call)
        }) {
            if source.children.len() == args.len() {
                for (arg, source) in args.iter().zip(&source.children) {
                    source.bind(arg, &mut self.expression_sources);
                }
            }
        }
    }

    pub(super) fn check_text(&mut self, parts: &[TextPart], loc: Loc) {
        let mut ordinal = 0;
        for part in parts {
            if let TextPart::Expr(expression) = part {
                self.check_at(expression, None, loc.line, ExpressionSlot::Text(ordinal));
                ordinal += 1;
            }
        }
    }

    pub(super) fn mark_expression_diagnostics(&mut self, expression: &Expr, start: usize) {
        let Some((file, span)) = self
            .expression_sources
            .get(&(expression as *const Expr as usize))
            .cloned()
        else {
            return;
        };
        for diagnostic in &mut self.diags[start..] {
            if !diagnostic.source_bound {
                diagnostic.file = file.clone();
                diagnostic.source_bound = true;
                if diagnostic.source_role.is_none() {
                    diagnostic.span = span;
                    diagnostic.source_role = Some(DiagnosticSourceRole::Expression);
                }
            }
        }
    }
}

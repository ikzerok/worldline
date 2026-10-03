//! 全局声明收集与初始化类型；块内声明可见不意味着启动时执行。
use super::{Ctx, VarInfo};
use crate::analysis_helpers::expr_kind_static;
use crate::ast::{LetStmt, Stmt};
use crate::diagnostic::{Diagnostic, Span};
use crate::source_provenance::ExpressionSlot;

fn declarations<'a>(body: &'a [Stmt], out: &mut Vec<&'a LetStmt>) {
    for stmt in body {
        match stmt {
            Stmt::Let(value) => out.push(value),
            Stmt::Choice(value) => declarations(&value.body, out),
            Stmt::If(value) => {
                for (_, branch) in &value.branches {
                    declarations(branch, out);
                }
            }
            Stmt::Scene(value) => declarations(&value.body, out),
            _ => {}
        }
    }
}

impl Ctx<'_> {
    pub(super) fn collect_vars(&mut self) {
        let mut values: Vec<_> = self.program.lets.iter().collect();
        for event in &self.program.events {
            declarations(&event.body, &mut values);
        }
        let mut fragment_scopes = std::collections::HashMap::new();
        for fragment in &self.program.fragments {
            let mut body_values = Vec::new();
            declarations(&fragment.body, &mut body_values);
            for value in &body_values {
                fragment_scopes.insert(
                    (value.file.clone(), value.loc.line, value.loc.column),
                    fragment,
                );
            }
            values.extend(body_values);
        }
        for value in &values {
            if value.name.is_empty() {
                continue;
            }
            let span = Span::new(
                value.loc.line,
                value.loc.column,
                value.name.chars().count() as u32,
            );
            if let Some(previous) = self.symbols.vars.get(&value.name) {
                self.diags.push(
                    Diagnostic::error(
                        "A104",
                        &value.file,
                        span,
                        format!("变量 `{}` 重复定义", value.name),
                    )
                    .with_source_role(crate::diagnostic::DiagnosticSourceRole::Target)
                    .with_related_source_role(
                        &previous.decl_file,
                        previous.decl_span,
                        crate::diagnostic::DiagnosticSourceRole::Target,
                    ),
                );
                continue;
            }
            self.symbols.vars.insert(
                value.name.clone(),
                VarInfo {
                    kind: None,
                    is_const: value.is_const,
                    decl_file: value.file.clone(),
                    decl_span: span,
                    read: false,
                },
            );
        }
        // 类型依赖可向后引用，迭代补全；不求值，也不提升初始化。
        for _ in 0..values.len() {
            let mut changed = false;
            for value in &values {
                let scope =
                    fragment_scopes.get(&(value.file.clone(), value.loc.line, value.loc.column));
                let mut scoped_symbols = self.symbols.clone();
                if let Some(fragment) = scope {
                    let bindings = fragment
                        .parameters
                        .iter()
                        .map(|p| (&p.name, p.kind, p.loc))
                        .chain(
                            crate::language::locals(&fragment.body)
                                .into_iter()
                                .map(|l| (&l.name, l.kind, l.loc)),
                        );
                    for (name, kind, loc) in bindings {
                        scoped_symbols.vars.insert(
                            name.clone(),
                            VarInfo {
                                kind: Some(kind),
                                is_const: true,
                                decl_file: fragment.file.clone(),
                                decl_span: Span::new(
                                    loc.line,
                                    loc.column,
                                    name.chars().count() as u32,
                                ),
                                read: false,
                            },
                        );
                    }
                }
                let kind = expr_kind_static(&value.expr, &scoped_symbols);
                if let Some(info) = self.symbols.vars.get_mut(&value.name) {
                    if info.kind.is_none() && kind.is_some() {
                        info.kind = kind;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for value in &self.program.lets.clone() {
            self.cur_file = value.file.clone();
            self.source_file = Some(value.file.clone());
            self.expression_fallback = value.loc;
            let kind = self.check_at(&value.expr, None, value.loc.line, ExpressionSlot::Value);
            if let Some(info) = self.symbols.vars.get_mut(&value.name) {
                if info.kind.is_none() {
                    info.kind = kind;
                }
            }
        }
    }
}

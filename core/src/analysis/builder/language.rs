use super::{Ctx, NodeCtx};
use crate::ast::*;
use crate::diagnostic::{Diagnostic, Span};
use crate::language::{is_new_builtin, locals, static_id, Parameter};
use crate::source_provenance::{ExpressionSlot, SourceOwner, StatementKind};
use std::collections::{HashMap, HashSet};

impl Ctx<'_> {
    pub(super) fn language_error(
        &mut self,
        code: &'static str,
        loc: Loc,
        message: impl Into<String>,
    ) {
        self.diags.push(Diagnostic::error(
            code,
            &self.cur_file,
            Span::new(loc.line, loc.column, 4),
            message,
        ));
    }
    pub(super) fn collect_callable_symbols(&mut self) {
        let mut seen: HashMap<String, (String, Loc)> = HashMap::new();
        let declarations: Vec<_> = self
            .program
            .rules
            .iter()
            .map(|r| (&r.name, &r.file, r.loc, Some(r.result)))
            .chain(
                self.program
                    .fragments
                    .iter()
                    .map(|f| (&f.name, &f.file, f.loc, None)),
            )
            .collect();
        for (name, file, loc, result) in declarations {
            self.cur_file = file.clone();
            if let Some((previous, at)) = seen.insert(name.clone(), (file.clone(), loc)) {
                self.diags.push(
                    Diagnostic::error(
                        "A104",
                        file,
                        Span::new(loc.line, loc.column, 4),
                        format!("规则/片段 `{name}` 重复定义"),
                    )
                    .with_related(&previous, Span::new(at.line, at.column, 4)),
                );
            }
            if is_new_builtin(name)
                || matches!(
                    name.as_str(),
                    "rnd" | "has" | "seen" | "visits" | "turns" | "perm"
                )
            {
                self.language_error("A104", loc, "规则/片段名不能覆盖内建函数");
            }
            if let Some(result) = result {
                self.symbols.rule_results.insert(name.clone(), result);
            }
        }
    }
    fn bind_parameters(&mut self, ps: &[Parameter]) {
        self.locals.clear();
        for p in ps {
            if self.locals.insert(p.name.clone(), p.kind).is_some() {
                self.language_error("A104", p.loc, format!("参数 `{}` 重复", p.name));
            }
        }
    }
    pub(super) fn walk_callables(&mut self) {
        for r in self.program.rules.clone() {
            self.cur_file = r.file;
            self.source_file = Some(self.cur_file.clone());
            self.expression_fallback = r.loc;
            self.bind_parameters(&r.parameters);
            self.in_rule = true;
            self.check_at(&r.expr, Some(r.result), r.loc.line, ExpressionSlot::Rule);
            self.in_rule = false;
        }
        for f in self.program.fragments.clone() {
            self.cur_file = f.file;
            self.source_file = Some(self.cur_file.clone());
            self.source_owner = Some(SourceOwner::new(&self.cur_file, f.loc.line));
            self.expression_fallback = f.loc;
            self.bind_parameters(&f.parameters);
            self.in_fragment = true;
            for l in locals(&f.body) {
                if self.locals.insert(l.name.clone(), l.kind).is_some() {
                    let scope = self.enter_source(l.loc, StatementKind::Local);
                    self.language_error("A104", l.loc, format!("局部/参数 `{}` 重复", l.name));
                    self.leave_source(scope);
                }
            }
            self.walk_block(
                &f.body,
                &NodeCtx {
                    event: usize::MAX,
                    node_name: format!("fragment:{}", f.name),
                },
                0,
            );
            self.in_fragment = false;
        }
        self.locals.clear();
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        for r in &self.program.rules {
            let mut deps = Vec::new();
            expression_calls(&r.expr, &mut deps);
            graph.insert(r.name.clone(), deps);
        }
        for f in &self.program.fragments {
            let mut deps = Vec::new();
            fragment_calls(&f.body, &mut deps);
            graph.insert(f.name.clone(), deps);
        }
        for (name, file, loc) in self
            .program
            .rules
            .iter()
            .map(|r| (&r.name, &r.file, r.loc))
            .chain(
                self.program
                    .fragments
                    .iter()
                    .map(|f| (&f.name, &f.file, f.loc)),
            )
        {
            let mut visited = HashSet::new();
            let mut pending = graph.get(name).cloned().unwrap_or_default();
            let mut cycle = false;
            while let Some(next) = pending.pop() {
                if &next == name {
                    cycle = true;
                    break;
                }
                if visited.insert(next.clone()) {
                    pending.extend(graph.get(&next).into_iter().flatten().cloned());
                }
            }
            if cycle {
                self.diags.push(Diagnostic::error(
                    "A230",
                    file,
                    Span::new(loc.line, loc.column, 4),
                    format!("规则/片段 `{name}` 存在直接或间接递归"),
                ));
            }
        }
    }
    fn check_arguments(&mut self, name: &str, args: &[Expr], parameters: &[Parameter], loc: Loc) {
        if args.len() != parameters.len() {
            self.language_error(
                "A103",
                loc,
                format!(
                    "`{name}` 需要{}个参数，实际{}个",
                    parameters.len(),
                    args.len()
                ),
            );
        }
        for (i, a) in args.iter().enumerate() {
            self.check_expr(a, parameters.get(i).map(|p| p.kind));
        }
    }
    pub(super) fn check_language_call(
        &mut self,
        name: &str,
        args: &[Expr],
        loc: Loc,
    ) -> Option<ValueKind> {
        if !self.program.language_version.supports_language_111() {
            self.language_error("A103", loc, "此表达式需要显式语言1.11");
            return None;
        }
        if let Some(rule) = self.program.rules.iter().find(|r| r.name == name).cloned() {
            self.check_arguments(name, args, &rule.parameters, loc);
            return Some(rule.result);
        }
        let (arity, result) = match name {
            "tag" => (1, ValueKind::Tag),
            "state" => (1, ValueKind::StateRef),
            "tags" => (args.len(), ValueKind::TagSet),
            "members" => (1, ValueKind::TagSet),
            "count" => (1, ValueKind::Num),
            "contains" => (2, ValueKind::Bool),
            "union" | "intersect" | "difference" => (2, ValueKind::TagSet),
            "when" => (3, ValueKind::Bool),
            _ => return None,
        };
        if args.len() != arity {
            self.language_error("A103", loc, format!("{name} 需要{arity}个参数"));
        }
        if matches!(name, "tag" | "state") {
            let id = args.first().and_then(static_id);
            let exists = id.is_some_and(|id| {
                self.program.catalog.iter().any(|d| match d {
                    crate::catalog::CatalogDecl::Tag(t) => name == "tag" && t.name == id,
                    crate::catalog::CatalogDecl::State(s) => name == "state" && s.id == id,
                    _ => false,
                })
            });
            if !exists {
                self.language_error("A216", loc, format!("{name} 必须引用已声明的静态身份"));
            }
        } else if name == "when" {
            if let Some(c) = args.first() {
                self.check_expr(c, Some(ValueKind::Bool));
            }
            let yes = args.get(1).and_then(|a| self.check_expr(a, None));
            if let Some(no) = args.get(2) {
                self.check_expr(no, yes);
            }
            return yes;
        } else {
            for (i, a) in args.iter().enumerate() {
                let kind = match name {
                    "tags" => ValueKind::Tag,
                    "members" => ValueKind::StateRef,
                    "contains" if i == 1 => ValueKind::Tag,
                    _ => ValueKind::TagSet,
                };
                self.check_expr(a, Some(kind));
            }
        }
        Some(result)
    }
    pub(super) fn check_language_statement(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Local(l) => {
                if !self.in_fragment {
                    self.language_error("A230", l.loc, "local只能出现在片段内");
                }
                self.check_at(&l.expr, Some(l.kind), l.loc.line, ExpressionSlot::Value);
            }
            Stmt::Return(loc) => {
                if !self.in_fragment {
                    self.language_error("A230", *loc, "return只能出现在片段内");
                }
            }
            Stmt::Call(c) => {
                self.bind_arguments(&c.args, c.loc.line);
                match self
                    .program
                    .fragments
                    .iter()
                    .find(|f| f.name == c.name)
                    .cloned()
                {
                    Some(f) => self.check_arguments(&c.name, &c.args, &f.parameters, c.loc),
                    None => self.language_error("A103", c.loc, format!("未知片段 `{}`", c.name)),
                }
            }
            Stmt::Say(s) => {
                if !self.symbols.characters.contains_key(&s.speaker) {
                    self.language_error(
                        "A208",
                        s.loc,
                        format!("say引用未定义角色 `{}`", s.speaker),
                    );
                }
                self.check_text(&s.text.parts, s.loc);
            }
            Stmt::DynamicChange(c) => {
                self.check_at(
                    &c.state,
                    Some(ValueKind::StateRef),
                    c.loc.line,
                    ExpressionSlot::State,
                );
                self.check_at(
                    &c.tags,
                    Some(ValueKind::TagSet),
                    c.loc.line,
                    ExpressionSlot::Tags,
                );
            }
            _ => {}
        }
    }
}
fn expression_calls(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Call { name, args, .. } => {
            out.push(name.clone());
            for a in args {
                expression_calls(a, out);
            }
        }
        Expr::Unary { expr, .. } => expression_calls(expr, out),
        Expr::Binary { lhs, rhs, .. } => {
            expression_calls(lhs, out);
            expression_calls(rhs, out);
        }
        _ => {}
    }
}
fn fragment_calls(body: &[Stmt], out: &mut Vec<String>) {
    for s in body {
        match s {
            Stmt::Call(c) => out.push(c.name.clone()),
            Stmt::Choice(c) => fragment_calls(&c.body, out),
            Stmt::If(i) => {
                for (_, b) in &i.branches {
                    fragment_calls(b, out);
                }
            }
            Stmt::Scene(s) => fragment_calls(&s.body, out),
            _ => {}
        }
    }
}

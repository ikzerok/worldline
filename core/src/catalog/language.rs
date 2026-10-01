//! 新语言身份与强引用只从 AST 投影；参数和局部不会伪装全局变量。
use super::{Catalog, ReferenceInfo, TargetRef};
use crate::ast::{ChangeKind, DivertTarget, Expr, Program, Stmt, TextPart};
use std::collections::BTreeSet;

pub(super) fn collect(program: &Program, symbols: &crate::Symbols, catalog: &mut Catalog) {
    if !program.language_version.supports_language_111() {
        return;
    }
    for rule in &program.rules {
        catalog.add_object("rule", &rule.name, &rule.name, &rule.file, rule.loc.line);
    }
    for fragment in &program.fragments {
        catalog.add_object(
            "fragment",
            &fragment.name,
            &fragment.name,
            &fragment.file,
            fragment.loc.line,
        );
    }
    let rules: BTreeSet<_> = program.rules.iter().map(|r| r.name.as_str()).collect();
    for rule in &program.rules {
        let locals = rule.parameters.iter().map(|p| p.name.as_str()).collect();
        let mut collector = Collector {
            catalog,
            symbols,
            rules: &rules,
            owner: TargetRef::new("rule", &rule.name),
            file: &rule.file,
            locals,
        };
        collector.expr(&rule.expr);
    }
    for fragment in &program.fragments {
        let mut locals: BTreeSet<_> = fragment
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        locals.extend(
            crate::language::locals(&fragment.body)
                .iter()
                .map(|p| p.name.as_str()),
        );
        let mut collector = Collector {
            catalog,
            symbols,
            rules: &rules,
            owner: TargetRef::new("fragment", &fragment.name),
            file: &fragment.file,
            locals,
        };
        collector.body(&fragment.body);
    }
    for (index, event) in program.events.iter().enumerate() {
        let Some(file) = program.event_files.get(index) else {
            continue;
        };
        let mut collector = Collector {
            catalog,
            symbols,
            rules: &rules,
            owner: TargetRef::new("event", &event.name),
            file,
            locals: BTreeSet::new(),
        };
        collector.body(&event.body);
        if let Some(expr) = &event.after {
            collector.expr(expr);
        }
        for effect in &event.effects {
            if let Some(expr) = &effect.cond {
                collector.expr(expr);
            }
        }
    }
    for decl in &program.lets {
        let mut collector = Collector {
            catalog,
            symbols,
            rules: &rules,
            owner: TargetRef::new("variable", &decl.name),
            file: &decl.file,
            locals: BTreeSet::new(),
        };
        collector.expr(&decl.expr);
    }
}
struct Collector<'a, 'b> {
    catalog: &'a mut Catalog,
    symbols: &'a crate::Symbols,
    rules: &'a BTreeSet<&'b str>,
    owner: TargetRef,
    file: &'b str,
    locals: BTreeSet<&'b str>,
}
impl Collector<'_, '_> {
    fn reference(&mut self, kind: &str, id: &str, line: u32) {
        self.catalog.references.push(ReferenceInfo {
            source: self.owner.clone(),
            target: TargetRef::new(kind, id),
            kind: "语言强引用".into(),
            file: self.file.into(),
            line,
        });
    }
    fn node(&mut self, name: &str, line: u32) {
        if let Some(path) = self.symbols.resolve_node(name) {
            let kind = if path.scenes.is_empty() {
                "event"
            } else {
                "scene"
            };
            self.reference(kind, name, line);
        }
    }
    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call { name, args, loc } => {
                if matches!(name.as_str(), "tag" | "state") {
                    if let Some(id) = args.first().and_then(crate::language::static_id) {
                        self.reference(name, id, loc.line);
                    }
                    return;
                }
                if self.rules.contains(name.as_str()) {
                    self.reference("rule", name, loc.line);
                }
                if name == "has" {
                    if self.owner.kind == "fragment" || self.owner.kind == "rule" {
                        for (arg, kind) in args.iter().zip(["state", "tag"]) {
                            if let Some(id) = crate::language::static_id(arg) {
                                self.reference(kind, id, loc.line);
                            }
                        }
                    }
                    return;
                }
                if matches!(name.as_str(), "seen" | "visits") {
                    if let Some(id) = args.first().and_then(crate::language::static_id) {
                        self.node(id, loc.line);
                    }
                    return;
                }
                for arg in args {
                    self.expr(arg);
                }
            }
            Expr::Var { name, loc } if !self.locals.contains(name.as_str()) => {
                if matches!(self.owner.kind.as_str(), "rule" | "fragment") {
                    self.reference("variable", name, loc.line);
                }
            }
            Expr::Unary { expr, .. } => self.expr(expr),
            Expr::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            _ => {}
        }
    }
    fn parts(&mut self, parts: &[TextPart], _line: u32) {
        for part in parts {
            if let TextPart::Expr(expr) = part {
                self.expr(expr);
            }
        }
    }
    fn body(&mut self, body: &[Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Text(text) => self.parts(&text.parts, text.loc.line),
                Stmt::Say(say) => {
                    self.reference("character", &say.speaker, say.loc.line);
                    self.parts(&say.text.parts, say.loc.line);
                }
                Stmt::Call(call) => {
                    self.reference("fragment", &call.name, call.loc.line);
                    for arg in &call.args {
                        self.expr(arg);
                    }
                }
                Stmt::Local(local) => self.expr(&local.expr),
                Stmt::DynamicChange(change) => {
                    self.expr(&change.state);
                    self.expr(&change.tags);
                }
                Stmt::Let(decl) => self.expr(&decl.expr),
                Stmt::Set(set) => {
                    self.expr(&set.expr);
                    if self.owner.kind == "fragment" {
                        self.reference("variable", &set.name, set.loc.line);
                    }
                }
                Stmt::Choice(choice) => {
                    self.parts(&choice.label, choice.loc.line);
                    if let Some(expr) = &choice.cond {
                        self.expr(expr);
                    }
                    if let Some(expr) = &choice.enable {
                        self.expr(expr);
                    }
                    self.body(&choice.body);
                }
                Stmt::If(branches) => {
                    for (condition, body) in &branches.branches {
                        if let Some(expr) = condition {
                            self.expr(expr);
                        }
                        self.body(body);
                    }
                }
                Stmt::Scene(scene) => self.body(&scene.body),
                Stmt::Divert(divert) => {
                    if self.owner.kind == "fragment" {
                        if let DivertTarget::Node(name) = &divert.target {
                            self.node(name, divert.loc.line);
                        }
                    }
                }
                Stmt::Change(change) => {
                    if self.owner.kind == "fragment" {
                        let change = &change.change;
                        match change.kind {
                            ChangeKind::Become | ChangeKind::AddTags | ChangeKind::RemoveTags => {}
                            ChangeKind::Meet | ChangeKind::Part => {
                                self.reference("character", &change.id, change.loc.line)
                            }
                            ChangeKind::Grant | ChangeKind::Revoke | ChangeKind::To => {}
                        }
                    }
                }
                Stmt::Return(_) | Stmt::Anchor(_) | Stmt::Effect(_) => {}
            }
        }
    }
}

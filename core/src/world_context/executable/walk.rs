use super::*;
use ExecutableContextRole as Role;

impl<'a> Builder<'a> {
    pub(super) fn program(&mut self) {
        for rule in &self.program.rules {
            if !self.tick() {
                return;
            }
            self.owner = TargetRef::new("rule", &rule.name);
            self.bind_parameters(&rule.parameters);
            if self.index.limited {
                return;
            }
            self.at(
                &rule.expr,
                Some(&rule.file),
                rule.loc.line,
                ExpressionSlot::Rule,
                Role::RuleBody,
            );
        }
        for fragment in &self.program.fragments {
            if !self.tick() {
                return;
            }
            self.owner = TargetRef::new("fragment", &fragment.name);
            self.source_owner = SourceOwner::new(&fragment.file, fragment.loc.line);
            self.bind_parameters(&fragment.parameters);
            if self.index.limited {
                return;
            }
            self.bind_locals(&fragment.body);
            if self.index.limited {
                return;
            }
            self.body(&fragment.body);
        }
        self.locals.clear();
        for (event, file) in self.program.events.iter().zip(&self.program.event_files) {
            if !self.tick() {
                return;
            }
            self.owner = TargetRef::new("event", &event.name);
            self.source_owner = SourceOwner::new(file, event.loc.line);
            if let Some(expression) = &event.after {
                self.at(
                    expression,
                    Some(file),
                    event.loc.line,
                    ExpressionSlot::After,
                    Role::EventRequirement,
                );
            }
            self.body(&event.body);
            for effect in &event.effects {
                if !self.tick() {
                    return;
                }
                self.effect(effect);
            }
        }
        for declaration in &self.program.lets {
            if !self.tick() {
                return;
            }
            self.owner = TargetRef::new("variable", &declaration.name);
            self.declaration(declaration, Some(&declaration.file));
        }
    }

    fn bind_parameters(&mut self, parameters: &'a [crate::language::Parameter]) {
        self.locals.clear();
        for parameter in parameters {
            if !self.tick() {
                return;
            }
            self.locals.insert(&parameter.name);
        }
    }

    fn bind_locals(&mut self, body: &'a [Stmt]) {
        for statement in body {
            if !self.tick() {
                return;
            }
            match statement {
                Stmt::Local(local) => {
                    self.locals.insert(&local.name);
                }
                Stmt::Choice(choice) => self.bind_locals(&choice.body),
                Stmt::If(branches) => {
                    for (_, body) in &branches.branches {
                        self.bind_locals(body);
                        if self.index.limited {
                            return;
                        }
                    }
                }
                Stmt::Scene(scene) => self.bind_locals(&scene.body),
                _ => {}
            }
        }
    }

    fn declaration(&mut self, declaration: &'a LetStmt, file: Option<&'a str>) {
        self.write(
            &declaration.name,
            file,
            declaration.loc.line,
            Role::GlobalInitializer,
        );
        self.at(
            &declaration.expr,
            file,
            declaration.loc.line,
            ExpressionSlot::Value,
            Role::GlobalInitializer,
        );
    }

    fn write(&mut self, name: &str, file: Option<&'a str>, line: u32, role: Role) {
        if (role == Role::AssignmentTarget && self.locals.contains(name))
            || !self.symbols.vars.contains_key(name)
        {
            return;
        }
        let source = file.and_then(|file| {
            self.program
                .source_provenance
                .statements
                .get(&(file.into(), line))
                .map(|statement| (file, statement.span))
        });
        self.emit(
            WorldContextKind::GlobalWrite,
            TargetRef::new("variable", name),
            role,
            source,
        );
    }

    fn effect(&mut self, effect: &'a EffectBlock) {
        if let Some(expression) = &effect.cond {
            let file = self.program.source_provenance.statement_file(
                &self.source_owner,
                effect.loc,
                StatementKind::Effect,
            );
            self.at(
                expression,
                file,
                effect.loc.line,
                ExpressionSlot::Condition(0),
                Role::EffectCondition,
            );
        }
    }

    pub(super) fn body(&mut self, body: &'a [Stmt]) {
        for statement in body {
            if !self.tick() {
                return;
            }
            let loc = crate::language::statement_loc(statement);
            let file = self.program.source_provenance.statement_file(
                &self.source_owner,
                loc,
                StatementKind::of(statement),
            );
            match statement {
                Stmt::Text(text) => {
                    self.parts(&text.parts, file, loc.line, Role::TextInterpolation)
                }
                Stmt::Say(say) => {
                    self.parts(&say.text.parts, file, loc.line, Role::TextInterpolation)
                }
                Stmt::Local(local) => self.at(
                    &local.expr,
                    file,
                    loc.line,
                    ExpressionSlot::Value,
                    Role::LocalInitializer,
                ),
                Stmt::Call(call) => {
                    let source = file.and_then(|file| {
                        self.program.source_provenance.expression(
                            file,
                            loc.line,
                            ExpressionSlot::Call,
                        )
                    });
                    self.emit(
                        WorldContextKind::FragmentCall,
                        TargetRef::new("fragment", &call.name),
                        Role::CallStatement,
                        source.and_then(|source| {
                            source.span.map(|span| (source.file.as_str(), span))
                        }),
                    );
                    for (index, arg) in call.args.iter().enumerate() {
                        self.expression(
                            arg,
                            source.and_then(|source| source.children.get(index)),
                            Role::FragmentArgument,
                        );
                        if self.index.limited {
                            return;
                        }
                    }
                }
                Stmt::Let(declaration) => self.declaration(declaration, file),
                Stmt::Set(set) => {
                    self.write(&set.name, file, loc.line, Role::AssignmentTarget);
                    self.at(
                        &set.expr,
                        file,
                        loc.line,
                        ExpressionSlot::Value,
                        Role::AssignmentValue,
                    );
                }
                Stmt::DynamicChange(change) => {
                    self.at(
                        &change.state,
                        file,
                        loc.line,
                        ExpressionSlot::State,
                        Role::DynamicState,
                    );
                    self.at(
                        &change.tags,
                        file,
                        loc.line,
                        ExpressionSlot::Tags,
                        Role::DynamicTags,
                    );
                }
                Stmt::Choice(choice) => {
                    self.parts(&choice.label, file, loc.line, Role::ChoiceLabel);
                    if let Some(expression) = &choice.cond {
                        self.at(
                            expression,
                            file,
                            loc.line,
                            ExpressionSlot::Condition(0),
                            Role::ChoiceCondition,
                        );
                    }
                    if let Some(expression) = &choice.enable {
                        self.at(
                            expression,
                            file,
                            loc.line,
                            ExpressionSlot::Enable,
                            Role::ChoiceEnable,
                        );
                    }
                    self.body(&choice.body);
                }
                Stmt::If(branches) => {
                    for (index, (condition, body)) in branches.branches.iter().enumerate() {
                        if let Some(expression) = condition {
                            self.at(
                                expression,
                                file,
                                loc.line,
                                ExpressionSlot::Condition(index as u32),
                                Role::BranchCondition,
                            );
                        }
                        self.body(body);
                        if self.index.limited {
                            return;
                        }
                    }
                }
                Stmt::Scene(scene) => {
                    let previous = self.owner.clone();
                    if previous.kind != "fragment" {
                        self.owner =
                            TargetRef::new("scene", &format!("{}.{}", previous.id, scene.name));
                    }
                    self.body(&scene.body);
                    self.owner = previous;
                }
                Stmt::Effect(effect) => self.effect(effect),
                Stmt::Return(_) | Stmt::Divert(_) | Stmt::Change(_) | Stmt::Anchor(_) => {}
            }
        }
    }
}

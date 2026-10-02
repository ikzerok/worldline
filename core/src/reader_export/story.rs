use super::*;
use crate::ast::{Change, ChangeKind, Event, Expr, Stmt, UnOp};

pub(super) struct Projection<'a> {
    compiled: &'a CompileResult,
    routes: &'a BTreeMap<TargetRef, String>,
    private_variables: BTreeSet<String>,
    pub enabled: bool,
}

impl<'a> Projection<'a> {
    pub fn new(
        compiled: &'a CompileResult,
        routes: &'a BTreeMap<TargetRef, String>,
        selection: &ReaderExportSelection,
        target: &TargetRef,
    ) -> Self {
        let mut private_variables = BTreeSet::new();
        if target.kind == "fragment" {
            if let Some(fragment) = compiled
                .program
                .fragments
                .iter()
                .find(|f| f.name == target.id)
            {
                private_variables.extend(fragment.parameters.iter().map(|p| p.name.clone()));
                private_names(&fragment.body, &mut private_variables);
            }
        } else {
            let name = target.id.split('.').next().unwrap_or(&target.id);
            if let Some(event) = compiled.program.events.iter().find(|e| e.name == name) {
                private_names(&event.body, &mut private_variables);
            }
        }
        Self {
            compiled,
            routes,
            private_variables,
            enabled: selection.schema_version == READER_SITE_SCHEMA_VERSION
                && selection
                    .required_features
                    .iter()
                    .any(|f| f == READER_STORY_FEATURE),
        }
    }

    pub fn condition(&self, expression: &Expr) -> String {
        self.expression(expression)
            .unwrap_or_else(|| "未公开条件".into())
    }

    pub fn event(&self, event: &Event) -> Vec<String> {
        if !self.enabled {
            return Vec::new();
        }
        let mut descriptions = Vec::new();
        if event.perm.is_some() {
            descriptions.push("准入：未公开条件".into());
        }
        if let Some(after) = &event.after {
            descriptions.push(format!("前提：{}", self.condition(after)));
        }
        for effect in &event.effects {
            let condition = effect.cond.as_ref().map(|c| self.condition(c));
            let prefix = format!(
                "{}{}",
                effect.when.label(),
                condition.map(|c| format!("（{c}）")).unwrap_or_default()
            );
            for action in &effect.actions {
                descriptions.push(format!("{prefix}：{}", self.change(action)));
            }
        }
        descriptions
    }

    pub fn statement(&self, statement: &Stmt) -> Vec<String> {
        if !self.enabled {
            return Vec::new();
        }
        match statement {
            Stmt::Choice(choice) => {
                let mut descriptions = Vec::new();
                if choice.once {
                    descriptions.push("仅一次选项".into());
                }
                if let Some(condition) = &choice.cond {
                    descriptions.push(format!("可见条件：{}", self.condition(condition)));
                }
                if let Some(condition) = &choice.enable {
                    descriptions.push(format!("可用条件：{}", self.condition(condition)));
                }
                descriptions
            }
            Stmt::Set(set) => vec![self
                .public_name("variable", &set.name)
                .filter(|_| !self.private_variables.contains(&set.name))
                .zip(self.expression(&set.expr))
                .map(|(name, expr)| format!("赋值：{name} = {expr}"))
                .unwrap_or_else(|| "未公开效果".into())],
            Stmt::Change(change) => vec![self.change(&change.change)],
            Stmt::DynamicChange(change) => vec![self
                .expression(&change.state)
                .zip(self.expression(&change.tags))
                .map(|(state, tags)| format!("{}：{state}，{tags}", change.kind.label()))
                .unwrap_or_else(|| "未公开效果".into())],
            Stmt::Divert(divert) if matches!(divert.target, crate::ast::DivertTarget::End) => {
                vec!["故事结束（END）".into()]
            }
            _ => Vec::new(),
        }
    }

    fn expression(&self, expression: &Expr) -> Option<String> {
        match expression {
            Expr::Num(value) if value.is_finite() => Some(value.to_string()),
            Expr::Str(value) => serde_json::to_string(value).ok(),
            Expr::Bool(value) => Some(value.to_string()),
            Expr::Var { name, .. } => {
                if self.private_variables.contains(name)
                    || !self.compiled.program.lets.iter().any(|v| v.name == *name)
                {
                    return None;
                }
                self.public_name("variable", name)
            }
            Expr::Unary { op, expr } => Some(format!(
                "{}({})",
                match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "not ",
                },
                self.expression(expr)?
            )),
            Expr::Binary { op, lhs, rhs } => Some(format!(
                "({} {} {})",
                self.expression(lhs)?,
                op.symbol(),
                self.expression(rhs)?
            )),
            Expr::Call { name, args, .. } => {
                if matches!(name.as_str(), "visits" | "seen") {
                    let id = crate::language::static_id(args.first()?)?;
                    let node = self.compiled.analysis.symbols.resolve_node(id)?;
                    let event = &self.compiled.program.events[node.event];
                    let kind = if node.scenes.is_empty() {
                        "event"
                    } else {
                        "scene"
                    };
                    return Some(format!(
                        "{name}({})",
                        self.public_name(kind, &node.full_name(&event.name))?
                    ));
                }
                if name == "has" && args.len() == 2 {
                    let state = self.public_name("state", crate::language::static_id(&args[0])?)?;
                    let tag = self.public_name("tag", crate::language::static_id(&args[1])?)?;
                    return Some(format!("has({state}, {tag})"));
                }
                if matches!(name.as_str(), "tag" | "state") {
                    let id = crate::language::static_id(args.first()?)?;
                    let kind = match name.as_str() {
                        "tag" => "tag",
                        "state" => "state",
                        _ => "event",
                    };
                    if args.len() != 1 {
                        return None;
                    }
                    return Some(format!("{name}({})", self.public_name(kind, id)?));
                }
                let public_name = if matches!(
                    name.as_str(),
                    "turns"
                        | "rnd"
                        | "tags"
                        | "members"
                        | "count"
                        | "contains"
                        | "union"
                        | "intersect"
                        | "difference"
                        | "when"
                ) {
                    name.clone()
                } else {
                    self.public_name("rule", name)?
                };
                let arguments: Option<Vec<_>> =
                    args.iter().map(|arg| self.expression(arg)).collect();
                Some(format!("{public_name}({})", arguments?.join(", ")))
            }
            _ => None,
        }
    }

    fn public_name(&self, kind: &str, id: &str) -> Option<String> {
        let target = TargetRef::new(kind, id);
        self.routes.get(&target)?;
        Some(
            self.compiled
                .analysis
                .catalog
                .object(&target)?
                .display
                .clone(),
        )
    }

    fn change(&self, change: &Change) -> String {
        let text = (|| -> Option<String> {
            let target = match change.kind {
                ChangeKind::Grant | ChangeKind::Revoke => return None,
                ChangeKind::Meet | ChangeKind::Part => self.public_name("character", &change.id)?,
                ChangeKind::To => self.public_name("storyline", change.to_storyline.as_deref()?)?,
                ChangeKind::Become | ChangeKind::AddTags | ChangeKind::RemoveTags => {
                    self.public_name("state", &change.id)?
                }
            };
            let tags: Option<Vec<_>> = change
                .tags
                .iter()
                .map(|tag| self.public_name("tag", tag))
                .collect();
            let tags = tags?;
            Some(format!(
                "{}：{target}{}",
                change.kind.label(),
                if tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", tags.join("、"))
                }
            ))
        })();
        text.unwrap_or_else(|| "未公开效果".into())
    }
}

fn private_names(statements: &[Stmt], names: &mut BTreeSet<String>) {
    for statement in statements {
        match statement {
            Stmt::Local(local) => {
                names.insert(local.name.clone());
            }
            Stmt::Let(local) => {
                names.insert(local.name.clone());
            }
            Stmt::Choice(choice) => private_names(&choice.body, names),
            Stmt::If(condition) => {
                for (_, branch) in &condition.branches {
                    private_names(branch, names);
                }
            }
            Stmt::Scene(scene) => private_names(&scene.body, names),
            _ => {}
        }
    }
}

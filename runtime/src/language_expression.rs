use super::{evidence::EvidenceRecorder, RunError, Story, Value};
use std::collections::{BTreeMap, HashSet};
use worldline_core::ast::{Expr, Loc};
impl Story<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn eval_language_call(
        &self,
        call: &Expr,
        name: &str,
        args: &[Expr],
        loc: Loc,
        rng: &mut u64,
        recorder: &mut Option<EvidenceRecorder>,
        scope: &BTreeMap<String, Value>,
        declared: &HashSet<String>,
        depth: usize,
    ) -> Result<Value, RunError> {
        let error = |message: &str| RunError {
            message: message.into(),
            node: self.current_node(),
            line: Some(loc.line),
        };
        if name == "when" {
            let [condition, yes, no] = args else {
                return Err(error("when需要3个参数"));
            };
            let Value::Bool(condition) =
                self.eval_in(condition, rng, recorder, scope, declared, depth)?
            else {
                return Err(error("when条件必须是布尔"));
            };
            return self.eval_in(
                if condition { yes } else { no },
                rng,
                recorder,
                scope,
                declared,
                depth,
            );
        }
        if matches!(name, "tag" | "state") {
            let [arg] = args else {
                return Err(error("身份构造需要1个静态参数"));
            };
            let id = worldline_core::language::static_id(arg)
                .ok_or_else(|| error("身份构造参数必须是静态ID"))?;
            let exists = self.program.catalog.iter().any(|d| match d {
                worldline_core::catalog::CatalogDecl::Tag(t) => name == "tag" && t.name == id,
                worldline_core::catalog::CatalogDecl::State(s) => name == "state" && s.id == id,
                _ => false,
            });
            if !exists {
                return Err(error("身份不存在"));
            }
            return Ok(if name == "tag" {
                Value::Tag(id.into())
            } else {
                Value::StateRef(id.into())
            });
        }
        let values = args
            .iter()
            .map(|e| self.eval_in(e, rng, recorder, scope, declared, depth))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(rule) = self.program.rules.iter().find(|r| r.name == name) {
            if rule.parameters.len() != values.len() {
                return Err(error("规则参数数量不符"));
            }
            let mut bindings = BTreeMap::new();
            for (p, value) in rule.parameters.iter().zip(values) {
                if p.kind != value.kind() {
                    return Err(error("规则参数类型不符"));
                }
                bindings.insert(p.name.clone(), value);
            }
            if let Some(recorder) = recorder {
                recorder.enter_rule(call, &rule.expr, &rule.file, rule.loc.line);
            }
            let names = bindings.keys().cloned().collect();
            return self
                .eval_in(&rule.expr, rng, recorder, &bindings, &names, depth + 1)
                .map_err(|mut e| {
                    e.message = format!(
                        "{}（规则 {} 定义于 {}:{}，调用行 {}）",
                        e.message, rule.name, rule.file, rule.loc.line, loc.line
                    );
                    if e.line.is_none_or(|line| line == 0) {
                        e.line = Some(rule.loc.line);
                    }
                    e
                });
        }
        match (name, values.as_slice()) {
            ("tags", _) => {
                let mut ids = Vec::new();
                for value in values {
                    let Value::Tag(id) = value else {
                        return Err(error("tags只接受标签身份"));
                    };
                    ids.push(id);
                }
                ids.sort();
                ids.dedup();
                Ok(Value::TagSet(ids))
            }
            ("members", [Value::StateRef(id)]) => self
                .states
                .get(id)
                .cloned()
                .map(|mut ids| {
                    ids.sort();
                    ids.dedup();
                    Value::TagSet(ids)
                })
                .ok_or_else(|| error("状态身份不存在")),
            ("count", [Value::TagSet(ids)]) => Ok(Value::Num(ids.len() as f64)),
            ("contains", [Value::TagSet(ids), Value::Tag(id)]) => Ok(Value::Bool(ids.contains(id))),
            ("union" | "intersect" | "difference", [Value::TagSet(a), Value::TagSet(b)]) => {
                let mut ids = match name {
                    "union" => [a.as_slice(), b.as_slice()].concat(),
                    "intersect" => a.iter().filter(|id| b.contains(id)).cloned().collect(),
                    _ => a.iter().filter(|id| !b.contains(id)).cloned().collect(),
                };
                ids.sort();
                ids.dedup();
                Ok(Value::TagSet(ids))
            }
            _ => Err(error("集合函数参数数量或类型不符")),
        }
    }
    pub(super) fn eval_global(&self, e: &Expr) -> Result<Value, RunError> {
        let mut rng = self.rng.get();
        let value = self.eval_in(e, &mut rng, &mut None, &BTreeMap::new(), &HashSet::new(), 0);
        self.rng.set(rng);
        value
    }
}

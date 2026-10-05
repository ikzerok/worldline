//! 一次 AST 遍历建立有界 occurrence/邻接索引；不展开调用、不修改旧引用目录。
use super::*;
use crate::{ast::*, source_provenance::*, Symbols};
use std::collections::{BTreeMap, BTreeSet};
mod walk;
const MAX_VISITS: usize = 200_000;
const MAX_OCCURRENCES: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableContextRole {
    RuleBody,
    CallStatement,
    FragmentArgument,
    GlobalInitializer,
    LocalInitializer,
    AssignmentValue,
    AssignmentTarget,
    TextInterpolation,
    ChoiceLabel,
    ChoiceCondition,
    ChoiceEnable,
    BranchCondition,
    EventRequirement,
    EffectCondition,
    DynamicState,
    DynamicTags,
}
impl ExecutableContextRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::RuleBody => "规则表达式",
            Self::CallStatement => "片段调用语句",
            Self::FragmentArgument => "片段调用参数",
            Self::GlobalInitializer => "全局初始化",
            Self::LocalInitializer => "局部初始化表达式",
            Self::AssignmentValue => "赋值右侧",
            Self::AssignmentTarget => "赋值目标",
            Self::TextInterpolation => "正文插值",
            Self::ChoiceLabel => "选择标签插值",
            Self::ChoiceCondition => "选择可见条件",
            Self::ChoiceEnable => "选择可选条件",
            Self::BranchCondition => "条件分支",
            Self::EventRequirement => "事件准入条件",
            Self::EffectCondition => "效果条件",
            Self::DynamicState => "动态状态表达式",
            Self::DynamicTags => "动态标签表达式",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExecutableContextIndex {
    records: Vec<WorldContextRecord>,
    adjacent: BTreeMap<TargetRef, Vec<usize>>,
    pub(super) limited: bool,
    pub(super) source_unavailable: bool,
}
impl ExecutableContextIndex {
    pub(crate) fn build(program: &Program, symbols: &Symbols) -> Self {
        let mut builder = Builder {
            program,
            symbols,
            index: Self::default(),
            visits: 0,
            occurrence: 0,
            owner: TargetRef::new("file", ""),
            source_owner: SourceOwner::new("", 0),
            locals: BTreeSet::new(),
        };
        builder.program();
        builder.index
    }

    pub(super) fn visit(
        &self,
        result: &crate::CompileResult,
        target: &TargetRef,
        source_unavailable: &mut bool,
        source_lines: &mut BTreeMap<String, Vec<usize>>,
        mut callback: impl FnMut(Option<WorldContextRecord>) -> bool,
    ) -> bool {
        for &index in self.adjacent.get(target).into_iter().flatten() {
            if !callback(None) {
                return false;
            }
            let record = &self.records[index];
            let source = &record.source;
            let valid = result
                .sources
                .get(std::path::Path::new(&source.file))
                .is_some_and(|text| {
                    // 同一次查询每份实际源码仅建一次物理行长度表；两跳不重扫整文。
                    let lines = source_lines
                        .entry(source.file.clone())
                        .or_insert_with(|| text.lines().map(|line| line.chars().count()).collect());
                    source
                        .line
                        .checked_sub(1)
                        .and_then(|line| lines.get(line as usize))
                        .is_some_and(|length| {
                            source
                                .column
                                .is_some_and(|column| column > 0 && column as usize <= length + 1)
                        })
                });
            if !valid {
                *source_unavailable = true;
                continue;
            }
            if !callback(Some(record.clone())) {
                return false;
            }
        }
        true
    }
}

struct Builder<'a> {
    program: &'a Program,
    symbols: &'a Symbols,
    index: ExecutableContextIndex,
    visits: usize,
    occurrence: usize,
    owner: TargetRef,
    source_owner: SourceOwner,
    locals: BTreeSet<&'a str>,
}
impl<'a> Builder<'a> {
    fn tick(&mut self) -> bool {
        if self.visits >= MAX_VISITS || self.index.records.len() >= MAX_OCCURRENCES {
            self.index.limited = true;
            return false;
        }
        self.visits += 1;
        true
    }

    fn emit(
        &mut self,
        kind: WorldContextKind,
        target: TargetRef,
        context: ExecutableContextRole,
        source: Option<(&str, crate::Span)>,
    ) {
        let occurrence = self.occurrence;
        self.occurrence += 1;
        let Some((file, span)) =
            source.filter(|(file, span)| !file.is_empty() && span.line > 0 && span.column > 0)
        else {
            self.index.source_unavailable = true;
            return;
        };
        if self.index.records.len() >= MAX_OCCURRENCES {
            self.index.limited = true;
            return;
        }
        let action = match kind {
            WorldContextKind::RuleCall => "调用规则",
            WorldContextKind::FragmentCall => "调用片段",
            WorldContextKind::GlobalRead => "静态读取",
            WorldContextKind::GlobalWrite => "静态写入",
            _ => unreachable!(),
        };
        let record = super::collect::record(
            kind,
            &self.owner,
            &target,
            file,
            span.line,
            Some(span.column),
            occurrence,
            format!("{action} · {}", context.label()),
            WorldContextProvenance::Executable {
                context,
                occurrence,
            },
        );
        let ordinal = self.index.records.len();
        self.index
            .adjacent
            .entry(self.owner.clone())
            .or_default()
            .push(ordinal);
        if self.owner != target {
            self.index.adjacent.entry(target).or_default().push(ordinal);
        }
        self.index.records.push(record);
    }

    fn at(
        &mut self,
        expression: &'a Expr,
        file: Option<&'a str>,
        line: u32,
        slot: ExpressionSlot,
        role: ExecutableContextRole,
    ) {
        let source =
            file.and_then(|file| self.program.source_provenance.expression(file, line, slot));
        self.expression(expression, source, role);
    }

    fn expression(
        &mut self,
        expression: &'a Expr,
        source: Option<&'a ExpressionSource>,
        role: ExecutableContextRole,
    ) {
        if !self.tick() {
            return;
        }
        let location =
            source.and_then(|source| source.span.map(|span| (source.file.as_str(), span)));
        match expression {
            Expr::Var { name, .. }
                if !self.locals.contains(name.as_str()) && self.symbols.vars.contains_key(name) =>
            {
                self.emit(
                    WorldContextKind::GlobalRead,
                    TargetRef::new("variable", name),
                    role,
                    location,
                );
            }
            Expr::Call { name, args, .. } => {
                if self.symbols.rule_results.contains_key(name) {
                    self.emit(
                        WorldContextKind::RuleCall,
                        TargetRef::new("rule", name),
                        role,
                        location,
                    );
                } else if matches!(
                    name.as_str(),
                    "tag" | "state" | "has" | "seen" | "visits" | "perm"
                ) {
                    return;
                }
                for (index, arg) in args.iter().enumerate() {
                    self.expression(
                        arg,
                        source.and_then(|source| source.children.get(index)),
                        role,
                    );
                    if self.index.limited {
                        break;
                    }
                }
            }
            Expr::Unary { expr, .. } => self.expression(
                expr,
                source.and_then(|source| source.children.first()),
                role,
            ),
            Expr::Binary { lhs, rhs, .. } => {
                self.expression(lhs, source.and_then(|source| source.children.first()), role);
                if !self.index.limited {
                    self.expression(rhs, source.and_then(|source| source.children.get(1)), role);
                }
            }
            _ => {}
        }
    }

    fn parts(
        &mut self,
        parts: &'a [TextPart],
        file: Option<&'a str>,
        line: u32,
        role: ExecutableContextRole,
    ) {
        let mut ordinal = 0;
        for part in parts {
            if let TextPart::Expr(expression) = part {
                self.at(expression, file, line, ExpressionSlot::Text(ordinal), role);
                ordinal += 1;
                if self.index.limited {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_budget_is_observable_only_in_opted_in_queries() {
        let mut compiled = crate::compile_source(
            "world.wl",
            "let score = 1\nevent start\n  {score}\n  -> END\n",
        );
        compiled.analysis.executable_context.limited = true;
        let target = TargetRef::new("variable", "score");
        let old = compiled
            .query_world_context(&target, Default::default())
            .unwrap();
        assert!(old.complete);
        let partial = compiled
            .query_world_context(
                &target,
                WorldContextOptions {
                    include_executable: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!partial.complete && partial.truncated);
        assert_eq!(partial.total, None);
        assert!(partial
            .reasons
            .contains(&WorldContextLimit::ExecutableIndexBudget));
        assert!(!partial.records.is_empty());
    }
    #[test]
    fn index_stops_before_visiting_beyond_hard_budget() {
        let compiled = crate::compile_source("world.wl", "event start\n  -> END\n");
        let mut builder = Builder {
            program: &compiled.program,
            symbols: &compiled.analysis.symbols,
            index: ExecutableContextIndex::default(),
            visits: MAX_VISITS - 1,
            occurrence: 0,
            owner: TargetRef::new("event", "start"),
            source_owner: SourceOwner::new("world.wl", 1),
            locals: BTreeSet::new(),
        };
        assert!(builder.tick());
        assert!(!builder.tick());
        assert!(builder.index.limited);
        assert_eq!(builder.visits, MAX_VISITS);
    }
}

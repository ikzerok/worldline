//! 世界观、人物资料与人物反向事件索引;与执行流分析共用符号表。
use std::collections::{BTreeMap, HashSet};

use crate::analysis::{CharacterRelationInfo, Symbols, WorldInfo};
use crate::ast::{ChangeKind, Program, Property, PropertyValue, Stmt};
use crate::{Diagnostic, Span};

pub(crate) fn properties(
    items: &[Property],
    file: &str,
    diags: &mut Vec<Diagnostic>,
) -> BTreeMap<String, PropertyValue> {
    let mut result = BTreeMap::new();
    for p in items {
        if result.insert(p.name.clone(), p.value.clone()).is_some() {
            diags.push(Diagnostic::error(
                "A212",
                file,
                Span::new(p.loc.line, 1, 8),
                format!("属性 `{}` 重复定义", p.name),
            ));
        }
    }
    result
}

pub(super) fn analyze_metadata(
    program: &Program,
    symbols: &mut Symbols,
    diags: &mut Vec<Diagnostic>,
) -> Option<WorldInfo> {
    for world in program.worlds.iter().skip(1) {
        diags.push(
            Diagnostic::error(
                "A211",
                &world.file,
                Span::new(world.loc.line, 1, 5),
                "一个工程只能定义一个世界观",
            )
            .with_related(
                &program.worlds[0].file,
                Span::new(program.worlds[0].loc.line, 1, 5),
            ),
        );
    }
    let world = program.worlds.first().map(|w| WorldInfo {
        id: w.name.clone(),
        display: w.display.clone().unwrap_or_else(|| w.name.clone()),
        description: w.description.clone(),
        properties: properties(&w.properties, &w.file, diags),
        file: w.file.clone(),
        line: w.loc.line,
    });
    let ids: HashSet<_> = symbols.characters.keys().cloned().collect();
    for ch in &program.characters {
        let Some(info) = symbols.characters.get_mut(&ch.name) else {
            continue;
        };
        if info.decl_file != ch.file || info.decl_span.line != ch.loc.line {
            continue;
        }
        info.properties = properties(&ch.properties, &ch.file, diags);
        let mut relations = HashSet::new();
        for relation in &ch.relations {
            if !ids.contains(&relation.target) {
                diags.push(Diagnostic::error(
                    "A208",
                    &ch.file,
                    Span::new(relation.loc.line, 1, 8),
                    format!("人物关系引用了未定义角色 `{}`", relation.target),
                ));
            }
            if !relations.insert((&relation.target, &relation.label)) {
                diags.push(Diagnostic::error(
                    "A212",
                    &ch.file,
                    Span::new(relation.loc.line, 1, 8),
                    "人物关系的目标和名称重复",
                ));
            }
            info.relations.push(CharacterRelationInfo {
                target: relation.target.clone(),
                label: relation.label.clone(),
                file: ch.file.clone(),
                line: relation.loc.line,
            });
        }
    }
    for event in &program.events {
        let mut used: HashSet<String> = event.characters.iter().cloned().collect();
        for effect in &event.effects {
            for change in &effect.actions {
                if matches!(change.kind, ChangeKind::Meet | ChangeKind::Part) {
                    used.insert(change.id.clone());
                }
            }
        }
        collect_characters(&event.body, &mut used);
        for id in used {
            if let Some(info) = symbols.characters.get_mut(&id) {
                if !info.events.contains(&event.name) {
                    info.events.push(event.name.clone());
                }
            }
        }
    }
    world
}

fn collect_characters(body: &[Stmt], used: &mut HashSet<String>) {
    for stmt in body {
        match stmt {
            Stmt::Change(c) if matches!(c.change.kind, ChangeKind::Meet | ChangeKind::Part) => {
                used.insert(c.change.id.clone());
            }
            Stmt::Scene(s) => collect_characters(&s.body, used),
            Stmt::Choice(c) => collect_characters(&c.body, used),
            Stmt::If(i) => {
                for (_, body) in &i.branches {
                    collect_characters(body, used);
                }
            }
            _ => {}
        }
    }
}

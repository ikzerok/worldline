//! 仅映射真实物理编辑带来的来源坐标；完整作者资料与引用仍参加比较。
use super::Failure;
use crate::{
    catalog::{Catalog, ReferenceInfo},
    CompileResult, Program,
};
use std::collections::BTreeMap;

pub(super) struct LineMap {
    pub source: String,
    pub destination: String,
    pub first: u32,
    pub last: u32,
    pub inserted: u32,
    pub removed_newlines: u32,
}

impl LineMap {
    pub(super) fn location(&self, file: &mut String, line: &mut u32) {
        if *file != self.source {
            return;
        }
        if (self.first..=self.last).contains(line) {
            *file = self.destination.clone();
            *line = self.inserted + *line - self.first;
        } else if *line > self.last {
            *line -= self.removed_newlines;
        }
    }

    fn line(&self, file: &str, line: u32) -> u32 {
        let mut file = file.to_owned();
        let mut line = line;
        self.location(&mut file, &mut line);
        line
    }
}

pub(super) fn equivalent(
    before: &CompileResult,
    after: &CompileResult,
    map: &LineMap,
) -> Result<(), Failure> {
    if after.has_errors() {
        return Err("移源候选编译失败，无法证明完整资料等价，工程未修改".into());
    }
    if before.program.files != after.program.files
        || before.program.entry != after.program.entry
        || before.program.language_version != after.program.language_version
    {
        return Err(Failure::semantic(
            "移源改变了源码加载顺序、默认入口或语言版本，工程未修改",
        ));
    }
    if before.analysis.fingerprint != after.analysis.fingerprint {
        return Err(Failure::semantic(
            "移源改变运行身份/指纹，旧存档不可直接沿用，工程未修改",
        ));
    }
    let mut expected = before.analysis.catalog.clone();
    let contexts = context_map(&before.program, map);
    map_catalog(&mut expected, map, &contexts);
    let mut actual = after.analysis.catalog.clone();
    canonical(&mut expected);
    canonical(&mut actual);
    if serde_json::to_value(expected).map_err(|error| error.to_string())?
        != serde_json::to_value(actual).map_err(|error| error.to_string())?
    {
        return Err(Failure::semantic(
            "移源后的完整资料、正式引用或来源不等价，工程未修改",
        ));
    }
    // schema 的正式绑定和字段约束不能只用运行指纹或目录引用证明。
    let mut schemas = before.program.schemas.clone();
    for schema in &mut schemas {
        for field in &mut schema.fields {
            field.loc.line = map.line(&schema.file, field.loc.line);
        }
        map.location(&mut schema.file, &mut schema.loc.line);
    }
    let mut bindings = before.program.schema_bindings.clone();
    for binding in &mut bindings {
        map.location(&mut binding.file, &mut binding.loc.line);
    }
    if schemas != after.program.schemas || bindings != after.program.schema_bindings {
        return Err(Failure::semantic(
            "移源改变了资料 schema 或正式绑定，工程未修改",
        ));
    }
    entity_declarations(&before.program, &after.program, map)
}

fn entity_declarations(before: &Program, after: &Program, map: &LineMap) -> Result<(), Failure> {
    if before.entities.len() != after.entities.len() {
        return Err(Failure::semantic("移源改变 entity 声明数量"));
    }
    for old in &before.entities {
        let Some(new) = after.entities.iter().find(|value| value.name == old.name) else {
            return Err(Failure::semantic("移源丢失 entity 稳定身份"));
        };
        let mut file = old.file.clone();
        let mut loc = old.loc;
        map.location(&mut file, &mut loc.line);
        let properties: Vec<_> = old
            .properties
            .iter()
            .map(|property| {
                let mut loc = property.loc;
                loc.line = map.line(&old.file, loc.line);
                (&property.name, &property.value, loc)
            })
            .collect();
        let actual: Vec<_> = new
            .properties
            .iter()
            .map(|property| (&property.name, &property.value, property.loc))
            .collect();
        if old.entity_type != new.entity_type
            || old.display != new.display
            || old.description != new.description
            || file != new.file
            || loc != new.loc
            || properties != actual
        {
            return Err(Failure::semantic(
                "移源改变了 entity 完整声明、属性原值/类型或次序",
            ));
        }
    }
    let moved = before
        .entities
        .iter()
        .find(|entity| entity.file == map.source && entity.loc.line == map.first)
        .ok_or("移源声明身份消失")?;
    let retained = |program: &Program| {
        program
            .entities
            .iter()
            .filter(|entity| entity.name != moved.name)
            .map(|entity| entity.name.clone())
            .collect::<Vec<_>>()
    };
    if retained(before) != retained(after) {
        return Err(Failure::semantic("移源改变了其他 entity 的声明顺序"));
    }
    Ok(())
}

fn canonical(catalog: &mut Catalog) {
    catalog.objects.sort_by(|a, b| a.target.cmp(&b.target));
    catalog.references.sort_by(ReferenceInfo::canonical_cmp);
}

type ContextMap = BTreeMap<(String, String), String>;
fn map_catalog(catalog: &mut Catalog, map: &LineMap, contexts: &ContextMap) {
    for object in &mut catalog.objects {
        if object.target.kind != "file" {
            map.location(&mut object.file, &mut object.line);
        }
    }
    for reference in &mut catalog.references {
        map.location(&mut reference.file, &mut reference.line);
    }
    for alias in &mut catalog.aliases {
        map.location(&mut alias.file, &mut alias.line);
    }
    for link in &mut catalog.text_links {
        map.location(&mut link.file, &mut link.line);
    }
    for link in catalog.marks.iter_mut().chain(&mut catalog.attachments) {
        map.location(&mut link.file, &mut link.line);
    }
    for anchor in catalog.anchors.values_mut() {
        map.location(&mut anchor.file, &mut anchor.line);
        for link in &mut anchor.links {
            map.location(&mut link.file, &mut link.line);
        }
    }
    let site = |site: &mut crate::states::StateChangeSite| {
        for context in &mut site.contexts {
            if let Some(value) = contexts.get(&(site.file.clone(), context.clone())) {
                *context = value.clone();
            }
        }
        map.location(&mut site.file, &mut site.line);
    };
    for state in catalog.states.values_mut() {
        map.location(&mut state.file, &mut state.line);
        for value in &mut state.changes {
            site(value);
        }
    }
    for value in &mut catalog.dynamic_state_changes {
        site(value);
    }
    for tag in catalog.tags.values_mut() {
        map.location(&mut tag.file, &mut tag.line);
    }
    for entity in catalog.entities.values_mut() {
        map.location(&mut entity.file, &mut entity.line);
    }
    for asset in catalog.assets.values_mut() {
        map.location(&mut asset.file, &mut asset.line);
    }
    for kind in catalog.relation_types.values_mut() {
        map.location(&mut kind.file, &mut kind.line);
    }
    for relation in catalog.relations.values_mut() {
        map.location(&mut relation.file, &mut relation.line);
    }
    for relation in &mut catalog.legacy_relations {
        map.location(&mut relation.handle.file, &mut relation.handle.line);
    }
}

/// 只对正式 AST 生成的上下文字符串重建已知行号；不解析/替换作者普通文字。
fn context_map(program: &Program, map: &LineMap) -> ContextMap {
    fn walk(body: &[crate::ast::Stmt], file: &str, map: &LineMap, out: &mut ContextMap) {
        use crate::ast::Stmt;
        for stmt in body {
            match stmt {
                Stmt::Scene(scene) => walk(&scene.body, file, map, out),
                Stmt::Choice(choice) => {
                    out.insert(
                        (
                            file.into(),
                            format!("选择：{}（第 {} 行）", choice.label_raw, choice.loc.line),
                        ),
                        format!(
                            "选择：{}（第 {} 行）",
                            choice.label_raw,
                            map.line(file, choice.loc.line)
                        ),
                    );
                    walk(&choice.body, file, map, out);
                }
                Stmt::If(condition) => {
                    for (index, (_, body)) in condition.branches.iter().enumerate() {
                        out.insert(
                            (
                                file.into(),
                                format!("第 {} 行条件的分支 {}", condition.loc.line, index + 1),
                            ),
                            format!(
                                "第 {} 行条件的分支 {}",
                                map.line(file, condition.loc.line),
                                index + 1
                            ),
                        );
                        walk(body, file, map, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut output = ContextMap::new();
    for (event, file) in program.events.iter().zip(&program.event_files) {
        walk(&event.body, file, map, &mut output);
        for effect in &event.effects {
            if effect.cond.is_some() {
                output.insert(
                    (
                        file.clone(),
                        format!("受第 {} 行效果条件约束", effect.loc.line),
                    ),
                    format!("受第 {} 行效果条件约束", map.line(file, effect.loc.line)),
                );
            }
        }
    }
    for fragment in &program.fragments {
        walk(&fragment.body, &fragment.file, map, &mut output);
    }
    output
}

#[cfg(test)]
mod tests;

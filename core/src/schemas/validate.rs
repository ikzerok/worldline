use super::{SchemaDecl, SchemaIndex, SchemaInstance, SchemaType};
use crate::ast::{Loc, Program, Property, PropertyValue};
use crate::catalog::{Catalog, ReferenceInfo, TargetRef};
use crate::{Diagnostic, Span};
use std::collections::{BTreeMap, BTreeSet};

fn diagnostic(code: &'static str, file: &str, loc: Loc, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, file, Span::new(loc.line, loc.column, 1), message)
}

/// 唯一的持续约束校验器。不改值，不读取默认值，不执行表达式。
pub fn validate(program: &Program, catalog: &Catalog) -> SchemaIndex {
    let mut index = SchemaIndex {
        schemas: program.schemas.clone(),
        bindings: program.schema_bindings.clone(),
        instances: Vec::new(),
        diagnostics: Vec::new(),
    };
    if !program.language_version.supports_language_112()
        && (!program.schemas.is_empty() || !program.schema_bindings.is_empty())
    {
        let (file, loc) = program
            .schemas
            .first()
            .map(|s| (s.file.as_str(), s.loc))
            .or_else(|| {
                program
                    .schema_bindings
                    .first()
                    .map(|b| (b.file.as_str(), b.loc))
            })
            .expect("schema declaration or binding exists");
        index.diagnostics.push(diagnostic(
            "SCH001",
            file,
            loc,
            "持续 schema 需要显式语言 1.12",
        ));
    }
    let mut declarations = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for schema in &program.schemas {
        if let Some(previous) = declarations.insert(schema.id.clone(), schema) {
            ambiguous.insert(schema.id.clone());
            index.diagnostics.push(
                diagnostic(
                    "SCH002",
                    &schema.file,
                    schema.loc,
                    format!("schema `{}` 重复声明", schema.id),
                )
                .with_related(
                    &previous.file,
                    Span::new(previous.loc.line, previous.loc.column, 1),
                ),
            );
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for field in &schema.fields {
            if !ids.insert(&field.id) || !keys.insert(&field.key) {
                index.diagnostics.push(diagnostic(
                    "SCH002",
                    &schema.file,
                    field.loc,
                    format!(
                        "schema `{}` 的字段 ID 或 property 键重复：{}/{}",
                        schema.id, field.id, field.key
                    ),
                ));
            }
        }
    }
    let mut bound: BTreeMap<TargetRef, &super::SchemaBinding> = BTreeMap::new();
    for binding in &program.schema_bindings {
        let mut diagnostics = Vec::new();
        if let Some(previous) = bound.get(&binding.target) {
            diagnostics.push(
                diagnostic(
                    "SCH003",
                    &binding.file,
                    binding.loc,
                    format!(
                        "{}:{} 重复绑定 schema，不允许多重约束合并",
                        binding.target.kind, binding.target.id
                    ),
                )
                .with_related(
                    &previous.file,
                    Span::new(previous.loc.line, previous.loc.column, 1),
                ),
            );
        } else {
            bound.insert(binding.target.clone(), binding);
        }
        let object = instance(program, &binding.target);
        if object.is_none() {
            diagnostics.push(diagnostic(
                "SCH003",
                &binding.file,
                binding.loc,
                format!(
                    "schema 绑定对象不存在：{}:{}",
                    binding.target.kind, binding.target.id
                ),
            ));
        }
        match declarations.get(&binding.schema_id) {
            _ if ambiguous.contains(&binding.schema_id) => diagnostics.push(diagnostic(
                "SCH003",
                &binding.file,
                binding.loc,
                format!("schema `{}` 有重复声明，绑定无法确定", binding.schema_id),
            )),
            Some(schema) => {
                if schema.kind != binding.target.kind
                    || schema.entity_type.as_ref().is_some_and(|kind| {
                        catalog
                            .entities
                            .get(&binding.target.id)
                            .is_none_or(|entity| &entity.entity_type != kind)
                    })
                {
                    diagnostics.push(
                        diagnostic(
                            "SCH003",
                            &binding.file,
                            binding.loc,
                            format!(
                                "对象 {}:{} 不符合 schema `{}` 的 kind/entity_type",
                                binding.target.kind, binding.target.id, schema.id
                            ),
                        )
                        .with_related(
                            &schema.file,
                            Span::new(schema.loc.line, schema.loc.column, 1),
                        ),
                    );
                } else if let Some((file, loc, properties)) = object {
                    validate_properties(schema, catalog, file, loc, properties, &mut diagnostics);
                }
            }
            None => diagnostics.push(diagnostic(
                "SCH003",
                &binding.file,
                binding.loc,
                format!("schema `{}` 不存在", binding.schema_id),
            )),
        }
        let (file, line) = object.map_or((binding.file.clone(), binding.loc.line), |(f, l, _)| {
            (f.into(), l.line)
        });
        index.instances.push(SchemaInstance {
            target: binding.target.clone(),
            schema_id: binding.schema_id.clone(),
            file,
            line,
            diagnostics: diagnostics.clone(),
        });
        index.diagnostics.extend(diagnostics);
    }
    program
        .source_provenance
        .resolve_diagnostics(&mut index.diagnostics);
    for instance in &mut index.instances {
        program
            .source_provenance
            .resolve_diagnostics(&mut instance.diagnostics);
    }
    crate::sort_diagnostics(&mut index.diagnostics);
    index
}

fn validate_properties(
    schema: &SchemaDecl,
    catalog: &Catalog,
    file: &str,
    loc: Loc,
    properties: &[Property],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for field in &schema.fields {
        let property = properties.iter().find(|p| p.name == field.key);
        let problem = match property {
            None if field.required => Some((
                "SCH004",
                format!(
                    "缺少 schema `{}` 必填字段 `{}`（字段身份 `{}`）",
                    schema.id, field.key, field.id
                ),
            )),
            None => None,
            Some(property) => {
                check_value(&property.value, &field.value_type, catalog).map(|(code, message)| {
                    (
                        code,
                        format!("schema `{}` 字段 `{}`：{message}", schema.id, field.key),
                    )
                })
            }
        };
        if let Some((code, message)) = problem {
            diagnostics.push(
                diagnostic(code, file, property.map_or(loc, |p| p.loc), message)
                    .with_related(&schema.file, Span::new(field.loc.line, field.loc.column, 1)),
            );
        }
    }
    if schema.closed {
        for property in properties {
            if !schema.fields.iter().any(|field| field.key == property.name) {
                diagnostics.push(
                    diagnostic(
                        "SCH008",
                        file,
                        property.loc,
                        format!(
                            "closed schema `{}` 未声明属性 `{}`",
                            schema.id, property.name
                        ),
                    )
                    .with_related(
                        &schema.file,
                        Span::new(schema.loc.line, schema.loc.column, 1),
                    ),
                );
            }
        }
    }
}

fn check_value(
    value: &PropertyValue,
    expected: &SchemaType,
    catalog: &Catalog,
) -> Option<(&'static str, &'static str)> {
    let correct_type = match expected {
        SchemaType::Text | SchemaType::Enum { .. } => matches!(value, PropertyValue::Str(_)),
        SchemaType::Number => matches!(value, PropertyValue::Num(n) if n.is_finite()),
        SchemaType::Boolean => matches!(value, PropertyValue::Bool(_)),
        SchemaType::Ref { .. } => matches!(value, PropertyValue::Ref(_)),
    };
    if !correct_type {
        return Some(("SCH005", "值类型不符，不进行自动强转"));
    }
    match (value, expected) {
        (PropertyValue::Str(value), SchemaType::Enum { values }) if !values.contains(value) => {
            Some(("SCH006", "值不属于枚举集合"))
        }
        (
            PropertyValue::Ref(reference),
            SchemaType::Ref {
                target_kind,
                entity_type,
            },
        ) => {
            if &reference.kind != target_kind {
                return Some(("SCH007", "强引用目标 kind 不符"));
            }
            // 缺失目标统一由原强引用校验器报 A214，避免伪称为子类不符。
            if let Some(actual) = catalog
                .entities
                .get(&reference.id)
                .filter(|_| reference.kind == "entity")
            {
                if entity_type
                    .as_ref()
                    .is_some_and(|expected| expected != &actual.entity_type)
                {
                    return Some(("SCH007", "强引用目标 entity_type 不符"));
                }
            }
            None
        }
        _ => None,
    }
}

fn instance<'a>(
    program: &'a Program,
    target: &TargetRef,
) -> Option<(&'a str, Loc, &'a [Property])> {
    match target.kind.as_str() {
        "world" => program
            .worlds
            .iter()
            .find(|x| x.name == target.id)
            .map(|x| (x.file.as_str(), x.loc, x.properties.as_slice())),
        "character" => program
            .characters
            .iter()
            .find(|x| x.name == target.id)
            .map(|x| (x.file.as_str(), x.loc, x.properties.as_slice())),
        "entity" => program
            .entities
            .iter()
            .find(|x| x.name == target.id)
            .map(|x| (x.file.as_str(), x.loc, x.properties.as_slice())),
        "relation" => program
            .relations
            .iter()
            .find(|x| x.id == target.id)
            .map(|x| (x.file.as_str(), x.loc, x.properties.as_slice())),
        _ => None,
    }
}

pub(crate) fn add_binding_references(program: &Program, catalog: &mut Catalog) {
    catalog
        .references
        .extend(program.schema_bindings.iter().map(|binding| ReferenceInfo {
            source: TargetRef::new("file", &binding.file),
            target: binding.target.clone(),
            kind: "schema 对象绑定".into(),
            file: binding.file.clone(),
            line: binding.loc.line,
        }));
}

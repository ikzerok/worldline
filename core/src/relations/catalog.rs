use super::{LegacyRelationHandle, LegacyRelationInfo, RelationTypeInfo, SemanticRelationInfo};
use crate::ast::{Program, Property, PropertyValue, RelationDef, RelationTypeDecl};
use crate::catalog::{Catalog, TargetRef};
use crate::diagnostic::{Diagnostic, Span};
use std::collections::{BTreeMap, HashMap, HashSet};

/// 从 AST 建立关系目录并产生关系诊断。
pub(crate) fn collect(program: &Program, catalog: &mut Catalog, diags: &mut Vec<Diagnostic>) {
    let mut known_objects: HashSet<TargetRef> = catalog
        .objects
        .iter()
        .map(|object| object.target.clone())
        .collect();
    let mut relation_types: BTreeMap<String, RelationTypeInfo> = BTreeMap::new();
    for declaration in &program.relation_types {
        if declaration.name.is_empty() {
            continue;
        }
        if let Some(old) = relation_types.get(&declaration.name) {
            diags.push(
                Diagnostic::error(
                    "A220",
                    &declaration.file,
                    Span::new(
                        declaration.loc.line,
                        declaration.loc.column,
                        declaration.name.chars().count() as u32,
                    ),
                    format!("关系类型 `{}` 重复定义", declaration.name),
                )
                .with_related(
                    &old.file,
                    Span::new(old.line, 1, declaration.name.chars().count() as u32),
                ),
            );
            continue;
        }
        for (side, kind) in [
            ("from", declaration.from_kind.as_deref()),
            ("to", declaration.to_kind.as_deref()),
        ] {
            if let Some(kind) = kind {
                if !crate::catalog::TARGET_KINDS.contains(&kind) {
                    diags.push(Diagnostic::error(
                        "A222",
                        &declaration.file,
                        Span::new(
                            declaration.loc.line,
                            declaration.loc.column,
                            declaration.name.len() as u32,
                        ),
                        format!(
                            "关系类型 `{}` 的 {side} 端点类型 `{kind}` 无效",
                            declaration.name
                        ),
                    ));
                }
            }
        }
        relation_types.insert(declaration.name.clone(), type_info(declaration));
    }
    catalog.relation_types = relation_types;

    // 关系实例本身也是完整的 TargetRef；先登记全部 ID，允许声明顺序之外的
    // 关系范围或关系端点引用，并由重复 ID 诊断决定最终目录条目。
    known_objects.extend(
        program
            .relations
            .iter()
            .filter(|declaration| !declaration.id.is_empty())
            .map(|declaration| TargetRef::new("relation", &declaration.id)),
    );
    let mut relations: BTreeMap<String, SemanticRelationInfo> = BTreeMap::new();
    for declaration in &program.relations {
        if declaration.id.is_empty() {
            continue;
        }
        if let Some(old) = relations.get(&declaration.id) {
            diags.push(
                Diagnostic::error(
                    "A223",
                    &declaration.file,
                    Span::new(
                        declaration.loc.line,
                        declaration.loc.column,
                        declaration.id.chars().count() as u32,
                    ),
                    format!("关系 `{}` 重复定义", declaration.id),
                )
                .with_related(
                    &old.file,
                    Span::new(old.line, 1, declaration.id.chars().count() as u32),
                ),
            );
            continue;
        }
        let type_info = catalog.relation_types.get(&declaration.relation_type);
        if type_info.is_none() {
            diags.push(Diagnostic::error(
                "A221",
                &declaration.file,
                Span::new(
                    declaration.loc.line,
                    declaration.loc.column,
                    declaration.id.len() as u32,
                ),
                format!(
                    "关系 `{}` 引用了未定义的关系类型 `{}`",
                    declaration.id, declaration.relation_type
                ),
            ));
        }
        validate_endpoint(
            &declaration.from,
            type_info.and_then(|info| info.from_kind.as_deref()),
            "from",
            declaration,
            &known_objects,
            diags,
        );
        validate_endpoint(
            &declaration.to,
            type_info.and_then(|info| info.to_kind.as_deref()),
            "to",
            declaration,
            &known_objects,
            diags,
        );
        for scope in &declaration.scope_refs {
            if !known_objects.contains(scope) {
                diags.push(Diagnostic::error(
                    "A222",
                    &declaration.file,
                    Span::new(
                        declaration.loc.line,
                        declaration.loc.column,
                        declaration.id.len() as u32,
                    ),
                    format!(
                        "关系 `{}` 的 scope 引用了不存在的对象 {}:{}",
                        declaration.id, scope.kind, scope.id
                    ),
                ));
            }
        }
        let properties = properties(&declaration.properties, &declaration.file, diags);
        let info = SemanticRelationInfo {
            id: declaration.id.clone(),
            relation_type: declaration.relation_type.clone(),
            from_ref: declaration.from.clone(),
            to_ref: declaration.to.clone(),
            description: declaration.description.clone(),
            source_note: declaration.source_note.clone(),
            scope_refs: declaration.scope_refs.clone(),
            properties,
            file: declaration.file.clone(),
            line: declaration.loc.line,
        };
        catalog.add_object(
            "relation",
            &declaration.id,
            if declaration.description.is_empty() {
                &declaration.id
            } else {
                &declaration.description
            },
            &declaration.file,
            declaration.loc.line,
        );
        known_objects.insert(TargetRef::new("relation", &declaration.id));
        relations.insert(declaration.id.clone(), info);
    }
    catalog.relations = relations;
    catalog.relation_index = relation_index(catalog);

    // 关系端点是内容引用；它们必须参加删除影响检查。
    for relation in catalog.relations.values() {
        catalog.references.push(crate::catalog::ReferenceInfo {
            source: TargetRef::new("relation", &relation.id),
            target: relation.from_ref.clone(),
            kind: "语义关系 from 端点".into(),
            file: relation.file.clone(),
            line: relation.line,
        });
        catalog.references.push(crate::catalog::ReferenceInfo {
            source: TargetRef::new("relation", &relation.id),
            target: relation.to_ref.clone(),
            kind: "语义关系 to 端点".into(),
            file: relation.file.clone(),
            line: relation.line,
        });
        for scope in &relation.scope_refs {
            catalog.references.push(crate::catalog::ReferenceInfo {
                source: TargetRef::new("relation", &relation.id),
                target: scope.clone(),
                kind: "语义关系范围".into(),
                file: relation.file.clone(),
                line: relation.line,
            });
        }
    }

    // 旧关系始终保留旧语义和指纹，只追加只读投影。重复项按出现次序区分。
    let mut legacy = Vec::new();
    for character in &program.characters {
        let source = TargetRef::new("character", &character.name);
        let mut occurrences: HashMap<(String, String), u32> = HashMap::new();
        for relation in &character.relations {
            let key = (relation.target.clone(), relation.label.clone());
            let occurrence = occurrences.entry(key).and_modify(|n| *n += 1).or_insert(1);
            legacy.push(LegacyRelationInfo {
                handle: LegacyRelationHandle {
                    source: source.clone(),
                    target: TargetRef::new("character", &relation.target),
                    label: relation.label.clone(),
                    occurrence: *occurrence,
                    file: character.file.clone(),
                    line: relation.loc.line,
                },
            });
        }
    }
    catalog.legacy_relations = legacy;
}

fn type_info(declaration: &RelationTypeDecl) -> RelationTypeInfo {
    RelationTypeInfo {
        id: declaration.name.clone(),
        display: declaration
            .display
            .clone()
            .unwrap_or_else(|| declaration.name.clone()),
        inverse_display: declaration.inverse_display.clone(),
        direction: declaration.direction,
        from_kind: declaration.from_kind.clone(),
        to_kind: declaration.to_kind.clone(),
        file: declaration.file.clone(),
        line: declaration.loc.line,
    }
}

fn properties(
    items: &[Property],
    file: &str,
    diags: &mut Vec<Diagnostic>,
) -> BTreeMap<String, PropertyValue> {
    let mut result = BTreeMap::new();
    for property in items {
        if result
            .insert(property.name.clone(), property.value.clone())
            .is_some()
        {
            diags.push(Diagnostic::error(
                "A220",
                file,
                Span::new(
                    property.loc.line,
                    property.loc.column,
                    property.name.len() as u32,
                ),
                format!("关系属性 `{}` 重复定义", property.name),
            ));
        }
    }
    result
}

fn validate_endpoint(
    endpoint: &TargetRef,
    expected_kind: Option<&str>,
    side: &str,
    declaration: &RelationDef,
    known_objects: &HashSet<TargetRef>,
    diags: &mut Vec<Diagnostic>,
) {
    if !crate::catalog::TARGET_KINDS.contains(&endpoint.kind.as_str()) {
        diags.push(Diagnostic::error(
            "A222",
            &declaration.file,
            Span::new(
                declaration.loc.line,
                declaration.loc.column,
                declaration.id.len() as u32,
            ),
            format!(
                "关系 `{}` 的 {side} 端点类型 `{}` 无效",
                declaration.id, endpoint.kind
            ),
        ));
    } else if !known_objects.contains(endpoint) {
        diags.push(Diagnostic::error(
            "A222",
            &declaration.file,
            Span::new(
                declaration.loc.line,
                declaration.loc.column,
                declaration.id.len() as u32,
            ),
            format!(
                "关系 `{}` 的 {side} 端点 {}:{} 不存在",
                declaration.id, endpoint.kind, endpoint.id
            ),
        ));
    }
    if let Some(expected) = expected_kind {
        if endpoint.kind != expected {
            diags.push(Diagnostic::error(
                "A222",
                &declaration.file,
                Span::new(
                    declaration.loc.line,
                    declaration.loc.column,
                    declaration.id.len() as u32,
                ),
                format!(
                    "关系 `{}` 的 {side} 端点类型应为 `{expected}`,实际为 `{}`",
                    declaration.id, endpoint.kind
                ),
            ));
        }
    }
}

/// 根据关系类型与实例构建机器可读的关系索引（供未来快照 DTO 复用）。
fn relation_index(catalog: &Catalog) -> BTreeMap<TargetRef, Vec<String>> {
    let mut index = BTreeMap::new();
    for relation in catalog.relations.values() {
        index
            .entry(relation.from_ref.clone())
            .or_insert_with(Vec::new)
            .push(relation.id.clone());
        index
            .entry(relation.to_ref.clone())
            .or_insert_with(Vec::new)
            .push(relation.id.clone());
    }
    for ids in index.values_mut() {
        ids.sort();
    }
    index
}

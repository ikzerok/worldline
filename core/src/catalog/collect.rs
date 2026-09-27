use super::{
    resolved_asset, supported_extension, AssetInfo, Catalog, CatalogDecl, CatalogLink, EntityInfo,
    ReferenceInfo, TagInfo, TargetRef,
};
use crate::ast::{PropertyValue, Stmt};
use crate::{Diagnostic, Program, RelationGraph, Span, Symbols};
use std::collections::BTreeMap;
use std::path::Path;
pub(crate) fn analyze(
    program: &Program,
    symbols: &Symbols,
    graph: &RelationGraph,
    diags: &mut Vec<Diagnostic>,
) -> Catalog {
    let mut catalog = Catalog::default();
    for node in &graph.nodes {
        catalog.add_object(
            if node.is_event { "event" } else { "scene" },
            &node.name,
            node.summary.as_deref().unwrap_or(&node.name),
            &node.file,
            node.line,
        );
    }
    for (id, c) in &symbols.characters {
        catalog.add_object("character", id, &c.display, &c.decl_file, c.decl_span.line);
    }
    for entity in &program.entities {
        if let Some(old) = catalog.entities.get(&entity.name) {
            diags.push(
                Diagnostic::error(
                    "A104",
                    &entity.file,
                    Span::new(
                        entity.loc.line,
                        entity.loc.column,
                        entity.name.chars().count() as u32,
                    ),
                    format!("实体 `{}` 重复定义", entity.name),
                )
                .with_related(
                    &old.file,
                    Span::new(
                        old.line,
                        entity.loc.column,
                        entity.name.chars().count() as u32,
                    ),
                ),
            );
            continue;
        }
        let display = entity
            .display
            .clone()
            .unwrap_or_else(|| entity.name.clone());
        catalog.add_object(
            "entity",
            &entity.name,
            &display,
            &entity.file,
            entity.loc.line,
        );
        catalog.entities.insert(
            entity.name.clone(),
            EntityInfo {
                id: entity.name.clone(),
                entity_type: entity.entity_type.clone(),
                display,
                description: entity.description.clone(),
                properties: crate::analysis_metadata::properties(
                    &entity.properties,
                    &entity.file,
                    diags,
                ),
                file: entity.file.clone(),
                line: entity.loc.line,
            },
        );
    }
    for w in &program.worlds {
        catalog.add_object(
            "world",
            &w.name,
            w.display.as_deref().unwrap_or(&w.name),
            &w.file,
            w.loc.line,
        );
    }
    for p in &program.periods {
        catalog.add_object(
            "period",
            &p.name,
            p.display.as_deref().unwrap_or(&p.name),
            &p.file,
            p.loc.line,
        );
    }
    for (id, s) in &symbols.storylines {
        let node = graph.nodes.iter().find(|n| &n.storyline == id);
        catalog.add_object(
            "storyline",
            id,
            &s.display,
            node.map(|n| n.file.as_str()).unwrap_or(""),
            node.map(|n| n.line).unwrap_or(1),
        );
    }
    for (id, v) in &symbols.vars {
        catalog.add_object("variable", id, id, &v.decl_file, v.decl_span.line);
    }
    for file in &program.files {
        let id = crate::compiler::source_path(Path::new(file))
            .to_string_lossy()
            .into_owned();
        catalog.add_object(
            "file",
            &id,
            Path::new(file)
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or(file),
            file,
            1,
        );
    }
    for decl in &program.catalog {
        match decl {
            CatalogDecl::Tag(tag) => {
                if let Some(old) = catalog.tags.get(&tag.name) {
                    diags.push(
                        Diagnostic::error(
                            "A104",
                            &tag.file,
                            Span::new(tag.loc.line, 1, 3),
                            format!("标签 `{}` 重复定义", tag.name),
                        )
                        .with_related(&old.file, Span::new(old.line, 1, 3)),
                    );
                    continue;
                }
                let properties =
                    crate::analysis_metadata::properties(&tag.properties, &tag.file, diags);
                let display = tag.display.clone().unwrap_or_else(|| tag.name.clone());
                catalog.add_object("tag", &tag.name, &display, &tag.file, tag.loc.line);
                catalog.tags.insert(
                    tag.name.clone(),
                    TagInfo {
                        id: tag.name.clone(),
                        display,
                        description: tag.description.clone(),
                        properties,
                        file: tag.file.clone(),
                        line: tag.loc.line,
                        declared: true,
                    },
                );
            }
            CatalogDecl::Asset(asset) => {
                if let Some(old) = catalog.assets.get(&asset.id) {
                    diags.push(
                        Diagnostic::error(
                            "A104",
                            &asset.file,
                            Span::new(asset.loc.line, 1, 5),
                            format!("素材 `{}` 重复定义", asset.id),
                        )
                        .with_related(&old.file, Span::new(old.line, 1, 5)),
                    );
                    continue;
                }
                let path = resolved_asset(&asset.file, &asset.path);
                let root = crate::compiler::source_path(Path::new(
                    program.files.first().unwrap_or(&asset.file),
                ))
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
                let inside = !Path::new(&asset.path).is_absolute() && path.starts_with(&root);
                if !inside {
                    diags.push(Diagnostic::error(
                        "A109",
                        &asset.file,
                        Span::new(asset.loc.line, 1, 5),
                        "附件必须使用工作区内相对路径",
                    ));
                }
                let available = inside
                    && crate::file_access::readable(&path)
                    && supported_extension(&asset.kind, &path);
                if !available {
                    diags.push(Diagnostic::warning(
                        "A215",
                        &asset.file,
                        Span::new(asset.loc.line, 1, 5),
                        format!(
                            "素材 `{}` 缺失、不可读或格式不匹配:{}",
                            asset.id,
                            path.display()
                        ),
                    ));
                }
                catalog.add_object(
                    "asset",
                    &asset.id,
                    &asset.display,
                    &asset.file,
                    asset.loc.line,
                );
                catalog.assets.insert(
                    asset.id.clone(),
                    AssetInfo {
                        id: asset.id.clone(),
                        kind: asset.kind.clone(),
                        path: asset.path.clone(),
                        resolved_path: path.to_string_lossy().into_owned(),
                        display: asset.display.clone(),
                        file: asset.file.clone(),
                        line: asset.loc.line,
                        available,
                    },
                );
            }
            _ => {}
        }
    }
    crate::anchors::collect_declarations(program, &mut catalog, diags);
    crate::states::collect_declarations(program, &mut catalog, diags);
    // 关系对象必须先于 mark/attach、正文链接和地图引用进入统一目录；否则
    // 这些消费者会把合法的 relation TargetRef 误报为未知对象。关系端点和
    // scope 也可能指向 anchor/state，因此要在它们的声明进入目录后收集。
    crate::relations::collect(program, &mut catalog, diags);
    crate::anchors::collect_links(program, &mut catalog, diags);
    for decl in &program.catalog {
        let (link, attach) = match decl {
            CatalogDecl::Mark(m) => (m, false),
            CatalogDecl::Attach(a) => (a, true),
            _ => continue,
        };
        let mut link = link.clone();
        if link.target.kind == "file" {
            link.target.id = resolved_asset(&link.file, &link.target.id)
                .to_string_lossy()
                .into_owned();
        }
        if catalog.object(&link.target).is_none() {
            diags.push(Diagnostic::error(
                "A214",
                &link.file,
                Span::new(link.line, 1, 4),
                format!("引用对象 {} {} 不存在", link.target.kind, link.target.id),
            ));
        }
        link.values.sort();
        link.values.dedup();
        for value in &link.values {
            if if attach {
                !catalog.assets.contains_key(value)
            } else {
                !catalog.tags.contains_key(value)
            } {
                diags.push(Diagnostic::error(
                    "A214",
                    &link.file,
                    Span::new(link.line, 1, 4),
                    format!(
                        "引用的{} `{value}` 未定义",
                        if attach { "素材" } else { "标签" }
                    ),
                ));
            }
        }
        if attach {
            catalog.attachments.push(link);
        } else {
            catalog.marks.push(link);
        }
    }
    for (event, file) in program.events.iter().zip(&program.event_files) {
        collect_inline(
            &event.body,
            file,
            &TargetRef::new("event", &event.name),
            &mut catalog,
        );
    }
    for (links, attach) in [(&catalog.marks, false), (&catalog.attachments, true)] {
        for link in links {
            for id in &link.values {
                let (source, target, kind) = if attach {
                    (link.target.clone(), TargetRef::new("asset", id), "素材引用")
                } else {
                    (TargetRef::new("tag", id), link.target.clone(), "标签引用")
                };
                catalog.references.push(ReferenceInfo {
                    source,
                    target,
                    kind: kind.into(),
                    file: link.file.clone(),
                    line: link.line,
                });
            }
        }
    }
    for edge in &graph.edges {
        let source = &graph.nodes[edge.from as usize];
        let target = &graph.nodes[edge.to as usize];
        catalog.references.push(ReferenceInfo {
            source: TargetRef::new(
                if source.is_event { "event" } else { "scene" },
                &source.name,
            ),
            target: TargetRef::new(
                if target.is_event { "event" } else { "scene" },
                &target.name,
            ),
            kind: "叙事连接".into(),
            file: edge.file.clone(),
            line: edge.line,
        });
    }
    for (event, file) in program.events.iter().zip(&program.event_files) {
        for predecessor in &event.predecessors {
            catalog.references.push(ReferenceInfo {
                source: TargetRef::new("event", &event.name),
                target: TargetRef::new("event", predecessor),
                kind: "先后约束".into(),
                file: file.clone(),
                line: event.loc.line,
            });
        }
        for (id, character) in &symbols.characters {
            if character.events.contains(&event.name) {
                catalog.references.push(ReferenceInfo {
                    source: TargetRef::new("event", &event.name),
                    target: TargetRef::new("character", id),
                    kind: "关联人物".into(),
                    file: file.clone(),
                    line: event.loc.line,
                });
            }
        }
    }
    for (id, character) in &symbols.characters {
        for relation in &character.relations {
            catalog.references.push(ReferenceInfo {
                source: TargetRef::new("character", id),
                target: TargetRef::new("character", &relation.target),
                kind: relation.label.clone(),
                file: relation.file.clone(),
                line: relation.line,
            });
        }
    }
    if let Some(world) = program.worlds.first() {
        collect_property_references(
            &world.properties,
            &TargetRef::new("world", &world.name),
            &world.file,
            &mut catalog,
            diags,
        );
    }
    for character in &program.characters {
        collect_property_references(
            &character.properties,
            &TargetRef::new("character", &character.name),
            &character.file,
            &mut catalog,
            diags,
        );
    }
    for entity in &program.entities {
        collect_property_references(
            &entity.properties,
            &TargetRef::new("entity", &entity.name),
            &entity.file,
            &mut catalog,
            diags,
        );
    }
    for declaration in &program.catalog {
        match declaration {
            CatalogDecl::Tag(tag) => collect_property_references(
                &tag.properties,
                &TargetRef::new("tag", &tag.name),
                &tag.file,
                &mut catalog,
                diags,
            ),
            CatalogDecl::Anchor(anchor) => collect_property_references(
                &anchor.properties,
                &TargetRef::new("anchor", &anchor.name),
                &anchor.file,
                &mut catalog,
                diags,
            ),
            _ => {}
        }
    }
    for relation in &program.relations {
        collect_property_references(
            &relation.properties,
            &TargetRef::new("relation", &relation.id),
            &relation.file,
            &mut catalog,
            diags,
        );
    }
    crate::states::collect_changes(program, &mut catalog, diags);
    crate::navigation::collect(program, &mut catalog, diags);
    catalog.objects.sort_by(|a, b| a.target.cmp(&b.target));
    catalog
}

fn collect_property_references(
    properties: &[crate::ast::Property],
    source: &TargetRef,
    file: &str,
    catalog: &mut Catalog,
    diags: &mut Vec<Diagnostic>,
) {
    for property in properties {
        let PropertyValue::Ref(target) = &property.value else {
            continue;
        };
        if catalog.object(target).is_none() {
            diags.push(Diagnostic::error(
                "A214",
                file,
                Span::new(property.loc.line, property.loc.column, 8),
                format!(
                    "属性 `{}` 引用的对象 {} `{}` 不存在",
                    property.name, target.kind, target.id
                ),
            ));
        }
        catalog.references.push(ReferenceInfo {
            source: source.clone(),
            target: target.clone(),
            kind: "对象属性引用".into(),
            file: file.into(),
            line: property.loc.line,
        });
    }
}

fn collect_inline(body: &[Stmt], file: &str, owner: &TargetRef, catalog: &mut Catalog) {
    for stmt in body {
        match stmt {
            Stmt::Text(text) if !text.tags.is_empty() => {
                for tag in &text.tags {
                    if !catalog.tags.contains_key(tag) {
                        catalog.add_object("tag", tag, tag, file, text.loc.line);
                        catalog.tags.insert(
                            tag.clone(),
                            TagInfo {
                                id: tag.clone(),
                                display: tag.clone(),
                                description: String::new(),
                                properties: BTreeMap::new(),
                                file: file.into(),
                                line: text.loc.line,
                                declared: false,
                            },
                        );
                    }
                }
                catalog.marks.push(CatalogLink {
                    target: owner.clone(),
                    values: text.tags.clone(),
                    file: file.into(),
                    line: text.loc.line,
                    inline: true,
                });
            }
            Stmt::Choice(c) => collect_inline(&c.body, file, owner, catalog),
            Stmt::Scene(s) => collect_inline(
                &s.body,
                file,
                &TargetRef::new("scene", &format!("{}.{}", owner.id, s.name)),
                catalog,
            ),
            Stmt::If(i) => {
                for (_, body) in &i.branches {
                    collect_inline(body, file, owner, catalog);
                }
            }
            _ => {}
        }
    }
}

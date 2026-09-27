use super::*;

impl<'a> Parser<'a> {
    /// 顶层解析。include 已在驱动层展开;入口 = 主文件第一个事件。
    pub fn parse_program(&mut self) -> Program {
        let mut program = Program::default();
        let main_file = self
            .lines
            .first()
            .map(|l| l.file.clone())
            .unwrap_or_default();
        while let Some(line) = self.peek() {
            let file = self.file_of(line);
            if line.indent != 0 {
                self.diags.push(Diagnostic::error(
                    "P002",
                    &file,
                    Span::new(line.no, 1, 1),
                    "顶层声明不能缩进",
                ));
            }
            match &line.kind {
                LineKind::Catalog(item) => {
                    let mut item = item.clone();
                    self.next();
                    if let crate::catalog::CatalogDecl::Tag(tag) = &mut item {
                        let (properties, _, description) =
                            self.parse_metadata(line.indent, &file, MetadataKind::World);
                        tag.properties = properties;
                        tag.description = description;
                    }
                    if let crate::catalog::CatalogDecl::Anchor(anchor) = &mut item {
                        let (properties, _, description) =
                            self.parse_metadata(line.indent, &file, MetadataKind::World);
                        for property in properties {
                            self.diags.push(Diagnostic::error(
                                "P002",
                                &file,
                                Span::new(property.loc.line, 1, 8),
                                "独立锚点块内只允许一个 description",
                            ));
                        }
                        anchor.description = description;
                    }
                    program.catalog.push(item);
                }
                LineKind::Let {
                    name,
                    expr_src,
                    loc,
                    name_span,
                } => {
                    self.next();
                    let expr = parse_expr_src(
                        expr_src,
                        &file,
                        line.no,
                        name_span.column + name.chars().count() as u32 + 1,
                        self.diags,
                    );
                    program.lets.push(LetStmt {
                        name: name.clone(),
                        expr,
                        is_const: false,
                        loc: *loc,
                        file: file.clone(),
                    });
                }
                LineKind::Const {
                    name,
                    expr_src,
                    loc,
                    name_span,
                } => {
                    self.next();
                    let expr = parse_expr_src(
                        expr_src,
                        &file,
                        line.no,
                        name_span.column + name.chars().count() as u32 + 1,
                        self.diags,
                    );
                    program.lets.push(LetStmt {
                        name: name.clone(),
                        expr,
                        is_const: true,
                        loc: *loc,
                        file: file.clone(),
                    });
                }
                LineKind::Storyline { name, display, loc } => {
                    let (name, display) = (name.clone(), display.clone());
                    let espan = *loc;
                    let s_indent = line.indent;
                    self.next();
                    let mut nested = false;
                    if !name.is_empty() {
                        program.storylines.push(StorylineDecl {
                            name: name.clone(),
                            display,
                            loc: Loc::new(espan.line, espan.column),
                        });
                    }
                    let prev = self.cur_storyline.replace(if name.is_empty() {
                        "main".into()
                    } else {
                        name
                    });
                    if prev.is_some() {
                        nested = true;
                    }
                    if nested {
                        self.diags.push(Diagnostic::error(
                            "P002",
                            &file,
                            espan,
                            "storyline 不能嵌套在另一个 storyline 块内",
                        ));
                    }
                    while let Some(nl) = self.peek().cloned() {
                        if nl.indent <= s_indent {
                            break;
                        }
                        match &nl.kind {
                            LineKind::Event { .. } => {
                                self.parse_event_decl(&mut program, &main_file)
                            }
                            LineKind::Let { .. } | LineKind::Const { .. } => {
                                self.parse_global_decl(&mut program);
                            }
                            LineKind::Character { loc: cloc, .. } => {
                                self.diags.push(Diagnostic::error(
                                    "P002",
                                    &file,
                                    *cloc,
                                    "character 只能出现在顶层,不能写入 storyline 块",
                                ));
                                self.next();
                            }
                            LineKind::Entity { loc: eloc, .. } => {
                                self.diags.push(Diagnostic::error(
                                    "P002",
                                    &file,
                                    *eloc,
                                    "entity 只能出现在顶层,不能写入 storyline 块",
                                ));
                                self.next();
                            }
                            LineKind::Storyline { loc: sloc, .. } => {
                                self.diags.push(Diagnostic::error(
                                    "P002",
                                    &file,
                                    *sloc,
                                    "storyline 不能嵌套在另一个 storyline 块内",
                                ));
                                self.next();
                            }
                            _ => {
                                self.diags.push(Diagnostic::error(
                                    "P002",
                                    &file,
                                    Span::new(nl.no, nl.indent + 1, 4),
                                    "storyline 块内只能出现 event(或 let / const)",
                                ));
                                self.next();
                            }
                        }
                    }
                    self.cur_storyline = prev;
                }
                LineKind::Period {
                    name,
                    display,
                    parent,
                    loc,
                } => {
                    program.periods.push(PeriodDecl {
                        parent: parent.clone(),
                        name: name.clone(),
                        display: display.clone(),
                        file,
                        loc: Loc::new(loc.line, loc.column),
                    });
                    self.next();
                }
                LineKind::World { name, display, loc } => {
                    let (name, display, loc, indent) =
                        (name.clone(), display.clone(), *loc, line.indent);
                    self.next();
                    let (properties, _, description) =
                        self.parse_metadata(indent, &file, MetadataKind::World);
                    program.worlds.push(WorldDecl {
                        name,
                        display,
                        description,
                        properties,
                        file,
                        loc: Loc::new(loc.line, loc.column),
                    });
                }
                LineKind::Character { name, display, loc } => {
                    let (name, display) = (name.clone(), display.clone());
                    let espan = *loc;
                    self.next();
                    let (properties, relations, _) =
                        self.parse_metadata(line.indent, &file, MetadataKind::Character);
                    program.characters.push(CharacterDecl {
                        name,
                        display,
                        loc: Loc::new(espan.line, espan.column),
                        file: file.clone(),
                        properties,
                        relations,
                    });
                }
                LineKind::Entity {
                    name,
                    entity_type,
                    display,
                    loc,
                } => {
                    let (name, entity_type, display, loc, indent) = (
                        name.clone(),
                        entity_type.clone(),
                        display.clone(),
                        *loc,
                        line.indent,
                    );
                    self.next();
                    let (properties, _, description) =
                        self.parse_metadata(indent, &file, MetadataKind::Entity);
                    program.entities.push(EntityDecl {
                        name,
                        entity_type,
                        display,
                        description,
                        properties,
                        file,
                        loc: Loc::new(loc.line, loc.column),
                    });
                }
                LineKind::RelationType { name, display, loc } => {
                    let (name, display, loc, indent) =
                        (name.clone(), display.clone(), *loc, line.indent);
                    self.next();
                    let mut inverse_display = None;
                    let mut direction = crate::relations::RelationDirection::Directed;
                    let mut from_kind = None;
                    let mut to_kind = None;
                    self.parse_relation_type_block(
                        indent,
                        &file,
                        &mut inverse_display,
                        &mut direction,
                        &mut from_kind,
                        &mut to_kind,
                    );
                    program.relation_types.push(RelationTypeDecl {
                        name,
                        display,
                        inverse_display,
                        direction,
                        from_kind,
                        to_kind,
                        file,
                        loc: Loc::new(loc.line, loc.column),
                    });
                }
                LineKind::RelationDef {
                    id,
                    relation_type,
                    from_kind,
                    from_id,
                    to_kind,
                    to_id,
                    loc,
                } => {
                    let (id, relation_type, from_kind, from_id, to_kind, to_id, loc, indent) = (
                        id.clone(),
                        relation_type.clone(),
                        from_kind.clone(),
                        from_id.clone(),
                        to_kind.clone(),
                        to_id.clone(),
                        *loc,
                        line.indent,
                    );
                    self.next();
                    let (description, source_note, scope_refs, properties) =
                        self.parse_relation_def_block(indent, &file);
                    program.relations.push(RelationDef {
                        id,
                        relation_type,
                        from: relation_target(&file, &from_kind, &from_id),
                        to: relation_target(&file, &to_kind, &to_id),
                        description,
                        source_note,
                        scope_refs,
                        properties,
                        file,
                        loc: Loc::new(loc.line, loc.column),
                    });
                }
                LineKind::Event { .. } => self.parse_event_decl(&mut program, &main_file),
                LineKind::Scene { loc, .. } => {
                    self.diags.push(Diagnostic::error(
                        "P005",
                        &file,
                        *loc,
                        "scene 必须声明在事件(或场景)块内部",
                    ));
                    self.next();
                }
                LineKind::Else { loc } | LineKind::ElseIf { loc, .. } => {
                    self.diags.push(Diagnostic::error(
                        "P002",
                        &file,
                        Span::new(loc.line, loc.column, 4),
                        "else 不能出现在事件块外",
                    ));
                    self.next();
                }
                _ => {
                    self.diags.push(Diagnostic::error(
                        "P002",
                        &file,
                        Span::new(line.no, line.indent + 1, 5),
                        "顶层只能是 event / storyline / character / entity / relation_type / relation_def / let / const / include;文本与选择必须写在事件块内",
                    ));
                    self.next();
                }
            }
        }
        program
    }

    /// 解析顶层/故事线块内的 let/const(下一行)。
    fn parse_global_decl(&mut self, program: &mut Program) {
        let Some(line) = self.next() else { return };
        let file = line.file.clone();
        match &line.kind {
            LineKind::Let {
                name,
                expr_src,
                loc,
                name_span,
            } => {
                let expr = parse_expr_src(
                    expr_src,
                    &file,
                    line.no,
                    name_span.column + name.chars().count() as u32 + 1,
                    self.diags,
                );
                program.lets.push(LetStmt {
                    name: name.clone(),
                    expr,
                    is_const: false,
                    loc: *loc,
                    file,
                });
            }
            LineKind::Const {
                name,
                expr_src,
                loc,
                name_span,
            } => {
                let expr = parse_expr_src(
                    expr_src,
                    &file,
                    line.no,
                    name_span.column + name.chars().count() as u32 + 1,
                    self.diags,
                );
                program.lets.push(LetStmt {
                    name: name.clone(),
                    expr,
                    is_const: true,
                    loc: *loc,
                    file,
                });
            }
            _ => {}
        }
    }

    /// 解析一个事件声明(下一行为 event 头)。
    fn parse_event_decl(&mut self, program: &mut Program, main_file: &str) {
        let Some(line) = self.next() else { return };
        let file = line.file.clone();
        let (
            name,
            summary,
            order,
            period,
            predecessors,
            characters,
            perm,
            after_src,
            espan,
            indent,
        ) = match &line.kind {
            LineKind::Event {
                name,
                summary,
                order,
                period,
                predecessors,
                characters,
                perm,
                after_src,
                loc,
            } => (
                name.clone(),
                summary.clone(),
                *order,
                period.clone(),
                predecessors.clone(),
                characters.clone(),
                perm.clone(),
                after_src.clone(),
                *loc,
                line.indent,
            ),
            _ => return,
        };
        let after = after_src.map(|src| {
            parse_expr_src(
                &src,
                &file,
                line.no,
                espan.column + name.chars().count() as u32 + 1,
                self.diags,
            )
        });
        let body = self.parse_block(indent, &file, true);
        let (effects, body) = extract_effects(body, &file, self.diags);
        if program.entry.is_empty() && file == main_file {
            program.entry = name.clone();
        }
        let storyline = self
            .cur_storyline
            .clone()
            .unwrap_or_else(|| "main".to_string());
        program.events.push(Event {
            name,
            body,
            loc: Loc::new(espan.line, espan.column),
            summary,
            order,
            period,
            predecessors,
            characters,
            perm,
            after,
            storyline,
            effects,
        });
        program.event_files.push(file);
    }
}

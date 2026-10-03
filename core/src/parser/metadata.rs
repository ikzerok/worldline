use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_metadata(
        &mut self,
        indent: u32,
        file: &str,
        kind: MetadataKind,
    ) -> (Vec<Property>, Vec<CharacterRelation>, String) {
        let mut properties = Vec::new();
        let mut relations = Vec::new();
        let mut description = String::new();
        let mut has_description = false;
        let mut block_indent = None;
        while let Some(line) = self.peek() {
            if line.indent <= indent || line.file != file {
                break;
            }
            self.next();
            if *block_indent.get_or_insert(line.indent) != line.indent {
                self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    "属性块内缩进必须一致",
                ));
            }
            match line.kind {
                LineKind::Property {
                    name,
                    value_src,
                    loc,
                } => {
                    let expr = self.sourced_expr(
                        &value_src,
                        file,
                        line.no,
                        line.no,
                        line.source.base + line.source.value,
                        ExpressionSlot::Value,
                    );
                    match parse_property_value(expr, file, self.options()) {
                        Ok(value) => {
                            properties.push(Property { name, value, loc });
                        }
                        Err(message) => {
                            self.diags.push(Diagnostic::error(
                                "P004",
                                file,
                                Span::new(line.no, 1, 8),
                                message,
                            ));
                        }
                    }
                }
                LineKind::Relation { target, label, loc } if kind == MetadataKind::Character => {
                    relations.push(CharacterRelation { target, label, loc })
                }
                LineKind::Description { text, loc }
                    if kind != MetadataKind::Character && !has_description =>
                {
                    let _ = loc;
                    description = text;
                    has_description = true;
                }
                _ => self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    match kind {
                        MetadataKind::World => "世界观块内只允许一个 description 和 property",
                        MetadataKind::Entity => "实体块内只允许一个 description 和 property",
                        MetadataKind::Character => "角色块内只允许 property 和 relation",
                    },
                )),
            }
        }
        (properties, relations, description)
    }

    pub(super) fn parse_relation_type_block(
        &mut self,
        indent: u32,
        file: &str,
        inverse_display: &mut Option<String>,
        direction: &mut crate::relations::RelationDirection,
        from_kind: &mut Option<String>,
        to_kind: &mut Option<String>,
    ) {
        let mut block_indent = None;
        let mut seen_fields = std::collections::HashSet::new();
        while let Some(line) = self.peek() {
            if line.indent <= indent || line.file != file {
                break;
            }
            self.next();
            if *block_indent.get_or_insert(line.indent) != line.indent {
                self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    "关系类型块内缩进必须一致",
                ));
            }
            if let LineKind::RelationField { name, loc, .. } = &line.kind {
                let canonical = match name.as_str() {
                    "from_kind" => "from",
                    "to_kind" => "to",
                    other => other,
                };
                if !seen_fields.insert(canonical.to_string()) {
                    self.diags.push(Diagnostic::error(
                        "A220",
                        file,
                        Span::new(loc.line, loc.column, name.len() as u32),
                        format!("关系类型不能重复声明 {canonical}"),
                    ));
                    continue;
                }
            }
            match line.kind {
                LineKind::RelationField { name, value, loc } => match name.as_str() {
                    "inverse" => *inverse_display = Some(value),
                    "direction" => match value.as_str() {
                        "directed" => *direction = crate::relations::RelationDirection::Directed,
                        "undirected" => {
                            *direction = crate::relations::RelationDirection::Undirected
                        }
                        _ => self.diags.push(Diagnostic::error(
                            "P004",
                            file,
                            Span::new(loc.line, loc.column, 9),
                            "关系类型 direction 只能是 directed 或 undirected",
                        )),
                    },
                    "from" | "from_kind" => {
                        let mut tokens = value.split_whitespace();
                        let kind = tokens.next().unwrap_or("");
                        if kind.is_empty() || tokens.next().is_some() {
                            self.diags.push(Diagnostic::error(
                                "P004",
                                file,
                                Span::new(loc.line, loc.column, 4),
                                "关系类型 from 后必须且只能有一个端点类型",
                            ));
                        } else {
                            *from_kind = Some(kind.into());
                        }
                    }
                    "to" | "to_kind" => {
                        let mut tokens = value.split_whitespace();
                        let kind = tokens.next().unwrap_or("");
                        if kind.is_empty() || tokens.next().is_some() {
                            self.diags.push(Diagnostic::error(
                                "P004",
                                file,
                                Span::new(loc.line, loc.column, 2),
                                "关系类型 to 后必须且只能有一个端点类型",
                            ));
                        } else {
                            *to_kind = Some(kind.into());
                        }
                    }
                    _ => self.diags.push(Diagnostic::error(
                        "P002",
                        file,
                        Span::new(loc.line, loc.column, name.chars().count() as u32),
                        "关系类型块内只允许 inverse、direction、from 和 to",
                    )),
                },
                _ => self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    "关系类型块内只允许 inverse、direction、from 和 to",
                )),
            }
        }
    }

    pub(super) fn parse_relation_def_block(
        &mut self,
        indent: u32,
        file: &str,
    ) -> (
        String,
        Option<String>,
        Vec<crate::catalog::TargetRef>,
        Vec<Property>,
    ) {
        let mut description = String::new();
        let mut source_note = None;
        let mut scope_refs = Vec::new();
        let mut properties = Vec::new();
        let mut block_indent = None;
        let mut has_description = false;
        while let Some(line) = self.peek() {
            if line.indent <= indent || line.file != file {
                break;
            }
            self.next();
            if *block_indent.get_or_insert(line.indent) != line.indent {
                self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    "关系定义块内缩进必须一致",
                ));
            }
            match line.kind {
                LineKind::Description { text, loc } => {
                    if has_description {
                        self.diags.push(Diagnostic::error(
                            "A220",
                            file,
                            Span::new(loc.line, loc.column, 11),
                            "关系定义只能有一个 description",
                        ));
                    } else {
                        description = text;
                        has_description = true;
                    }
                }
                LineKind::Property {
                    name,
                    value_src,
                    loc,
                } => {
                    let expr = self.sourced_expr(
                        &value_src,
                        file,
                        line.no,
                        line.no,
                        line.source.base + line.source.value,
                        ExpressionSlot::Value,
                    );
                    match parse_property_value(expr, file, self.options()) {
                        Ok(value) => {
                            if properties
                                .iter()
                                .any(|property: &Property| property.name == name)
                            {
                                self.diags.push(Diagnostic::error(
                                    "A220",
                                    file,
                                    Span::new(loc.line, 1, 8),
                                    format!("关系属性 `{name}` 重复定义"),
                                ));
                            } else {
                                properties.push(Property { name, value, loc });
                            }
                        }
                        Err(message) => {
                            self.diags.push(Diagnostic::error(
                                "P004",
                                file,
                                Span::new(line.no, 1, 8),
                                message,
                            ));
                        }
                    }
                }
                LineKind::RelationField { name, value, loc } => match name.as_str() {
                    "source_note" => {
                        if source_note.replace(value).is_some() {
                            self.diags.push(Diagnostic::error(
                                "A220",
                                file,
                                Span::new(loc.line, loc.column, 11),
                                "关系定义不能重复声明 source_note",
                            ));
                        }
                    }
                    "scope" | "scope_ref" => {
                        let tokens =
                            crate::catalog_syntax::tokenize(&value, file, line.no, self.diags);
                        let kind = tokens.first().map(|token| token.0.as_str()).unwrap_or("");
                        let id = tokens.get(1).map(|token| token.0.as_str()).unwrap_or("");
                        let quoted = |index: usize| tokens.get(index).is_some_and(|token| token.1);
                        let valid_id = if kind == "file" {
                            quoted(1) && !id.is_empty()
                        } else {
                            !quoted(1) && !id.is_empty()
                        };
                        if tokens.len() != 2 || kind.is_empty() || id.is_empty() || !valid_id {
                            self.diags.push(Diagnostic::error(
                                "P004",
                                file,
                                Span::new(loc.line, loc.column, name.chars().count() as u32),
                                "scope 后需要对象类型和 ID",
                            ));
                        } else {
                            scope_refs.push(relation_target(file, kind, id));
                        }
                    }
                    _ => self.diags.push(Diagnostic::error(
                        "P002",
                        file,
                        Span::new(loc.line, loc.column, name.chars().count() as u32),
                        "关系定义块内只允许 description、source_note、scope 和 property",
                    )),
                },
                _ => self.diags.push(Diagnostic::error(
                    "P002",
                    file,
                    Span::new(line.no, 1, 1),
                    "关系定义块内只允许 description、source_note、scope 和 property",
                )),
            }
        }
        (description, source_note, scope_refs, properties)
    }
}

use super::*;

impl Project {
    pub fn write_character(
        &mut self,
        path: &Path,
        original: Option<&str>,
        draft: &CharacterDraft,
    ) -> Result<(), String> {
        identifier(&draft.id)?;
        if let Some(original) = original
            .filter(|id| *id != draft.id && self.language_version_kind().supports_language_111())
        {
            let mut candidate = self.clone();
            let plan = candidate.plan_rename_target(
                &crate::catalog::TargetRef::new("character", original),
                &draft.id,
            )?;
            candidate.apply_rename_plan(&plan)?;
            let mut renamed_draft = draft.clone();
            for (_, value) in &mut renamed_draft.properties {
                if let PropertyValue::Ref(target) = value {
                    if target.kind == "character" && target.id == original {
                        target.id = draft.id.clone();
                    }
                }
            }
            for (target, _) in &mut renamed_draft.relations {
                if target == original {
                    *target = draft.id.clone();
                }
            }
            candidate.write_character(path, Some(&draft.id), &renamed_draft)?;
            *self = candidate;
            return Ok(());
        }

        let mut out = format!(
            "character {} as {}\n{}",
            draft.id,
            quote(&draft.display),
            property_lines(&draft.properties)?
        );
        for (target, label) in &draft.relations {
            identifier(target)?;
            out.push_str(&format!("  relation {target} as {}\n", quote(label)));
        }
        self.replace_metadata(path, original, "character", &out)?;
        if let Some(original) = original.filter(|id| *id != draft.id) {
            self.rename_character_references(original, &draft.id)?;
        }
        Ok(())
    }

    pub fn write_world(&mut self, draft: &WorldDraft) -> Result<(), String> {
        identifier(&draft.id)?;
        let result = self.compile_current();
        let world = result.analysis.world;
        let path = world
            .as_ref()
            .map(|w| PathBuf::from(&w.file))
            .unwrap_or_else(|| self.entry.clone());
        let out = format!(
            "world {} as {}\n  description {}\n{}",
            draft.id,
            quote(&draft.display),
            quote(&draft.description),
            property_lines(&draft.properties)?
        );
        self.replace_metadata(&path, world.as_ref().map(|w| w.id.as_str()), "world", &out)
    }

    /// 创建或修改 1.10 实体作者资料。实体 ID 是稳定身份，修改资料时必须保留。
    pub fn write_entity(
        &mut self,
        path: &Path,
        original: Option<&str>,
        draft: &EntityDraft,
    ) -> Result<(), String> {
        if !self.language_version_kind().supports_entities() {
            return Err("entity 需要工程显式启用语言 1.10".into());
        }
        identifier(&draft.id)?;
        identifier(&draft.entity_type)?;
        if draft.display.trim().is_empty() {
            return Err("实体显示名不能为空".into());
        }
        if original.is_some_and(|id| id != draft.id) {
            return Err("实体 ID 是引用身份,修改名称和资料时请保留 ID".into());
        }
        let result = self.compile_current();
        let existing = result.analysis.catalog.entities.get(&draft.id);
        if original.is_some() && existing.is_none() {
            return Err("待修改的实体不存在".into());
        }
        if original.is_none() && existing.is_some() {
            return Err("实体 ID 已存在".into());
        }
        let path = existing
            .map(|entity| PathBuf::from(&entity.file))
            .unwrap_or_else(|| path.to_path_buf());
        let out = format!(
            "entity {} kind {} as {}\n  description {}\n{}",
            draft.id,
            draft.entity_type,
            quote(&draft.display),
            quote(&draft.description),
            property_lines(&draft.properties)?
        );
        self.replace_metadata(&path, original, "entity", &out)
    }

    /// 删除实体前重新生成影响计划，引用或地图标记未解除时拒绝写入。
    pub fn remove_entity(&mut self, id: &str) -> Result<(), String> {
        if !self.language_version_kind().supports_entities() {
            return Err("entity 需要工程显式启用语言 1.10".into());
        }
        let target = crate::catalog::TargetRef::new("entity", id);
        let impact = self.deletion_impact(&target);
        if !impact.complete {
            return Err("引用检查不完整，请先修复内容或地图诊断，再删除实体".into());
        }
        if !impact.can_delete() {
            let mut locations = impact
                .content_references
                .iter()
                .map(|reference| {
                    format!(
                        "{}:{}（{}）",
                        reference.file, reference.line, reference.kind
                    )
                })
                .collect::<Vec<_>>();
            locations.extend(
                impact
                    .map_placements
                    .iter()
                    .chain(&impact.map_scopes)
                    .map(|placement| format!("{} / {}", placement.map_id, placement.placement_id)),
            );
            locations.extend(impact.manuscripts.iter().map(|reference| {
                format!(
                    "{} / {} ({:?})",
                    reference.manuscript_id, reference.chapter_id, reference.role
                )
            }));
            return Err(format!(
                "实体 `{id}` 仍有引用，请先明确解除或重新绑定这些引用：{}",
                locations
                    .into_iter()
                    .chain(
                        impact.graph_views.iter().map(|reference| format!(
                            "{} / {}",
                            reference.view_id, reference.field
                        ))
                    )
                    .collect::<Vec<_>>()
                    .join("、")
            ));
        }
        let entity = self
            .compile_current()
            .analysis
            .catalog
            .entities
            .get(id)
            .cloned()
            .ok_or("实体不存在")?;
        let path = PathBuf::from(entity.file);
        let text = self.document(&path)?.to_string();
        let parsed = crate::lexer::lex_source_with_options(
            &path.to_string_lossy(),
            &text,
            &mut Vec::new(),
            self.compile_options(),
        );
        let index = parsed
            .iter()
            .position(|line| matches!(&line.kind, LineKind::Entity { name, .. } if name == id))
            .ok_or("实体声明源位置不存在")?;
        let block = block_at(&text, &parsed, index);
        let mut text = text;
        text.replace_range(block.range, "");
        self.set_text(&path, text)
    }

    pub(crate) fn replace_metadata(
        &mut self,
        path: &Path,
        original: Option<&str>,
        kind: &str,
        out: &str,
    ) -> Result<(), String> {
        let mut text = self.document(path)?.to_string();
        if let Some(id) = original {
            let lines = if kind == "entity" {
                crate::lexer::lex_source_with_options(
                    &path.to_string_lossy(),
                    &text,
                    &mut Vec::new(),
                    self.compile_options(),
                )
            } else {
                lines(&text, path)
            };
            let i = lines
                .iter()
                .position(|l| match &l.kind {
                    LineKind::World { name, .. } if kind == "world" => name == id,
                    LineKind::Character { name, .. } if kind == "character" => name == id,
                    LineKind::Catalog(crate::catalog::CatalogDecl::Tag(tag)) if kind == "tag" => {
                        tag.name == id
                    }
                    LineKind::Entity { name, .. } if kind == "entity" => name == id,
                    _ => false,
                })
                .ok_or("声明不存在")?;
            let block = block_at(&text, &lines, i);
            let retained = comments(&text[block.range.clone()]);
            text.replace_range(block.range, &format!("{retained}{out}\n"));
        } else {
            text = format!("{out}\n{text}");
        }
        self.set_text(path, text)
    }

    fn rename_character_references(&mut self, old: &str, new: &str) -> Result<(), String> {
        for (path, document) in &mut self.documents {
            let parsed = lines(&document.text, path);
            let mut physical: Vec<String> = document
                .text
                .split_inclusive('\n')
                .map(str::to_string)
                .collect();
            for line in parsed {
                let replacement = match line.kind {
                    LineKind::Text { content, .. } => {
                        let updated = crate::navigation::rename_links(&content, old, new);
                        (updated != content).then_some(updated)
                    }
                    LineKind::Choice {
                        label_raw,
                        once,
                        cond_src,
                        localization_id,
                        ..
                    } => {
                        let updated = crate::navigation::rename_links(&label_raw, old, new);
                        (updated != label_raw).then(|| {
                            format!(
                                "choice {}{}{}{}",
                                if once { "once " } else { "" },
                                quote(&updated),
                                cond_src.map(|c| format!(" if {c}")).unwrap_or_default(),
                                localization_id
                                    .map(|id| format!(" #wl-localization:{id}"))
                                    .unwrap_or_default()
                            )
                        })
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::Alias(alias))
                        if alias.target.kind == "character" && alias.target.id == old =>
                    {
                        Some(format!("alias character {new} as {}", quote(&alias.name)))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::AnchorLink(link))
                        if link.target.kind == "character" && link.target.id == old =>
                    {
                        Some(format!("anchor_link {} character {new}", link.anchor))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::State(state))
                        if state.target.kind == "character" && state.target.id == old =>
                    {
                        let tags = if state.tags.is_empty() {
                            "[]".into()
                        } else {
                            state.tags.join(", ")
                        };
                        Some(format!(
                            "state {} on character {new} with {tags} as {}",
                            state.id,
                            quote(&state.display)
                        ))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::Mark(mut link))
                        if link.target.kind == "character" && link.target.id == old =>
                    {
                        link.target.id = new.into();
                        Some(crate::catalog_edit::link_source(
                            &link.target,
                            &link.values,
                            false,
                        ))
                    }
                    LineKind::Catalog(crate::catalog::CatalogDecl::Attach(mut link))
                        if link.target.kind == "character" && link.target.id == old =>
                    {
                        link.target.id = new.into();
                        Some(crate::catalog_edit::link_source(
                            &link.target,
                            &link.values,
                            true,
                        ))
                    }
                    LineKind::Event {
                        name,
                        summary,
                        characters,
                        order,
                        period,
                        predecessors,
                        perm,
                        after_src,
                        ..
                    } if characters.iter().any(|id| id == old) => Some(event_header(&EventDraft {
                        id: name,
                        summary: summary.unwrap_or_default(),
                        characters: characters
                            .into_iter()
                            .map(|id| if id == old { new.into() } else { id })
                            .collect(),
                        order,
                        period,
                        predecessors,
                        perm: perm.unwrap_or_default(),
                        after: after_src.unwrap_or_default(),
                        ..Default::default()
                    })),
                    LineKind::ChangeLine { kind, id, note, .. }
                        if id == old && matches!(kind, ChangeKind::Meet | ChangeKind::Part) =>
                    {
                        Some(format!(
                            "{} {new}{}",
                            if kind == ChangeKind::Meet {
                                "meet"
                            } else {
                                "part"
                            },
                            note.map(|n| format!(" as {}", quote(&n)))
                                .unwrap_or_default()
                        ))
                    }
                    LineKind::Relation { target, label, .. } if target == old => {
                        Some(format!("relation {new} as {}", quote(&label)))
                    }
                    _ => None,
                };
                if let Some(replacement) = replacement {
                    let raw = &physical[line.no as usize - 1];
                    let cleaned = crate::lexer::strip_comments(raw);
                    let chars = cleaned.trim_end().chars().count();
                    let suffix_at = raw
                        .char_indices()
                        .nth(chars)
                        .map(|(i, _)| i)
                        .unwrap_or(raw.len());
                    let suffix = &raw[suffix_at..];
                    physical[line.no as usize - 1] =
                        format!("{}{replacement}{suffix}", " ".repeat(line.indent as usize));
                }
            }
            document.text = physical.concat();
        }
        Ok(())
    }
}

use super::*;

impl Project {
    pub fn write_period(&mut self, id: &str, display: &str) -> Result<(), String> {
        let parent = self
            .compile()
            .analysis
            .timeline
            .periods
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.parent.clone());
        self.write_period_with_parent(id, display, parent.as_deref())
    }

    pub fn write_period_with_parent(
        &mut self,
        id: &str,
        display: &str,
        parent: Option<&str>,
    ) -> Result<(), String> {
        identifier(id)?;
        if let Some(parent) = parent {
            identifier(parent)?;
        }
        let result = self.compile_current();
        let existing = result.analysis.timeline.periods.iter().find(|p| p.id == id);
        let path = existing
            .map(|p| PathBuf::from(&p.file))
            .unwrap_or_else(|| self.entry.clone());
        let mut text = self.document(&path)?.to_string();
        let out = format!(
            "period {id} as {}{}\n",
            quote(display),
            parent.map(|p| format!(" within {p}")).unwrap_or_default()
        );
        if let Some(period) = existing {
            let parsed = lines(&text, &path);
            let i = parsed
                .iter()
                .position(|l| l.no == period.line)
                .ok_or("时段声明不存在")?;
            let block = block_at(&text, &parsed, i);
            let suffix = header_comment(&text[block.range.start..block.header_end]);
            let header = format!("{}{suffix}\n", out.trim_end());
            text.replace_range(block.range.start..block.header_end, &header);
        } else {
            text = format!("{out}\n{text}");
        }
        self.set_text(&path, text)
    }

    pub fn order_events(&mut self, before: &str, after: &str) -> Result<(), String> {
        let (path, mut draft) = self.event_draft(after)?;
        if !draft.predecessors.iter().any(|id| id == before) {
            draft.predecessors.push(before.into());
        }
        self.write_event(&path, Some(after), &draft)
    }

    /// 修改先在副本中完成;任何语法或引用错误都不提交到当前文档。
    pub fn edit(
        &mut self,
        operation: impl FnOnce(&mut Project) -> Result<(), String>,
    ) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        let mut candidate = self.clone();
        operation(&mut candidate)?;
        let result = candidate.compile();
        if let Some(d) = result
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
        {
            return Err(format!(
                "{}:{} {} {}",
                d.file.rsplit(['/', '\\']).next().unwrap_or(&d.file),
                d.span.line,
                d.code,
                d.message
            ));
        }
        *self = candidate;
        Ok(())
    }

    pub fn event_draft(&self, id: &str) -> Result<(PathBuf, EventDraft), String> {
        let result = self.compile_current();
        self.event_draft_from(id, &result)
    }

    /// 一次编译取得全工程事件内容，供只读正文概览使用。
    pub fn event_drafts(&self) -> Vec<(PathBuf, EventDraft)> {
        let result = self.compile_current();
        let mut nodes: Vec<_> = result
            .analysis
            .graph
            .nodes
            .iter()
            .filter(|n| n.is_event)
            .collect();
        nodes.sort_by_key(|node| {
            (
                result
                    .analysis
                    .symbols
                    .storyline_order
                    .iter()
                    .position(|s| s == &node.storyline),
                node.seq,
                node.name.clone(),
            )
        });
        nodes
            .iter()
            .filter_map(|node| self.event_draft_from(&node.name, &result).ok())
            .collect()
    }

    fn event_draft_from(
        &self,
        id: &str,
        result: &crate::CompileResult,
    ) -> Result<(PathBuf, EventDraft), String> {
        let index = result.program.event_index(id).ok_or("事件不存在")?;
        let event = &result.program.events[index];
        let path = PathBuf::from(&result.program.event_files[index]);
        let text = self.document(&path)?;
        let lines = lines(text, &path);
        let i = lines
            .iter()
            .position(|l| matches!(&l.kind, LineKind::Event { name, .. } if name == id))
            .ok_or("事件源位置不存在")?;
        let block = block_at(text, &lines, i);
        let padding = " ".repeat(block.body_indent);
        let mut body = String::new();
        let mut effects = Vec::new();
        let mut cursor = block.header_end;
        for (j, line) in lines.iter().enumerate().skip(i + 1) {
            if line.indent as usize <= block.indent {
                break;
            }
            if line.indent as usize != block.body_indent {
                continue;
            }
            let LineKind::Effect {
                when_src, cond_src, ..
            } = &line.kind
            else {
                continue;
            };
            let when = match when_src.as_str() {
                "enter" => EffectWhen::Enter,
                "done" => EffectWhen::Done,
                "exit" => EffectWhen::Exit,
                _ => return Err("效果时机须为 enter、done 或 exit".into()),
            };
            let effect = block_at(text, &lines, j);
            body.push_str(&text[cursor..effect.range.start]);
            // 头部注释放入动作区,条件保持词法层给出的原表达式,不从 AST 反推源码。
            let mut actions = comments(&text[effect.range.start..effect.header_end]);
            actions.push_str(
                &text[effect.header_end..effect.range.end]
                    .lines()
                    // 独立注释允许少于动作的缩进,只移除实际存在的空格。
                    .map(|l| {
                        let spaces = l.bytes().take_while(|b| *b == b' ').count();
                        &l[spaces.min(effect.body_indent)..]
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            effects.push(EffectDraft {
                when,
                condition: cond_src.clone().unwrap_or_default(),
                actions: actions.trim_end().into(),
            });
            cursor = effect.range.end;
        }
        body.push_str(&text[cursor..block.range.end]);
        let body = body
            .lines()
            .map(|l| l.strip_prefix(&padding).unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string();
        let (perm, after) = match &lines[i].kind {
            LineKind::Event {
                perm, after_src, ..
            } => (
                perm.clone().unwrap_or_default(),
                after_src.clone().unwrap_or_default(),
            ),
            _ => (String::new(), String::new()),
        };
        Ok((
            path,
            EventDraft {
                id: id.into(),
                summary: event.summary.clone().unwrap_or_default(),
                storyline: event.storyline.clone(),
                characters: event.characters.clone(),
                order: event.order,
                period: event.period.clone(),
                predecessors: event.predecessors.clone(),
                perm,
                after,
                effects,
                body,
            },
        ))
    }

    /// 完整候选工程通过编译后才一次提交，包含已有后继的时间关系。
    pub fn write_event(
        &mut self,
        path: &Path,
        original: Option<&str>,
        draft: &EventDraft,
    ) -> Result<(), String> {
        let result = self.compile_current();
        self.validate_event_edit(path, original, draft, &result)?;
        self.edit(|candidate| candidate.replace_event_source(path, original, draft))
    }

    fn replace_event_source(
        &mut self,
        path: &Path,
        original: Option<&str>,
        draft: &EventDraft,
    ) -> Result<(), String> {
        qualified(&draft.id)?;
        identifier(&draft.storyline)?;
        if let Some(original) = original {
            if original != draft.id {
                return Err("事件 ID 是稳定引用,修改事件内容时请保留 ID".into());
            }
        }
        let text = self.document(path)?.to_string();
        let lines = lines(&text, path);
        let block = original
            .and_then(|id| {
                lines
                    .iter()
                    .position(|l| matches!(&l.kind, LineKind::Event { name, .. } if name == id))
            })
            .map(|i| block_at(&text, &lines, i));
        if original.is_some() && block.is_none() {
            return Err("事件不存在于目标文件".into());
        }
        let old_storyline = if let Some(id) = original {
            Some(self.event_draft(id)?.1.storyline)
        } else {
            None
        };
        let mut text = text;
        let mut source = event_source(
            draft,
            block.as_ref().map(|b| b.indent).unwrap_or(2),
            block.as_ref().map(|b| b.body_indent).unwrap_or(4),
        );
        if let Some(block) = block {
            let suffix = header_comment(&text[block.range.start..block.header_end]);
            if let Some(end) = source.find('\n') {
                source.insert_str(end, suffix);
            }
            if old_storyline.as_deref() == Some(&draft.storyline) {
                text.replace_range(block.range, &source);
                return self.set_text(path, text);
            }
            source = event_source(draft, 2, 4);
            if let Some(end) = source.find('\n') {
                source.insert_str(end, suffix);
            }
            text.replace_range(block.range, "");
        }
        text.push_str(&format!("\nstoryline {}\n{}", draft.storyline, source));
        self.set_text(path, text)
    }

    pub fn move_event(&mut self, id: &str, storyline: &str, position: usize) -> Result<(), String> {
        identifier(storyline)?;
        if self.event_draft(id)?.1.period.is_some() {
            return Err("时段内事件为部分顺序,请用先后约束编辑时间关系".into());
        }
        let result = self.compile_current();
        let mut nodes: Vec<_> = result
            .analysis
            .graph
            .nodes
            .iter()
            .filter(|n| {
                n.is_event
                    && n.storyline == storyline
                    && n.name != id
                    && !result
                        .analysis
                        .timeline
                        .events
                        .iter()
                        .any(|e| e.event == n.name)
            })
            .collect();
        nodes.sort_by_key(|n| (n.seq, &n.name));
        let mut names: Vec<String> = nodes.iter().map(|n| n.name.clone()).collect();
        names.insert(position.min(names.len()), id.into());
        for (i, name) in names.iter().enumerate() {
            let (path, mut draft) = self.event_draft(name)?;
            draft.storyline = storyline.into();
            draft.order = Some((i as u32 + 1) * 10);
            self.write_event(&path, Some(name), &draft)?;
        }
        Ok(())
    }

    pub fn connect_events(
        &mut self,
        from: &str,
        to: &str,
        label: &str,
        drift: bool,
    ) -> Result<(), String> {
        qualified(to)?;
        let (path, mut draft) = self.event_draft(from)?;
        let body_lines = lines(&draft.body, &path);
        let terminal = body_lines
            .iter()
            .find(|l| l.indent == 0 && matches!(l.kind, LineKind::Divert { .. }));
        let arrow = if drift { "->>" } else { "->" };
        let mut body: Vec<String> = draft.body.lines().map(str::to_string).collect();
        if label.trim().is_empty() {
            let line = format!("{arrow} {to}");
            if let Some(terminal) = terminal {
                body[terminal.no as usize - 1] = line;
            } else {
                body.push(line);
            }
        } else {
            let insert = body_lines
                .iter()
                .find(|l| {
                    l.indent == 0
                        && matches!(l.kind, LineKind::Choice { .. } | LineKind::Divert { .. })
                })
                .map(|l| l.no as usize - 1)
                .unwrap_or(body.len());
            body.insert(insert, format!("choice {}\n  {arrow} {to}", quote(label)));
        }
        draft.body = body.join("\n");
        self.write_event(&path, Some(from), &draft)
    }

    pub fn remove_event(&mut self, id: &str) -> Result<(), String> {
        let (path, _) = self.event_draft(id)?;
        let impact = self.deletion_impact(&crate::catalog::TargetRef::new("event", id));
        if !impact.complete {
            return Err("引用检查不完整，请先修复内容或地图诊断，再删除事件".into());
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
            locations.extend(
                impact
                    .graph_views
                    .iter()
                    .map(|reference| format!("{} / {}", reference.view_id, reference.field)),
            );
            locations.extend(impact.manuscripts.iter().map(|reference| {
                format!(
                    "{} / {} ({:?})",
                    reference.manuscript_id, reference.chapter_id, reference.role
                )
            }));
            let locations = locations.join("、");
            return Err(format!(
                "事件 `{id}` 仍有引用，请先明确解除或重新绑定这些引用：{locations}"
            ));
        }
        let mut text = self.document(&path)?.to_string();
        let lines = lines(&text, &path);
        let i = lines
            .iter()
            .position(|l| matches!(&l.kind, LineKind::Event { name, .. } if name == id))
            .ok_or("事件不存在")?;
        text.replace_range(block_at(&text, &lines, i).range, "");
        self.set_text(&path, text)
    }
}

fn event_source(draft: &EventDraft, indent: usize, body_indent: usize) -> String {
    let padding = " ".repeat(indent);
    let mut out = format!("{padding}{}\n", event_header(draft));
    let padding = " ".repeat(body_indent);
    // 效果声明统一放在正文后,避免正文开头的注释在下一次提取时附着到效果块尾部。
    // 运行时机由 when 决定,与效果声明在正文前后的位置无关。
    for line in draft.body.trim_end().lines() {
        out.push_str(&format!("{padding}{line}\n"));
    }
    for effect in &draft.effects {
        let when = match effect.when {
            EffectWhen::Enter => "enter",
            EffectWhen::Done => "done",
            EffectWhen::Exit => "exit",
        };
        out.push_str(&format!("{padding}effect on {when}"));
        if !effect.condition.trim().is_empty() {
            out.push_str(&format!(" if {}", effect.condition.trim()));
        }
        out.push('\n');
        for line in effect.actions.trim_end().lines() {
            out.push_str(&format!("{padding}  {line}\n"));
        }
    }
    out.push('\n');
    out
}

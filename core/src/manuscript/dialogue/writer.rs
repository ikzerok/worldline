use super::*;

pub(super) struct Edit {
    pub range: Range<usize>,
    pub before: String,
    pub after: String,
    pub expected: Option<DialogueDraft>,
    pub line: u32,
    pub original: Option<DialogueStatement>,
    pub losses: Vec<String>,
    pub confirmed: bool,
    pub no_change: bool,
}

pub(super) fn prepare(
    buffer: &WritingBuffer,
    projection: &DialogueProjection,
    operation: &DialogueOperation,
) -> Result<Edit> {
    let statement = |id: &str| {
        projection
            .statements
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| {
                DialogueError::new("STALE_DRAFT", "正式语句标识已过期，不能用显示块或选区代替")
            })
    };
    let (original, draft, losses, confirmed) = match operation {
        DialogueOperation::Insert { anchor_id, draft } => {
            let anchor = projection
                .anchors
                .iter()
                .find(|a| &a.id == anchor_id)
                .ok_or_else(|| {
                    DialogueError::new("STALE_DRAFT", "插入锚已过期或不是正式安全位置")
                })?;
            validate_draft(draft, projection)?;
            let body = encode(draft, None, buffer.path())?;
            let after = format!(
                "{}{}{body}{}",
                if anchor.prefix_newline {
                    &anchor.newline
                } else {
                    ""
                },
                anchor.indent,
                if anchor.suffix_newline {
                    &anchor.newline
                } else {
                    ""
                }
            );
            return Ok(Edit {
                range: anchor.byte_offset..anchor.byte_offset,
                before: String::new(),
                after,
                expected: Some(parts::normalized(draft)),
                line: anchor.line,
                original: None,
                losses: Vec::new(),
                confirmed: true,
                no_change: false,
            });
        }
        DialogueOperation::Delete { statement_id } => {
            let old = statement(statement_id)?;
            ensure_comments(buffer, old)?;
            return Ok(Edit {
                range: old.source.byte_start..old.source.byte_end,
                before: old.source.excerpt.clone(),
                after: String::new(),
                expected: None,
                line: old.source.line,
                original: Some(old.clone()),
                losses: deletion_losses(old),
                confirmed: true,
                no_change: false,
            });
        }
        DialogueOperation::Update {
            statement_id,
            draft,
        } => {
            let old = statement(statement_id)?;
            if old.kind != draft.kind {
                return Err(DialogueError::new(
                    "UNSUPPORTED_CONVERSION",
                    "更新不能隐式转换语句种类，请使用明确转换动作",
                ));
            }
            let losses = if draft.direction.is_none() {
                old.draft
                    .direction
                    .as_ref()
                    .map(|d| format!("direction 将移除：{d}"))
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            };
            (old, draft.clone(), losses, true)
        }
        DialogueOperation::Convert {
            statement_id,
            to,
            speaker,
            allow_direction_loss,
        } => {
            let old = statement(statement_id)?;
            if *to == DialogueKind::Say && (old.glue || !old.tags.is_empty()) {
                return Err(DialogueError::new(
                    "UNSUPPORTED_CONVERSION",
                    "带粘接或标签的正文不能无损转换为当前 Say；原稿已保留",
                ));
            }
            let mut draft = old.draft.clone();
            draft.kind = *to;
            draft.speaker = speaker.clone();
            let mut losses = Vec::new();
            let mut confirmed = true;
            if *to == DialogueKind::Text {
                if speaker.is_some() {
                    return Err(DialogueError::new(
                        "INVALID_REQUEST",
                        "旁白转换不能指定说话者",
                    ));
                }
                if let Some(direction) = draft.direction.take() {
                    losses.push(format!("direction 将移除：{direction}"));
                    confirmed = *allow_direction_loss;
                }
            }
            (old, draft, losses, confirmed)
        }
    };
    validate_draft(&draft, projection)?;
    let no_change = parts::normalized(&original.draft) == parts::normalized(&draft);
    if !no_change {
        ensure_comments(buffer, original)?;
    }
    let after = if no_change {
        original.source.excerpt.clone()
    } else {
        encode(&draft, Some(original), buffer.path())?
    };
    Ok(Edit {
        range: original.source.byte_start..original.source.byte_end,
        before: original.source.excerpt.clone(),
        after,
        expected: Some(parts::normalized(&draft)),
        line: original.source.line,
        original: Some(original.clone()),
        losses,
        confirmed,
        no_change,
    })
}

fn ensure_comments(buffer: &WritingBuffer, statement: &DialogueStatement) -> Result<()> {
    if crate::lexer::comment_source_spans(buffer.source())
        .iter()
        .any(|comment| {
            comment.range.start < statement.source.byte_end
                && comment.range.end > statement.source.byte_start
        })
    {
        return Err(DialogueError::new(
            "SOURCE_UNAVAILABLE",
            "语句内部含块注释，不能无损重建；请保留输入并在源码中编辑",
        ));
    }
    Ok(())
}

fn validate_draft(draft: &DialogueDraft, projection: &DialogueProjection) -> Result<()> {
    match draft.kind {
        DialogueKind::Say => {
            if !draft.speaker.as_ref().is_some_and(|speaker| {
                speaker.kind == "character"
                    && projection.speakers.iter().any(|s| &s.target == speaker)
            }) {
                return Err(DialogueError::new(
                    "INVALID_SPEAKER",
                    "正式台词必须选择当前工程中精确的 character 身份",
                ));
            }
        }
        DialogueKind::Text if draft.speaker.is_some() || draft.direction.is_some() => {
            return Err(DialogueError::new(
                "INVALID_DRAFT",
                "普通正文不能含正式说话者或演出备注",
            ));
        }
        DialogueKind::Text => {}
    }
    Ok(())
}

fn encode(
    draft: &DialogueDraft,
    original: Option<&DialogueStatement>,
    path: &std::path::Path,
) -> Result<String> {
    let quoted = draft.kind == DialogueKind::Say;
    let mut body = String::new();
    for part in &draft.parts {
        match part {
            DialoguePart::Literal { text } => body.push_str(&escape(text, true)?),
            DialoguePart::Expression { source } => {
                let unchanged = original.is_some_and(|statement| statement.draft.parts.iter().any(|part|
                    matches!(part, DialoguePart::Expression { source: existing } if existing == source)));
                if !unchanged {
                    parts::expression(source)?;
                }
                body.push('{');
                body.push_str(&if quoted {
                    escape(source, false)?
                } else {
                    source.clone()
                });
                body.push('}');
            }
            DialoguePart::Link { target, label } => {
                let value = crate::navigation::link_source(target, label, &path.to_string_lossy())
                    .map_err(|e| DialogueError::new("INVALID_DRAFT", e))?;
                body.push_str(&value);
            }
        }
    }
    let mut output = if quoted {
        let speaker = draft
            .speaker
            .as_ref()
            .ok_or_else(|| DialogueError::new("INVALID_SPEAKER", "台词缺少说话者"))?;
        let mut output = format!("say {} \"{body}\"", speaker.id);
        if let Some(direction) = &draft.direction {
            output.push_str(&format!(" direction \"{}\"", escape(direction, false)?));
        }
        output
    } else {
        format!("\\{body}")
    };
    if let Some(original) = original {
        if !quoted {
            if original.glue {
                output.push_str(" ~");
            }
            for tag in &original.tags {
                output.push_str(&format!(" #{tag}"));
            }
        }
        if let Some(id) = &original.localization_id {
            output.push_str(&format!(" #wl-localization:{id}"));
        }
    }
    Ok(output)
}

fn escape(text: &str, body: bool) -> Result<String> {
    let mut output = String::new();
    for ch in text.chars() {
        match ch {
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '{' | '}' | '[' | ']' | '#' | '~' if body => {
                output.push('\\');
                output.push(ch);
            }
            value if value.is_control() => {
                return Err(DialogueError::new(
                    "INVALID_DRAFT",
                    "此控制字符不能用现有语言无损编码，请使用源码入口",
                ))
            }
            _ => output.push(ch),
        }
    }
    Ok(output)
}

fn deletion_losses(statement: &DialogueStatement) -> Vec<String> {
    let mut losses = Vec::new();
    if let Some(direction) = &statement.draft.direction {
        losses.push(format!("删除语句及其 direction：{direction}"));
    }
    if let Some(id) = &statement.localization_id {
        losses.push(format!(
            "删除本地化源 ID：{id}；旧译文保留，按既有规则成为孤立条目"
        ));
    }
    if statement.glue {
        losses.push("删除此句的粘接元数据".into());
    }
    if !statement.tags.is_empty() {
        losses.push(format!("删除此句标签：{}", statement.tags.join("、")));
    }
    losses
}

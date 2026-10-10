use super::*;
use crate::ast::{Stmt, TextStmt};
use crate::{manuscript::ReviewNode, CompileResult};
mod index;

pub(super) fn build(
    project: &Project,
    buffer: &WritingBuffer,
    target: &TargetRef,
    result: &CompileResult,
) -> Result<DialogueProjection> {
    if !matches!(target.kind.as_str(), "event" | "scene" | "fragment") {
        return Err(DialogueError::new(
            "SOURCE_UNAVAILABLE",
            "正式对白来源仅支持事件、场景或片段",
        ));
    }
    let review = crate::manuscript::review_projection(result, target).map_err(|e| {
        DialogueError::new(
            if e.code == "review_limit" {
                "BUDGET_EXCEEDED"
            } else {
                "INVALID_DRAFT"
            },
            e.message,
        )
    })?;
    let signature = serde_json::to_vec(&(
        project.root.to_string_lossy(),
        project.content_baseline(),
        project.search_refresh_generation(),
        project.manuscript_observation_key(),
        buffer.path().to_string_lossy(),
        buffer.original(),
        buffer.source(),
        buffer.generation(),
        target,
    ))
    .map_err(|e| DialogueError::new("INVALID_REQUEST", e.to_string()))?;
    let snapshot = crate::presentation_commands::document_hash(&signature);
    let records = index::records(result, &review.nodes, buffer.path());
    let mut anchors = index::Anchors::new(buffer.source());
    let mut projection = DialogueProjection {
        schema_version: 1,
        target: target.clone(),
        baseline: project.content_baseline(),
        generation: buffer.generation(),
        snapshot,
        statements: Vec::new(),
        anchors: Vec::new(),
        rows: Vec::new(),
        speakers: result
            .analysis
            .catalog
            .objects
            .iter()
            .filter(|o| o.target.kind == "character")
            .map(|o| ReviewSpeaker {
                target: o.target.clone(),
                display: o.display.clone(),
            })
            .collect(),
        complete: true,
    };
    projection
        .speakers
        .sort_by(|a, b| (&a.display, &a.target.id).cmp(&(&b.display, &b.target.id)));
    rows(
        &review.nodes,
        0,
        result,
        buffer,
        &records,
        &mut anchors,
        &mut projection,
    )?;
    if !projection.rows.iter().any(|row| {
        row.source
            .as_ref()
            .is_some_and(|s| std::path::Path::new(&s.file) == buffer.path())
    }) {
        return Err(DialogueError::new(
            "SOURCE_UNAVAILABLE",
            "当前文件不属于所选对白来源",
        ));
    }
    if serde_json::to_vec(&projection)
        .map_err(|e| DialogueError::new("INVALID_REQUEST", e.to_string()))?
        .len()
        > crate::manuscript::MAX_REVIEW_JSON_BYTES
    {
        return Err(DialogueError::new(
            "BUDGET_EXCEEDED",
            "对白投影超过 1 MiB，请缩小来源范围",
        ));
    }
    Ok(projection)
}

#[allow(clippy::too_many_arguments)]
fn rows(
    nodes: &[ReviewNode],
    depth: usize,
    result: &CompileResult,
    buffer: &WritingBuffer,
    records: &index::Records<'_>,
    anchors: &mut index::Anchors,
    out: &mut DialogueProjection,
) -> Result<()> {
    for node in nodes {
        let mut statement_id = None;
        if let Some(source) = node
            .source
            .as_ref()
            .filter(|s| std::path::Path::new(&s.file) == buffer.path())
        {
            if let Some(statement) = records.get(&(source.file.clone(), source.line)) {
                let content = match statement {
                    Stmt::Text(text) => Some((text, DialogueKind::Text, None, None)),
                    Stmt::Say(say) => Some((
                        &say.text,
                        DialogueKind::Say,
                        Some(TargetRef::new("character", &say.speaker)),
                        say.direction.clone(),
                    )),
                    _ => None,
                };
                if let Some((text, kind, speaker, direction)) = content {
                    let id = format!(
                        "{}:statement:{}:{}",
                        out.snapshot, source.line, source.byte_start
                    );
                    let parts = parts::project(result, text, source, kind == DialogueKind::Say)?;
                    let after_anchor_id = anchor(buffer, source, true, anchors, out);
                    anchor(buffer, source, false, anchors, out);
                    out.statements.push(make_statement(
                        id.clone(),
                        source,
                        text,
                        kind,
                        speaker,
                        direction,
                        parts,
                        after_anchor_id,
                    ));
                    statement_id = Some(id);
                } else if matches!(
                    statement,
                    Stmt::Divert(_)
                        | Stmt::Call(_)
                        | Stmt::Return(_)
                        | Stmt::Local(_)
                        | Stmt::Let(_)
                        | Stmt::Set(_)
                ) {
                    anchor(buffer, source, false, anchors, out);
                }
            }
        }
        let mut label = node.label.clone();
        if let Some(condition) = &node.condition {
            label.push_str(&format!(" · {condition}（未求值）"));
        }
        if let Some(enable) = &node.enable {
            label.push_str(&format!(" · enable {enable}（未求值）"));
        }
        out.rows.push(DialogueRow {
            depth,
            label,
            statement_id,
            source: node.source.clone(),
            kind: node.kind,
        });
        rows(
            &node.children,
            depth + 1,
            result,
            buffer,
            records,
            anchors,
            out,
        )?;
        if let Some(label) = &node.end_label {
            out.rows.push(DialogueRow {
                depth,
                label: label.clone(),
                statement_id: None,
                source: None,
                kind: ReviewKind::Structure,
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn make_statement(
    id: String,
    source: &ReviewSource,
    text: &TextStmt,
    kind: DialogueKind,
    speaker: Option<TargetRef>,
    direction: Option<String>,
    parts: Vec<DialogueMappedPart>,
    after_anchor_id: Option<String>,
) -> DialogueStatement {
    let draft = DialogueDraft {
        kind,
        speaker,
        direction,
        parts: parts.iter().map(|p| p.part.clone()).collect(),
    };
    DialogueStatement {
        id,
        kind,
        source: source.clone(),
        draft,
        parts,
        glue: text.glue,
        tags: text.tags.clone(),
        localization_id: text.localization_id.clone(),
        after_anchor_id,
    }
}

fn anchor(
    buffer: &WritingBuffer,
    source: &ReviewSource,
    after: bool,
    anchors: &mut index::Anchors,
    out: &mut DialogueProjection,
) -> Option<String> {
    let text = buffer.source();
    let start = text[..source.byte_start].rfind('\n').map_or(0, |i| i + 1);
    let physical_end = text[start..].find('\n').map(|i| start + i + 1);
    let end = physical_end.unwrap_or(text.len());
    let indent = &text[start..source.byte_start];
    if !indent.chars().all(|c| matches!(c, ' ' | '\t')) {
        return None;
    }
    let at = if after { end } else { start };
    if !anchors.safe_at(at) {
        return None;
    }
    let newline = if text[start..end].ends_with("\r\n") {
        "\r\n"
    } else if physical_end.is_some() {
        "\n"
    } else if text[..start].ends_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let id = format!("{}:anchor:{at}:{}", out.snapshot, indent.len());
    if anchors.insert(&id) {
        out.anchors.push(DialogueInsertionAnchor {
            id: id.clone(),
            line: source.line + u32::from(after),
            byte_offset: at,
            label: format!(
                "第 {} 行{}（同层语句）",
                source.line,
                if after { "之后" } else { "之前" }
            ),
            indent: indent.into(),
            newline: newline.into(),
            prefix_newline: after && physical_end.is_none(),
            suffix_newline: !after || physical_end.is_some(),
        });
    }
    Some(id)
}

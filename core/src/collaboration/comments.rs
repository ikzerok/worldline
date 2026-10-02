use super::{
    absolute_path, register_new_document, relative_path, report, update_known_fields, AnchorStatus,
    CollaborationResult, CommentAnchor, CommentCommand, CommentDocument, CommentDraft,
    CommentIndex, CommentReference,
};
use crate::catalog::TargetRef;
use crate::presentation::MapIndex;
use crate::presentation_commands::{document_hash, Revision};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, valid_id};
use crate::CompileResult;
use serde_json::{json, Value};
use std::path::Path;

fn selected_lines(text: &str, start_line: u32, end_line: u32) -> Option<String> {
    if start_line == 0 || end_line < start_line {
        return None;
    }
    let lines = text.lines().collect::<Vec<_>>();
    let start = usize::try_from(start_line - 1).ok()?;
    let end = usize::try_from(end_line).ok()?;
    if start >= lines.len() || end > lines.len() {
        return None;
    }
    Some(lines[start..end].join("\n"))
}

pub fn capture_text_anchor(
    project: &Project,
    path: &Path,
    start_line: u32,
    end_line: u32,
) -> Result<CommentAnchor, String> {
    let text = project.document(path)?;
    let quote =
        selected_lines(text, start_line, end_line).ok_or("正文批注范围无效，请重新选择行号")?;
    Ok(CommentAnchor::TextRange {
        path: relative_path(project, path)?,
        start_line,
        end_line,
        baseline_hash: document_hash(quote.as_bytes()),
        quote,
    })
}

pub fn anchor_status(
    project: &Project,
    content: &CompileResult,
    maps: &MapIndex,
    anchor: &CommentAnchor,
) -> AnchorStatus {
    let attached = match anchor {
        CommentAnchor::Object { target } => content.analysis.catalog.object(target).is_some(),
        CommentAnchor::MapPlacement {
            map_id,
            placement_id,
        } => maps.maps.get(map_id).is_some_and(|map| {
            map.placements.contains_key(placement_id)
                || map
                    .scene
                    .as_ref()
                    .is_some_and(|scene| scene.nodes.contains_key(placement_id))
        }),
        CommentAnchor::TextRange {
            path,
            start_line,
            end_line,
            baseline_hash,
            quote,
        } => absolute_path(project, path)
            .ok()
            .and_then(|path| project.document(&path).ok())
            .and_then(|text| selected_lines(text, *start_line, *end_line))
            .is_some_and(|current| {
                &current == quote && document_hash(current.as_bytes()) == *baseline_hash
            }),
    };
    if attached {
        AnchorStatus::Attached
    } else {
        AnchorStatus::Detached
    }
}

fn validate_new_anchor(
    project: &Project,
    content: &CompileResult,
    maps: &MapIndex,
    anchor: &CommentAnchor,
) -> Result<(), String> {
    if anchor_status(project, content, maps, anchor) == AnchorStatus::Attached {
        Ok(())
    } else {
        Err("批注锚点当前无法解析；请重新选择对象、标记或正文范围".into())
    }
}

pub fn build_comment_index(
    project: &Project,
    content: &CompileResult,
    maps: &MapIndex,
) -> CommentIndex {
    let mut index = CommentIndex::default();
    let manifest = manifest_path(&project.root);
    let Ok(document) = project.authoring_document(&manifest) else {
        return index;
    };
    if document.is_deleted() {
        return index;
    }
    let registry = parse_registry(&project.root, document.bytes());
    index.diagnostics.extend(registry.diagnostics);
    for (id, path) in registry.comments {
        let parsed = (|| -> Result<(CommentDraft, Value, bool), String> {
            let document = project.authoring_document(&path)?;
            if document.is_deleted() {
                return Err("注册的批注文档已删除".into());
            }
            let source = parse_unique_json(document.bytes())
                .map_err(|error| format!("批注 JSON 无法解析：{error}"))?;
            if source.get("schema_version").and_then(Value::as_u64) != Some(1) {
                return Err("批注 schema_version 不受支持".into());
            }
            let draft: CommentDraft = serde_json::from_value(source.clone())
                .map_err(|error| format!("批注结构无效：{error}"))?;
            if draft.id != id {
                return Err("批注 ID 与清单注册 ID 不一致".into());
            }
            Ok((draft, source, document.is_read_only()))
        })();
        match parsed {
            Ok((draft, source, read_only)) => {
                let anchor_status = anchor_status(project, content, maps, &draft.anchor);
                index.comments.insert(
                    id,
                    CommentDocument {
                        draft,
                        path,
                        source,
                        read_only,
                        anchor_status,
                    },
                );
            }
            Err(error) => report(&mut index.diagnostics, &path, "COLLAB001", error),
        }
    }
    crate::sort_diagnostics(&mut index.diagnostics);
    index
}

impl CommentIndex {
    pub fn references_to(&self, target: &TargetRef) -> Vec<CommentReference> {
        self.comments
            .values()
            .filter_map(|comment| match &comment.draft.anchor {
                CommentAnchor::Object { target: anchor } if anchor == target => {
                    Some(CommentReference {
                        comment_id: comment.draft.id.clone(),
                        file: comment.path.to_string_lossy().into_owned(),
                        anchor: "object".into(),
                    })
                }
                _ => None,
            })
            .collect()
    }
}

pub fn write_comment(
    project: &mut Project,
    revision: &mut Revision,
    command: CommentCommand,
) -> Result<CollaborationResult, String> {
    if command.expected_revision != *revision
        || command.expected_baseline != project.content_baseline()
    {
        return Err("StaleRevision：批注基线已过期，请重新检查后提交".into());
    }
    if !valid_id(&command.draft.id)
        || command.draft.author.trim().is_empty()
        || command.draft.body.trim().is_empty()
    {
        return Err("批注需要有效 ID、作者和正文".into());
    }
    if command
        .original
        .as_deref()
        .is_some_and(|id| id != command.draft.id)
    {
        return Err("批注 ID 是稳定身份，不能在编辑时改名".into());
    }
    let content = project.compile();
    let maps = crate::presentation_commands::map_index_with_content(project, &content);
    let index = build_comment_index(project, &content, &maps);
    if index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
    {
        return Err("批注索引含错误，请先修复后再写入".into());
    }
    let old = command
        .original
        .as_deref()
        .and_then(|id| index.comments.get(id));
    if command.original.is_some() && old.is_none() {
        return Err("待编辑批注不存在".into());
    }
    if old.is_none_or(|comment| comment.draft.anchor != command.draft.anchor) {
        validate_new_anchor(project, &content, &maps, &command.draft.anchor)?;
    }
    project.checkpoint_disk_baselines_match()?;
    if old.is_some_and(|comment| comment.read_only) {
        return Err("批注文档为只读，不能覆盖".into());
    }
    if command.original.is_none() && index.comments.contains_key(&command.draft.id) {
        return Err("批注 ID 已存在".into());
    }
    let path = old.map(|comment| comment.path.clone()).unwrap_or_else(|| {
        project
            .root
            .join(format!(".world/comments/{}.json", command.draft.id))
    });
    let mut source = old
        .map(|comment| comment.source.clone())
        .unwrap_or_else(|| json!({"schema_version":1}));
    let mut fresh = serde_json::to_value(&command.draft).map_err(|error| error.to_string())?;
    preserve_anchor_extensions(&source, &mut fresh);
    update_known_fields(&mut source, &fresh)?;
    let bytes = serde_json::to_vec_pretty(&source).map_err(|error| error.to_string())?;
    let mut candidate = project.clone();
    let changed_files = if old.is_some() {
        candidate.set_authoring_document(&path, bytes)?;
        vec![path]
    } else {
        register_new_document(
            project,
            &mut candidate,
            "comments",
            "collaboration.comments.v1",
            &command.draft.id,
            &path,
            bytes,
        )?
    };
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(CollaborationResult {
        changed_files,
        new_revision: *revision,
    })
}

// 只覆盖已理解的字段，扩展字段不是 UI 重建锚点的附带损失。
fn preserve_anchor_extensions(source: &Value, fresh: &mut Value) {
    let Some(old) = source.get("anchor").and_then(Value::as_object) else {
        return;
    };
    let Some(new) = fresh.get_mut("anchor").and_then(Value::as_object_mut) else {
        return;
    };
    for (key, value) in old {
        if !matches!(
            key.as_str(),
            "kind"
                | "target"
                | "map_id"
                | "placement_id"
                | "path"
                | "start_line"
                | "end_line"
                | "baseline_hash"
                | "quote"
        ) {
            new.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    if let (Some(old), Some(new)) = (
        old.get("target").and_then(Value::as_object),
        new.get_mut("target").and_then(Value::as_object_mut),
    ) {
        for (key, value) in old {
            if !matches!(key.as_str(), "kind" | "id") {
                new.entry(key.clone()).or_insert_with(|| value.clone());
            }
        }
    }
}

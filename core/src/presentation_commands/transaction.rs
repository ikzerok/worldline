use super::*;

pub(super) fn apply_inner(
    project: &mut Project,
    revision: &mut Revision,
    envelope: CommandEnvelope,
    content: &crate::CompileResult,
) -> Result<CommandResult, EditError> {
    if envelope.expected_revision != *revision {
        return Err(EditError::StaleRevision {
            expected: envelope.expected_revision,
            actual: *revision,
        });
    }

    let map_path = map_document_path(project, envelope.command.map_id())?;
    let _expected_map =
        envelope
            .expected_documents
            .get(&map_path)
            .ok_or_else(|| EditError::ExternalConflict {
                path: map_path.clone(),
                expected: "缺少地图文档基线".into(),
                actual: project
                    .authoring_document(&map_path)
                    .map(|document| document_hash(document.bytes()))
                    .unwrap_or_else(|error| format!("缺失:{error}")),
            })?;
    for (path, expected) in &envelope.expected_documents {
        let document =
            project
                .authoring_document(path)
                .map_err(|message| EditError::ExternalConflict {
                    path: path.clone(),
                    expected: expected.clone(),
                    actual: format!("缺失:{message}"),
                })?;
        let actual = document_hash(document.bytes());
        if &actual != expected {
            return Err(EditError::ExternalConflict {
                path: path.clone(),
                expected: expected.clone(),
                actual,
            });
        }
    }

    let before_revision = *revision;
    let applied = document::apply_to_document(project, &envelope.command, content)?;
    let path = applied.path;
    let applied_revision = revision.next_presentation();
    let changed_files = vec![path.clone()];
    let undo_record = UndoRecord {
        base_revision: before_revision,
        applied_revision,
        changes: vec![DocumentChange {
            path,
            before: applied.before,
            after: applied.after,
        }],
    };
    *revision = applied_revision;
    Ok(CommandResult {
        new_revision: applied_revision,
        changed_files,
        affected_refs: applied.affected_refs,
        undo_record,
        diagnostics: applied.diagnostics,
    })
}

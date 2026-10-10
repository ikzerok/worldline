use super::*;

#[test]
fn ordinary_inventory_mutation_invalidates_preview_even_without_project_refresh() {
    let (work, mut project) = Workspace::new(BASIC, "1.11", false);
    fs::write(work.0.join("private.txt"), "before").unwrap();
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: say("新句"),
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    let baseline = project.content_baseline();
    fs::write(work.0.join("private.txt"), "after").unwrap();
    assert!(project.stage_dialogue_edit(&mut buffer, &plan).is_err());
    assert!(project.apply_dialogue_edit(&buffer, &plan).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer.source(), BASIC);
}

#[test]
fn blocked_migration_never_leaves_manifest_or_partial_statement_and_flags_fingerprint() {
    let source = "character a\nevent start\n  第一行\n  local invalid syntax\n  -> END\n";
    let (_work, mut project) = Workspace::new(source, "1.9", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let mut command = request(
        &project,
        &buffer,
        DialogueOperation::Convert {
            statement_id: old.id,
            to: DialogueKind::Say,
            speaker: Some(TargetRef::new("character", "a")),
            allow_direction_loss: false,
        },
    );
    command.enable_language_1_11 = true;
    let baseline = project.content_baseline();
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert!(!plan.can_apply);
    assert!(!plan.fingerprint_comparison_reliable);
    assert!(!plan.migration.as_ref().unwrap().can_apply);
    assert!(project.stage_dialogue_edit(&mut buffer, &plan).is_err());
    assert!(project.apply_dialogue_edit(&buffer, &plan).is_err());
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer.source(), source);
}

#[test]
fn deleting_statement_keeps_trailing_comments_and_reports_metadata_losses() {
    let source = "character a\nevent start\n  say a \"原句\" direction \"备注\" #wl-localization:line_a // 保留🙂\n  -> END\n";
    let (_work, project) = Workspace::new(source, "1.11", true);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Delete {
            statement_id: old.id,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert!(plan.metadata_losses.iter().any(|s| s.contains("备注")));
    assert!(plan.metadata_losses.iter().any(|s| s.contains("line_a")));
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    assert!(buffer.source().contains(" // 保留🙂\n"));
    assert!(project_rows(&project, &buffer).statements.is_empty());
    assert!(project.dialogue_continuation(&buffer, &plan).is_err());
}

#[test]
fn unrepresentable_literal_text_and_internal_comments_fail_losslessly() {
    let source = "character a\nevent start\n  普通正文\n  -> END\n";
    let (_work, project) = Workspace::new(source, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    for value in ["前面 // 后面", "  前导空格", "末尾空格 "] {
        let draft = DialogueDraft {
            kind: DialogueKind::Text,
            speaker: None,
            direction: None,
            parts: vec![literal(value)],
        };
        let command = request(
            &project,
            &buffer,
            DialogueOperation::Update {
                statement_id: old.id.clone(),
                draft,
            },
        );
        assert!(
            project.preview_dialogue_edit(&buffer, &command).is_err(),
            "{value}"
        );
        assert_eq!(buffer.source(), source);
    }
    let source = "character a\nevent start\n  say /*内部注释*/ a \"正文\"\n  -> END\n";
    let (_work, project) = Workspace::new(source, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: say("修改"),
        },
    );
    assert_eq!(
        project
            .preview_dialogue_edit(&buffer, &command)
            .unwrap_err()
            .code,
        "SOURCE_UNAVAILABLE"
    );
}

#[test]
fn unknown_capability_and_stale_refresh_refuse_form_operations() {
    let (work, mut project) = Workspace::new(BASIC, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: say("新句"),
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    fs::write(work.0.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.11","required_features":["future.unknown.v1"]}"#).unwrap();
    project.refresh().unwrap();
    assert_eq!(
        project
            .stage_dialogue_edit(&mut buffer, &plan)
            .unwrap_err()
            .code,
        "READ_ONLY"
    );
    assert!(project.project_dialogue_buffer(&buffer, &start()).is_err());
    assert_eq!(buffer.source(), BASIC);
}

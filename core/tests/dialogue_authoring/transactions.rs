use super::*;

#[test]
fn text_updates_preserve_glue_tags_id_and_comments_but_conversion_refuses_them() {
    let source = "character a\nevent start\n  原句 ~ #opaque #wl-localization:line_a // 尾注\n  第二句\n  -> END\n";
    let (_work, project) = Workspace::new(source, "1.11", true);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let convert = request(
        &project,
        &buffer,
        DialogueOperation::Convert {
            statement_id: old.id.clone(),
            to: DialogueKind::Say,
            speaker: Some(TargetRef::new("character", "a")),
            allow_direction_loss: false,
        },
    );
    assert_eq!(
        project
            .preview_dialogue_edit(&buffer, &convert)
            .unwrap_err()
            .code,
        "UNSUPPORTED_CONVERSION"
    );
    let mut draft = old.draft.clone();
    draft.parts = vec![literal("更改😀")];
    let update = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &update).unwrap();
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    let new = project_rows(&project, &buffer).statements.remove(0);
    assert!(new.glue);
    assert_eq!(new.tags, vec!["opaque"]);
    assert_eq!(new.localization_id.as_deref(), Some("line_a"));
    assert!(buffer.source().contains(" // 尾注\n"));
}

#[test]
fn direction_loss_is_visible_and_requires_new_confirmed_preview() {
    let source = BASIC.replace("\"原句\"", "\"原句\" direction \"私密备注\"");
    let (_work, project) = Workspace::new(&source, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let mut command = request(
        &project,
        &buffer,
        DialogueOperation::Convert {
            statement_id: old.id,
            to: DialogueKind::Text,
            speaker: None,
            allow_direction_loss: false,
        },
    );
    let unconfirmed = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert!(!unconfirmed.can_apply);
    assert!(unconfirmed.metadata_losses[0].contains("私密备注"));
    assert!(project
        .stage_dialogue_edit(&mut buffer, &unconfirmed)
        .is_err());
    if let DialogueOperation::Convert {
        allow_direction_loss,
        ..
    } = &mut command.operation
    {
        *allow_direction_loss = true;
    }
    let mut forged = unconfirmed.clone();
    forged.request = command.clone();
    assert!(project.stage_dialogue_edit(&mut buffer, &forged).is_err());
    let confirmed = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert_ne!(unconfirmed.plan_digest, confirmed.plan_digest);
    project
        .stage_dialogue_edit(&mut buffer, &confirmed)
        .unwrap();
    assert_eq!(
        project_rows(&project, &buffer).statements[0].kind,
        DialogueKind::Text
    );
    assert!(!buffer.source().contains("私密备注"));
}

#[test]
fn migration_and_full_draft_apply_atomically_and_can_restore_then_save_reopen() {
    let source = "character a\nevent start\n  原句\n  -> END\n";
    let (work, mut project) = Workspace::new(source, "1.9", false);
    let before = project.clone();
    let baseline = project.content_baseline();
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    buffer.replace_source(buffer.source().replace("原句", "完整当前草稿"));
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
    assert_eq!(
        project
            .preview_dialogue_edit(&buffer, &command)
            .unwrap_err()
            .code,
        "MIGRATION_REQUIRED"
    );
    assert_eq!(project.content_baseline(), baseline);
    command.enable_language_1_11 = true;
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert!(plan.migration.is_some());
    assert!(plan.includes_unapplied_draft);
    assert_eq!(plan.changes.len(), 2);
    assert!(project.stage_dialogue_edit(&mut buffer, &plan).is_err());
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(fs::read_to_string(work.0.join("world.wl")).unwrap(), source);
    project.apply_dialogue_edit(&buffer, &plan).unwrap();
    assert_eq!(project.language_version(), "1.11");
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("完整当前草稿"));
    let applied = project.clone();
    assert!(project.restore(before));
    assert_eq!(project.language_version(), "1.9");
    assert!(project.restore(applied));
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.language_version(), "1.11");
    assert!(reopened
        .document(&reopened.entry)
        .unwrap()
        .contains("say a"));
}

#[test]
fn inserting_and_deleting_preserves_crlf_final_newline_state_and_trailing_comments() {
    let source = "character a\r\nevent start\r\n  //独立\r\n  -> END //尾注";
    let (_work, project) = Workspace::new(source, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let rows = project_rows(&project, &buffer);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Insert {
            anchor_id: rows.anchors[0].id.clone(),
            draft: say("新句\n同句"),
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    assert_eq!(
        buffer.source(),
        "character a\r\nevent start\r\n  //独立\r\n  say a \"新句\\n同句\"\r\n  -> END //尾注"
    );
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Delete {
            statement_id: old.id,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    assert!(buffer.source().ends_with("-> END //尾注"));
    assert!(buffer.source().contains("//独立\r\n  \r\n"));
}

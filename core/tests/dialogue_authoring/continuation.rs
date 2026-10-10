use super::*;

#[test]
fn continuation_proves_changed_inserted_and_unchanged_statement_without_client_offsets() {
    let (_work, project) = Workspace::new(BASIC, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let request = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: say("当前输入"),
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &request).unwrap();
    assert!(project.dialogue_continuation(&buffer, &plan).is_err());
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    let next = project.dialogue_continuation(&buffer, &plan).unwrap();
    let command = super::request(
        &project,
        &buffer,
        DialogueOperation::Insert {
            anchor_id: next.id,
            draft: say(""),
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    let next = project.dialogue_continuation(&buffer, &plan).unwrap();
    let rows = project_rows(&project, &buffer);
    assert_eq!(rows.statements.len(), 2);
    assert!(rows.statements[1].draft.parts.is_empty());
    assert_eq!(rows.statements[1].after_anchor_id.as_ref(), Some(&next.id));
    let command = super::request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: rows.statements[1].id.clone(),
            draft: rows.statements[1].draft.clone(),
        },
    );
    let unchanged = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project
        .stage_dialogue_edit(&mut buffer, &unchanged)
        .unwrap();
    assert_eq!(
        project
            .dialogue_continuation(&buffer, &unchanged)
            .unwrap()
            .id,
        next.id
    );
    buffer.replace_source(buffer.source().replace("当前输入", "后来输入"));
    assert!(project.dialogue_continuation(&buffer, &unchanged).is_err());
}

#[test]
fn atomic_migration_continuation_uses_reopened_current_file_and_is_one_transaction() {
    let (_work, mut project) =
        Workspace::new("character a\nevent start\n  原文\n  -> END\n", "1.9", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
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
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.apply_dialogue_edit(&buffer, &plan).unwrap();
    let current = project.open_writing_buffer(&start()).unwrap();
    let next = project.dialogue_continuation(&current, &plan).unwrap();
    assert_eq!(
        project_rows(&project, &current).statements[0]
            .after_anchor_id
            .as_ref(),
        Some(&next.id)
    );
}

#[test]
fn eof_after_anchor_uses_only_necessary_newline_and_keeps_no_final_newline() {
    let source = "character a\nevent start\n  say a \"最后一句\"";
    let (_work, project) = Workspace::new(source, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Insert {
            anchor_id: old.after_anchor_id.unwrap(),
            draft: say("下一句"),
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    assert_eq!(buffer.source(), format!("{source}\n  say a \"下一句\""));
    assert!(project.dialogue_continuation(&buffer, &plan).is_ok());
}

#[test]
fn writing_identity_distinguishes_undo_branch_same_generation_and_unchanged_is_stable() {
    let (_work, project) = Workspace::new(BASIC, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let mut one = buffer.clone();
    let mut two = buffer.clone();
    one.replace_source(BASIC.replace("原句", "分叉一"));
    two.replace_source(BASIC.replace("原句", "分叉二"));
    assert_eq!(one.generation(), two.generation());
    assert_ne!(one.identity(), two.identity());
    assert_eq!(one.identity(), one.clone().identity());
    assert_ne!(buffer.identity(), one.identity());
}

#[test]
fn continuation_rechecks_inventory_after_staging_and_after_project_apply() {
    for apply in [false, true] {
        let (work, mut project) = Workspace::new(BASIC, "1.11", false);
        fs::write(work.0.join("attachment.txt"), "before").unwrap();
        let mut buffer = project.open_writing_buffer(&start()).unwrap();
        let old = project_rows(&project, &buffer).statements.remove(0);
        let command = request(
            &project,
            &buffer,
            DialogueOperation::Update {
                statement_id: old.id,
                draft: say("修改"),
            },
        );
        let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
        if apply {
            project.apply_dialogue_edit(&buffer, &plan).unwrap();
            buffer = project.open_writing_buffer(&start()).unwrap();
        } else {
            project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
        }
        assert!(project.dialogue_continuation(&buffer, &plan).is_ok());
        fs::write(work.0.join("attachment.txt"), "after").unwrap();
        assert!(project.dialogue_continuation(&buffer, &plan).is_err());
        assert!(buffer.source().contains("修改"));
    }
}

#[test]
fn continuation_is_bound_to_exact_physical_source_file() {
    let (work, _) = Workspace::new(BASIC, "1.11", false);
    let root = "character a\nevent start\ninclude \"one.wl\"\nevent other\ninclude \"two.wl\"\n";
    let body = "  say a \"相同正文\"\n  -> END\n";
    fs::write(work.0.join("world.wl"), root).unwrap();
    fs::write(work.0.join("one.wl"), body).unwrap();
    fs::write(work.0.join("two.wl"), body).unwrap();
    let project = Project::open(&work.0).unwrap();
    let buffer = project
        .open_source_writing_buffer(&work.0.join("one.wl"))
        .unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: old.draft,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert!(plan.no_change);
    assert!(project.dialogue_continuation(&buffer, &plan).is_ok());
    let other = project
        .open_source_writing_buffer(&work.0.join("two.wl"))
        .unwrap();
    assert_eq!(buffer.source(), other.source());
    assert_eq!(buffer.generation(), other.generation());
    assert!(project.dialogue_continuation(&other, &plan).is_err());
}

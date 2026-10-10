use super::*;

#[test]
fn formal_statements_are_separate_from_merged_prose_and_identity_is_stable() {
    let source =
        "character a\nevent start\n  a: 普通文字\n  第二句\n  say a \"正式台词\"\n  -> END\n";
    let (_work, project) = Workspace::new(source, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let rows = project_rows(&project, &buffer);
    assert_eq!(rows.statements.len(), 3);
    assert_eq!(rows.statements[0].kind, DialogueKind::Text);
    assert!(rows.statements[0].draft.speaker.is_none());
    assert_eq!(
        rows.statements[2].draft.speaker,
        Some(TargetRef::new("character", "a"))
    );
    assert_ne!(rows.statements[0].id, rows.statements[1].id);
    assert_eq!(
        rows.snapshot,
        project_rows(&project, &project.open_writing_buffer(&start()).unwrap()).snapshot
    );
    assert!(rows.statements.iter().all(|s| s.after_anchor_id.is_some()));
}

#[test]
fn typed_literals_expressions_links_direction_and_unicode_round_trip_in_one_buffer() {
    let (_work, mut project) = Workspace::new(BASIC, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let mut draft = say("中文😀\n引号\"反斜杠\\花括号{}和[[外形]] #~");
    draft.parts.push(DialoguePart::Expression {
        source: "\"含}的字符串\" + \"🙂\"".into(),
    });
    draft.parts.push(DialoguePart::Link {
        target: TargetRef::new("character", "b"),
        label: "同名角色".into(),
    });
    draft.direction = Some("低声\n\"私密\"\\".into());
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: draft.clone(),
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert!(plan.can_apply);
    assert_eq!(project.document(&project.entry).unwrap(), BASIC);
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    assert_eq!(project.document(&project.entry).unwrap(), BASIC);
    let projected = project_rows(&project, &buffer);
    assert_eq!(projected.statements[0].draft, draft);
    assert!(projected.statements[0].parts[0].source_range.is_none());
    assert!(projected.statements[0].parts[1..]
        .iter()
        .all(|p| p.source_range.is_some()));
    project.apply_writing_buffer(&buffer).unwrap();
    assert!(!project.compile_read_only().unwrap().has_errors());
}

#[test]
fn no_change_keeps_all_source_bytes_and_generation_and_does_not_apply_other_draft() {
    let source = BASIC
        .replace("say a \"原句\"", "say   a   \"原句\"  // 尾注🙂")
        .replace('\n', "\r\n");
    let (_work, mut project) = Workspace::new(&source, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    buffer.replace_source(
        buffer
            .source()
            .replace("character b as", "character b   as"),
    );
    let before = buffer.source().to_owned();
    let generation = buffer.generation();
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
    assert!(plan.changes.is_empty());
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    project.apply_dialogue_edit(&buffer, &plan).unwrap();
    assert_eq!(buffer.source(), before);
    assert_eq!(buffer.generation(), generation);
    assert_eq!(project.document(&project.entry).unwrap(), source);
}

#[test]
fn full_character_identity_is_used_even_with_same_display_names() {
    let (_work, mut project) = Workspace::new(BASIC, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let rows = project_rows(&project, &buffer);
    assert_eq!(rows.speakers.len(), 2);
    let old = &rows.statements[0];
    let mut draft = old.draft.clone();
    draft.speaker = Some(TargetRef::new("character", "b"));
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id.clone(),
            draft,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    assert_eq!(plan.old_speaker, Some(TargetRef::new("character", "a")));
    assert_eq!(plan.new_speaker, Some(TargetRef::new("character", "b")));
    project.apply_dialogue_edit(&buffer, &plan).unwrap();
    assert!(project.document(&project.entry).unwrap().contains("say b"));
}

#[test]
fn net_dialogue_with_per_line_comments_has_complete_ordered_unique_anchors() {
    let mut source = String::from("character a\r\nevent start\r\n");
    for index in 0..180 {
        source.push_str(&format!("  say a \"第{index:03}句：雨停后我们沿河走到港口，讨论那封真实的来信。🙂\" // 此句独立作者尾注{index}\r\n"));
    }
    source.push_str("  -> END\r\nfragment unrelated()\r\n  say a \"未选中的片段不进入当前正文\"\r\n  return\r\n");
    let (_work, project) = Workspace::new(&source, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let projection = project_rows(&project, &buffer);
    assert_eq!(projection.statements.len(), 180);
    assert_eq!(projection.anchors.len(), 181);
    assert!(projection.complete);
    assert_eq!(
        projection.statements[0].draft.parts,
        vec![literal(
            "第000句：雨停后我们沿河走到港口，讨论那封真实的来信。🙂"
        )]
    );
    assert_eq!(
        projection.statements[179].draft.parts,
        vec![literal(
            "第179句：雨停后我们沿河走到港口，讨论那封真实的来信。🙂"
        )]
    );
    let unique = projection
        .anchors
        .iter()
        .map(|anchor| &anchor.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), projection.anchors.len());
    assert_eq!(buffer.source(), source);
    assert_eq!(buffer.generation(), 0);
}

#[test]
fn oversized_net_dialogue_refuses_complete_projection_without_changing_source() {
    let mut source = String::from("character a\nevent start\n");
    for index in 0..1200 {
        source.push_str(&format!("  say a \"第{index:04}句：夜班工人把列车抵达的消息告诉仍在码头等待的同伴。The letter is for the keeper.🙂\" // 私有行尾{index}\n"));
    }
    source.push_str("  -> END\n");
    let (_work, project) = Workspace::new(&source, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let error = project
        .project_dialogue_buffer(&buffer, &start())
        .unwrap_err();
    assert_eq!(error.code, "BUDGET_EXCEEDED");
    assert_eq!(project.document(&project.entry).unwrap(), source);
    assert_eq!(buffer.source(), source);
    assert_eq!(buffer.generation(), 0);
    assert_eq!(fs::read_to_string(&project.entry).unwrap(), source);
}

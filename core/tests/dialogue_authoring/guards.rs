use super::*;

#[test]
fn stale_generation_forged_plan_and_external_disk_edits_write_nothing() {
    let (work, mut project) = Workspace::new(BASIC, "1.11", false);
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
    for tamper in [0, 1, 2, 3] {
        let mut forged = plan.clone();
        match tamper {
            0 => forged.after.push('!'),
            1 => forged.range.start += 1,
            2 => forged.changes.clear(),
            _ => forged.runtime_fingerprint_after ^= 1,
        }
        assert!(project.stage_dialogue_edit(&mut buffer, &forged).is_err());
        assert!(project.apply_dialogue_edit(&buffer, &forged).is_err());
        assert_eq!(buffer.source(), BASIC);
        assert_eq!(project.document(&project.entry).unwrap(), BASIC);
    }
    buffer.replace_source(BASIC.replace("原句", "新输入"));
    assert!(project.stage_dialogue_edit(&mut buffer, &plan).is_err());
    assert!(buffer.source().contains("新输入"));
    let mut fresh = project.open_writing_buffer(&start()).unwrap();
    fs::write(work.0.join("world.wl"), BASIC.replace("原句", "外改")).unwrap();
    assert!(project.stage_dialogue_edit(&mut fresh, &plan).is_err());
    assert!(project.apply_dialogue_edit(&fresh, &plan).is_err());
    assert_eq!(fresh.source(), BASIC);
    assert_eq!(project.document(&project.entry).unwrap(), BASIC);
}

#[test]
fn fragment_and_conditional_rows_do_not_flatten_or_change_structure() {
    let source = "character a\nfragment shared()\n  if true\n    say a \"片段\"\n  return\nevent start\n  call shared()\n  -> END\n";
    let (_work, project) = Workspace::new(source, "1.11", false);
    let target = TargetRef::new("fragment", "shared");
    let mut buffer = project.open_writing_buffer(&target).unwrap();
    let rows = project.project_dialogue_buffer(&buffer, &target).unwrap();
    assert_eq!(rows.statements.len(), 1);
    assert!(rows.rows.iter().any(|r| r.label.contains("未求值")));
    let old = &rows.statements[0];
    let mut command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id.clone(),
            draft: say("片段更新"),
        },
    );
    command.target = target;
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    assert_eq!(buffer.source(), source.replace("片段", "片段更新"));
}

#[test]
fn invalid_forms_and_unrepresentable_text_preserve_draft_and_source_rescue() {
    let (_work, mut project) = Workspace::new(BASIC, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let mut draft = say("输入");
    draft.speaker = Some(TargetRef::new("entity", "a"));
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id.clone(),
            draft,
        },
    );
    assert_eq!(
        project
            .preview_dialogue_edit(&buffer, &command)
            .unwrap_err()
            .code,
        "INVALID_SPEAKER"
    );
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: DialogueDraft {
                kind: DialogueKind::Text,
                speaker: None,
                direction: None,
                parts: vec![literal("坏请求")],
            },
        },
    );
    assert_eq!(
        project
            .preview_dialogue_edit(&buffer, &command)
            .unwrap_err()
            .code,
        "UNSUPPORTED_CONVERSION"
    );
    assert_eq!(buffer.source(), BASIC);
    buffer.replace_source(BASIC.replace("say a \"原句\"", "say a \"未闭合"));
    assert!(project.project_dialogue_buffer(&buffer, &start()).is_err());
    assert!(buffer.source().contains("未闭合"));
    assert!(project.apply_source_writing_buffer(&buffer).is_ok());
    assert!(project.compile_read_only().unwrap().has_errors());
}

#[test]
fn json_rejects_duplicates_unknown_nested_targets_and_budgets() {
    assert!(parse_dialogue_target(r#"{"kind":"event","id":"start","extra":1}"#).is_err());
    assert!(parse_dialogue_target(r#"{"kind":"event","kind":"scene","id":"start"}"#).is_err());
    let (_work, project) = Workspace::new(BASIC, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft: say("输入"),
        },
    );
    let raw = serde_json::to_string(&command).unwrap();
    assert_eq!(parse_dialogue_edit_request(&raw).unwrap(), command);
    let mut json = serde_json::to_value(&command).unwrap();
    json["operation"]["draft"]["speaker"]["extra"] = serde_json::json!(true);
    assert!(parse_dialogue_edit_request(&json.to_string()).is_err());
    assert_eq!(
        parse_dialogue_edit_request(&" ".repeat(MAX_DIALOGUE_REQUEST_BYTES + 1))
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
}

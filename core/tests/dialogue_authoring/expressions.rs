use super::*;

#[test]
fn new_or_modified_expression_budgets_reject_before_recursive_parse_without_writes() {
    let (_work, project) = Workspace::new(BASIC, "1.11", false);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let too_deep = format!("{}1{}", "(".repeat(65), ")".repeat(65));
    let too_many = std::iter::repeat_n("1", 129)
        .collect::<Vec<_>>()
        .join(" + ");
    for source in [too_deep, too_many] {
        let mut draft = say("文字");
        draft.parts.push(DialoguePart::Expression { source });
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
            "BUDGET_EXCEEDED"
        );
        assert_eq!(buffer.source(), BASIC);
        assert_eq!(project.document(&project.entry).unwrap(), BASIC);
    }
}

#[test]
fn editing_other_parts_preserves_already_compiled_large_expression_source_exactly() {
    let expression = std::iter::repeat_n("1", 135)
        .collect::<Vec<_>>()
        .join(" + ");
    let source = BASIC.replace("原句", &format!("{{{expression}}} 原句"));
    let (_work, project) = Workspace::new(&source, "1.11", false);
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    let old = project_rows(&project, &buffer).statements.remove(0);
    let mut draft = old.draft.clone();
    draft.speaker = Some(TargetRef::new("character", "b"));
    draft.direction = Some("保持复杂 token".into());
    *draft.parts.last_mut().unwrap() = literal(" 修改其他文字");
    let command = request(
        &project,
        &buffer,
        DialogueOperation::Update {
            statement_id: old.id,
            draft,
        },
    );
    let plan = project.preview_dialogue_edit(&buffer, &command).unwrap();
    project.stage_dialogue_edit(&mut buffer, &plan).unwrap();
    assert!(buffer.source().contains(&format!("{{{expression}}}")));
    assert!(buffer.source().contains("say b"));
    assert!(buffer.source().contains("修改其他文字"));
}

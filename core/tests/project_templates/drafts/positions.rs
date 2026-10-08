use super::*;

#[test]
fn cross_group_move_preview_reports_direct_parent_and_index_without_instance_migration() {
    let mut project = project("draft-position-move");
    register(&mut project, super::operations::grouped());
    let content = project.compile_object_search_snapshot();
    let before_sources = project.sources();
    let fingerprint = content.analysis.fingerprint;
    let original = project
        .template_draft(
            Source::Existing {
                id: "project:groups".into(),
            },
            &content,
        )
        .unwrap();
    let moved = project
        .edit_template_draft(
            &original.draft,
            &Edit::MoveField {
                field_id: "field_1".into(),
                parent_id: Some("right".into()),
                index: 1,
            },
            &content,
        )
        .unwrap();
    let mutation = project.template_mutation_from_draft(&moved.draft).unwrap();
    let mut revision = Revision::default();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    let changes = preview
        .field_changes
        .iter()
        .filter(|c| c.field_id == "field_1")
        .collect::<Vec<_>>();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].change, "position_changed");
    assert_eq!(changes[0].old_parent_id.as_deref(), Some("left"));
    assert_eq!(changes[0].new_parent_id.as_deref(), Some("right"));
    assert_eq!(changes[0].old_index, Some(0));
    assert_eq!(changes[0].new_index, Some(1));
    assert!(!preview.field_changes.iter().any(|c| c.field_id == "inner"));
    let instance = preview
        .instances
        .iter()
        .find(|instance| instance.target.id == "harbor")
        .unwrap();
    let states = instance
        .fields
        .iter()
        .filter(|field| field.field_id == "field_1")
        .collect::<Vec<_>>();
    assert_eq!(states.len(), 2);
    assert_eq!(states[0].value, states[1].value);
    assert_eq!(states[0].state, states[1].state);
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    assert_eq!(project.sources(), before_sources);
    assert_eq!(
        project
            .compile_object_search_snapshot()
            .analysis
            .fingerprint,
        fingerprint
    );
}

#[test]
fn reorder_plus_label_change_has_separate_position_record_and_root_indexes() {
    let mut project = project("draft-position-reorder");
    register(&mut project, super::operations::grouped());
    let content = project.compile_object_search_snapshot();
    let original = project
        .template_draft(
            Source::Existing {
                id: "project:groups".into(),
            },
            &content,
        )
        .unwrap();
    let moved = project
        .edit_template_draft(
            &original.draft,
            &Edit::MoveField {
                field_id: "ref_field".into(),
                parent_id: None,
                index: 0,
            },
            &content,
        )
        .unwrap();
    let changed = project
        .edit_template_draft(
            &moved.draft,
            &Edit::UpdateField {
                field_id: "ref_field".into(),
                properties: Properties {
                    label: "新目标标签".into(),
                    key: Some("destination".into()),
                    field_type: FieldType::ObjectRef,
                    required: false,
                    choices: vec![],
                    target: Some(DraftTarget {
                        kind: "entity".into(),
                        entity_type: Some("place".into()),
                    }),
                    default: Some(json!({"kind":"entity","id":"harbor"})),
                },
            },
            &content,
        )
        .unwrap();
    let mutation = project
        .template_mutation_from_draft(&changed.draft)
        .unwrap();
    let revision = Revision::default();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    let changes = preview
        .field_changes
        .iter()
        .filter(|c| c.field_id == "ref_field")
        .collect::<Vec<_>>();
    assert_eq!(changes.len(), 2);
    assert!(changes.iter().any(|c| c.change == "label_changed"));
    assert!(changes.iter().any(|c| c.change == "position_changed"));
    for change in changes {
        assert_eq!(change.old_parent_id, None);
        assert_eq!(change.new_parent_id, None);
        assert_eq!(change.old_index, Some(2));
        assert_eq!(change.new_index, Some(0));
    }
    assert!(!preview.field_changes.iter().any(|c| c.field_id == "inner"));
}

#[test]
fn added_and_removed_field_impacts_include_only_the_existing_sides_positions() {
    let mut project = project("draft-position-lifecycle");
    let bytes = super::operations::grouped();
    let revision = Revision::default();
    let mutation = project.template_mutation_from_bytes(&bytes).unwrap();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    let added = preview
        .field_changes
        .iter()
        .find(|change| change.field_id == "field_1")
        .unwrap();
    assert_eq!(added.change, "added");
    assert_eq!(added.old_parent_id, None);
    assert_eq!(added.old_index, None);
    assert_eq!(added.new_parent_id.as_deref(), Some("left"));
    assert_eq!(added.new_index, Some(0));
    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let deletion = ProjectTemplateMutation::Delete {
        id: "project:groups".into(),
    };
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, deletion))
        .unwrap();
    let removed = preview
        .field_changes
        .iter()
        .find(|change| change.field_id == "field_1")
        .unwrap();
    assert_eq!(removed.change, "removed");
    assert_eq!(removed.old_parent_id.as_deref(), Some("left"));
    assert_eq!(removed.old_index, Some(0));
    assert_eq!(removed.new_parent_id, None);
    assert_eq!(removed.new_index, None);
}

use super::*;

#[test]
fn legacy_draft_without_reservation_metadata_still_reserves_bound_project_names() {
    let mut project = project("draft-reserved-legacy");
    register(&mut project, super::operations::grouped());
    let content = project.compile_object_search_snapshot();
    let mut document: Value = serde_json::from_slice(&super::operations::grouped()).unwrap();
    document["fields"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    let legacy: ProjectTemplateDraft = serde_json::from_value(json!({
        "source_bytes":serde_json::to_vec(&document).unwrap(), "existing_id":"project:groups"
    }))
    .unwrap();
    assert!(legacy.reserved_field_ids.is_empty());
    let added = project
        .edit_template_draft(
            &legacy,
            &Edit::AddField {
                parent_id: None,
                index: 3,
                field_type: FieldType::Text,
            },
            &content,
        )
        .unwrap();
    let field = &added.template.as_ref().unwrap().fields[3];
    assert_eq!(field.id, "field_2");
    assert_eq!(field.key.as_deref(), Some("property_2"));
    let document = value(&added.draft);
    assert!(document.get("reserved_field_ids").is_none());
    assert!(document.get("reserved_keys").is_none());
}

#[test]
fn invalid_raw_projection_and_failed_edits_do_not_advance_or_discard_identity_reservations() {
    let project = project("draft-reserved-bad-json");
    let content = project.compile_object_search_snapshot();
    let original = project.template_draft(Source::New, &content).unwrap();
    let first = project
        .edit_template_draft(
            &original.draft,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::Text,
            },
            &content,
        )
        .unwrap();
    let deleted = project
        .edit_template_draft(
            &first.draft,
            &Edit::DeleteField {
                field_id: "field_1".into(),
            },
            &content,
        )
        .unwrap();
    assert_eq!(deleted.draft.reserved_field_ids, ["field_1"]);
    assert_eq!(deleted.draft.reserved_keys, ["property_1"]);
    let mut bad_raw = deleted.draft.clone();
    bad_raw.source_bytes = b"{bad raw bytes".to_vec();
    let projection = project.project_template_draft_projection(&bad_raw, &content);
    assert_eq!(projection.draft, bad_raw);
    assert!(!projection.editable);
    assert!(project
        .edit_template_draft(
            &bad_raw,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::Text
            },
            &content
        )
        .is_err());
    assert_eq!(bad_raw.reserved_field_ids, ["field_1"]);
    assert_eq!(bad_raw.source_bytes, b"{bad raw bytes");
    assert!(project
        .edit_template_draft(
            &deleted.draft,
            &Edit::AddField {
                parent_id: None,
                index: 99,
                field_type: FieldType::Text
            },
            &content
        )
        .is_err());
    let added = project
        .edit_template_draft(
            &deleted.draft,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::Text,
            },
            &content,
        )
        .unwrap();
    assert_eq!(added.template.as_ref().unwrap().fields[0].id, "field_2");
    assert_eq!(deleted.draft.reserved_field_ids, ["field_1"]);
}

#[test]
fn reservation_sets_are_sorted_and_deduplicated_only_on_successful_edit() {
    let project = project("draft-reserved-normalization");
    let content = project.compile_object_search_snapshot();
    let mut draft = project.template_draft(Source::New, &content).unwrap().draft;
    draft.reserved_field_ids = vec!["field_9".into(), "field_1".into(), "field_9".into()];
    draft.reserved_keys = vec![
        "property_9".into(),
        "property_1".into(),
        "property_9".into(),
    ];
    assert_eq!(
        project
            .project_template_draft_projection(&draft, &content)
            .draft,
        draft
    );
    let edited = project
        .edit_template_draft(
            &draft,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::Text,
            },
            &content,
        )
        .unwrap();
    assert_eq!(
        edited.draft.reserved_field_ids,
        ["field_1", "field_2", "field_9"]
    );
    assert_eq!(
        edited.draft.reserved_keys,
        ["property_1", "property_2", "property_9"]
    );
    assert_eq!(draft.reserved_field_ids, ["field_9", "field_1", "field_9"]);
}

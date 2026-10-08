use super::*;

pub(super) fn grouped() -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schema_version":1, "id":"project:groups", "title":"分组",
        "applies_to":{"kind":"entity","entity_type":"place", "x-target":{"keep":true}},
        "x-root":{"keep":true},
        "fields":[
            {"id":"left","label":"左","type":"group","x-group":1,"fields":[
                {"id":"field_1","key":"property_1","label":"一","type":"text","required":false,"x-field":{"keep":true}},
                {"id":"retained","key":"retained","label":"保留","type":"text","required":false}
            ]},
            {"id":"right","label":"右","type":"group","fields":[
                {"id":"nested","label":"内","type":"group","fields":[
                    {"id":"inner","key":"inner","label":"内字段","type":"text","required":false}
                ]}
            ]},
            {"id":"ref_field","key":"destination","label":"目标","type":"object_ref","required":false,
             "target":{"kind":"entity","entity_type":"place","x-target-field":{"keep":true}},
             "default":{"kind":"entity","id":"harbor"},"x-ref":true}
        ]
    })).unwrap()
}

#[test]
fn add_nested_groups_allocates_global_ids_and_keys_and_metadata_preserves_extensions() {
    let project = project("draft-groups");
    let content = project.compile_object_search_snapshot();
    let original = draft(&project, grouped());
    let added = project
        .edit_template_draft(
            &original.draft,
            &Edit::AddField {
                parent_id: Some("left".into()),
                index: 1,
                field_type: FieldType::Group,
            },
            &content,
        )
        .unwrap();
    let tree = value(&added.draft);
    let group = &tree["fields"][0]["fields"][1];
    assert_eq!(group["type"], "group");
    assert_eq!(group["fields"][0]["id"], "field_2");
    assert_eq!(group["fields"][0]["key"], "property_2");
    let updated = project
        .edit_template_draft(
            &added.draft,
            &Edit::SetMetadata {
                title: "修改标题".into(),
                applies_to: DraftTarget {
                    kind: "world".into(),
                    entity_type: None,
                },
            },
            &content,
        )
        .unwrap();
    let updated = value(&updated.draft);
    assert_eq!(updated["x-root"]["keep"], true);
    assert_eq!(updated["applies_to"]["x-target"]["keep"], true);
    assert!(updated["applies_to"].get("entity_type").is_none());
    assert_eq!(updated["id"], "project:groups");
    let enum_added = project
        .edit_template_draft(
            &original.draft,
            &Edit::AddField {
                parent_id: None,
                index: 3,
                field_type: FieldType::Enum,
            },
            &content,
        )
        .unwrap();
    assert_eq!(enum_added.template.unwrap().fields[3].choices.len(), 2);
}

#[test]
fn move_uses_post_removal_indexes_and_rejects_cycles_or_empty_parent_atomically() {
    let project = project("draft-move");
    let content = project.compile_object_search_snapshot();
    let original = draft(&project, grouped());
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
    let tree = value(&moved.draft);
    assert_eq!(tree["fields"][0]["fields"].as_array().unwrap().len(), 1);
    assert_eq!(tree["fields"][1]["fields"][1]["id"], "field_1");
    assert_eq!(tree["fields"][1]["fields"][1]["x-field"]["keep"], true);
    let reordered = project
        .edit_template_draft(
            &moved.draft,
            &Edit::MoveField {
                field_id: "field_1".into(),
                parent_id: Some("right".into()),
                index: 0,
            },
            &content,
        )
        .unwrap();
    assert_eq!(
        value(&reordered.draft)["fields"][1]["fields"][0]["id"],
        "field_1"
    );
    for edit in [
        Edit::MoveField {
            field_id: "right".into(),
            parent_id: Some("right".into()),
            index: 0,
        },
        Edit::MoveField {
            field_id: "right".into(),
            parent_id: Some("nested".into()),
            index: 0,
        },
        Edit::MoveField {
            field_id: "inner".into(),
            parent_id: None,
            index: 0,
        },
        Edit::MoveField {
            field_id: "field_1".into(),
            parent_id: None,
            index: 99,
        },
        Edit::MoveField {
            field_id: "field_1".into(),
            parent_id: Some("retained".into()),
            index: 0,
        },
        Edit::DeleteField {
            field_id: "inner".into(),
        },
    ] {
        assert!(project
            .edit_template_draft(&original.draft, &edit, &content)
            .is_err());
        assert_eq!(original.draft.source_bytes, grouped());
    }
}

#[test]
fn update_validates_properties_preserves_field_identity_and_never_materializes_defaults() {
    let project = project("draft-props");
    let content = project.compile_object_search_snapshot();
    let baseline = project.content_baseline();
    let original = draft(&project, grouped());
    let mut properties = text_properties("property_renamed");
    properties.required = true;
    properties.default = Some(json!("仅提示"));
    let edited = project
        .edit_template_draft(
            &original.draft,
            &Edit::UpdateField {
                field_id: "field_1".into(),
                properties: properties.clone(),
            },
            &content,
        )
        .unwrap();
    let field = &value(&edited.draft)["fields"][0]["fields"][0];
    assert_eq!(field["id"], "field_1");
    assert_eq!(field["x-field"]["keep"], true);
    assert_eq!(field["default"], "仅提示");
    assert_eq!(project.content_baseline(), baseline);
    let mut invalids = vec![];
    let mut duplicate = properties.clone();
    duplicate.key = Some("retained".into());
    invalids.push(duplicate);
    let mut bad_key = properties.clone();
    bad_key.key = Some("two words".into());
    invalids.push(bad_key);
    let mut bad_label = properties.clone();
    bad_label.label = " ".into();
    invalids.push(bad_label);
    let mut bad_default = properties.clone();
    bad_default.field_type = FieldType::Number;
    invalids.push(bad_default);
    let mut bad_enum = properties.clone();
    bad_enum.field_type = FieldType::Enum;
    bad_enum.choices = vec!["same".into(), "same".into()];
    invalids.push(bad_enum);
    let mut group = properties.clone();
    group.field_type = FieldType::Group;
    invalids.push(group);
    for properties in invalids {
        assert!(project
            .edit_template_draft(
                &original.draft,
                &Edit::UpdateField {
                    field_id: "field_1".into(),
                    properties
                },
                &content
            )
            .is_err());
    }
    assert!(project
        .edit_template_draft(
            &original.draft,
            &Edit::UpdateField {
                field_id: "left".into(),
                properties
            },
            &content
        )
        .is_err());
    let mut reference = text_properties("destination");
    reference.field_type = FieldType::ObjectRef;
    reference.target = Some(DraftTarget {
        kind: "entity".into(),
        entity_type: None,
    });
    let edited = project
        .edit_template_draft(
            &original.draft,
            &Edit::UpdateField {
                field_id: "ref_field".into(),
                properties: reference,
            },
            &content,
        )
        .unwrap();
    assert_eq!(
        value(&edited.draft)["fields"][2]["target"]["x-target-field"]["keep"],
        true
    );
}

#[test]
fn replace_merges_unknown_fields_by_global_id_after_moves_and_never_revives_deletions() {
    let mut project = project("draft-merge");
    register(&mut project, grouped());
    let content = project.compile_object_search_snapshot();
    let existing = project
        .template_draft(
            Source::Existing {
                id: "project:groups".into(),
            },
            &content,
        )
        .unwrap();
    let mut document = value(&existing.draft);
    let mut moved = document["fields"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    moved.as_object_mut().unwrap().remove("x-field");
    let mut reference = document["fields"].as_array_mut().unwrap().remove(2);
    reference.as_object_mut().unwrap().remove("x-ref");
    reference["target"]
        .as_object_mut()
        .unwrap()
        .remove("x-target-field");
    document["fields"][1]["fields"]
        .as_array_mut()
        .unwrap()
        .push(moved);
    document["fields"][1]["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id":"new_parent", "label":"新父", "type":"group", "fields":[reference]
        }));
    document.as_object_mut().unwrap().remove("x-root");
    document["applies_to"]
        .as_object_mut()
        .unwrap()
        .remove("x-target");
    // 删除 left 组；其 retained 子字段和分组扩展不得复活。
    document["fields"].as_array_mut().unwrap().remove(0);
    register(&mut project, serde_json::to_vec(&document).unwrap());
    let result = project
        .template_draft(
            Source::Existing {
                id: "project:groups".into(),
            },
            &content,
        )
        .unwrap();
    let tree = value(&result.draft);
    assert_eq!(tree["x-root"]["keep"], true);
    assert_eq!(tree["applies_to"]["x-target"]["keep"], true);
    assert_eq!(tree["fields"][0]["fields"][1]["x-field"]["keep"], true);
    assert_eq!(tree["fields"][0]["fields"][2]["fields"][0]["x-ref"], true);
    assert_eq!(
        tree["fields"][0]["fields"][2]["fields"][0]["target"]["x-target-field"]["keep"],
        true
    );
    let text = String::from_utf8(result.draft.source_bytes).unwrap();
    assert!(!text.contains("retained"));
    assert!(!text.contains("x-group"));
}

#[test]
fn advanced_json_field_id_change_is_a_real_removed_and_added_field() {
    let mut project = project("draft-field-id");
    register(&mut project, template("project:field_ids", "text", "x-old"));
    let content = project.compile_object_search_snapshot();
    let existing = project
        .template_draft(
            Source::Existing {
                id: "project:field_ids".into(),
            },
            &content,
        )
        .unwrap();
    let mut document = value(&existing.draft);
    document["fields"][0]["id"] = "new_field_identity".into();
    document["fields"][0]
        .as_object_mut()
        .unwrap()
        .remove("x-old");
    let revision = Revision::default();
    let mutation = project
        .template_mutation_from_bytes(&serde_json::to_vec(&document).unwrap())
        .unwrap();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    assert!(preview
        .field_changes
        .iter()
        .any(|c| c.field_id == "note_field" && c.change == "removed"));
    assert!(preview
        .field_changes
        .iter()
        .any(|c| c.field_id == "new_field_identity" && c.change == "added"));
    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let result = project
        .template_draft(
            Source::Existing {
                id: "project:field_ids".into(),
            },
            &content,
        )
        .unwrap();
    assert!(value(&result.draft)["fields"][0].get("x-old").is_none());
}

#[test]
fn typed_scalar_transitions_and_explicit_group_deletion_preserve_children_until_confirmed_edit() {
    let project = project("draft-type-transitions");
    let content = project.compile_object_search_snapshot();
    let baseline = project.content_baseline();
    let mut current = draft(&project, grouped());
    for (field_type, default) in [
        (FieldType::Number, json!(42)),
        (FieldType::Boolean, json!(false)),
        (FieldType::Enum, json!("甲")),
        (FieldType::ObjectRef, json!({"kind":"entity","id":"harbor"})),
        (FieldType::Text, json!("提示")),
    ] {
        let mut properties = text_properties("property_1");
        properties.field_type = field_type;
        properties.required = true;
        properties.default = Some(default);
        if field_type == FieldType::Enum {
            properties.choices = vec!["甲".into(), "乙".into()];
        }
        if field_type == FieldType::ObjectRef {
            properties.target = Some(DraftTarget {
                kind: "entity".into(),
                entity_type: Some("place".into()),
            });
        }
        current = project
            .edit_template_draft(
                &current.draft,
                &Edit::UpdateField {
                    field_id: "field_1".into(),
                    properties,
                },
                &content,
            )
            .unwrap();
        let tree = value(&current.draft);
        assert_eq!(tree["fields"][0]["fields"][0]["id"], "field_1");
        assert_eq!(tree["fields"][0]["fields"][0]["type"], field_type.as_str());
        assert_eq!(tree["fields"][0]["fields"][0]["x-field"]["keep"], true);
    }
    let renamed = project
        .edit_template_draft(
            &current.draft,
            &Edit::UpdateField {
                field_id: "left".into(),
                properties: Properties {
                    label: "改组名".into(),
                    key: None,
                    field_type: FieldType::Group,
                    required: false,
                    choices: vec![],
                    target: None,
                    default: None,
                },
            },
            &content,
        )
        .unwrap();
    assert_eq!(
        value(&renamed.draft)["fields"][0]["fields"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let deleted = project
        .edit_template_draft(
            &renamed.draft,
            &Edit::DeleteField {
                field_id: "left".into(),
            },
            &content,
        )
        .unwrap();
    assert_eq!(value(&deleted.draft)["fields"].as_array().unwrap().len(), 2);
    assert!(!String::from_utf8(deleted.draft.source_bytes)
        .unwrap()
        .contains("property_1"));
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn deleting_then_adding_a_field_never_reuses_its_stable_identity_or_old_extensions() {
    let mut project = project("draft-no-recycled-identity");
    register(&mut project, grouped());
    let content = project.compile_object_search_snapshot();
    let original = project
        .template_draft(
            Source::Existing {
                id: "project:groups".into(),
            },
            &content,
        )
        .unwrap();
    let deleted = project
        .edit_template_draft(
            &original.draft,
            &Edit::DeleteField {
                field_id: "field_1".into(),
            },
            &content,
        )
        .unwrap();
    let added = project
        .edit_template_draft(
            &deleted.draft,
            &Edit::AddField {
                parent_id: None,
                index: 3,
                field_type: FieldType::Text,
            },
            &content,
        )
        .unwrap();
    let new_field = &added.template.as_ref().unwrap().fields[3];
    assert_ne!(new_field.id, "field_1");
    assert_ne!(new_field.key.as_deref(), Some("property_1"));
    let revision = Revision::default();
    let mutation = project.template_mutation_from_draft(&added.draft).unwrap();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    assert!(preview
        .field_changes
        .iter()
        .any(|change| change.field_id == "field_1" && change.change == "removed"));
    assert!(preview
        .field_changes
        .iter()
        .any(|change| change.field_id == new_field.id && change.change == "added"));
    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let after = project
        .template_draft(
            Source::Existing {
                id: "project:groups".into(),
            },
            &content,
        )
        .unwrap();
    assert!(value(&after.draft)["fields"][3].get("x-field").is_none());
}

#[test]
fn an_unapplied_draft_keeps_deleted_identity_reservations_when_serialized_and_resumed() {
    let project = project("draft-reservation-roundtrip");
    let content = project.compile_object_search_snapshot();
    let new = project.template_draft(Source::New, &content).unwrap();
    let first = project
        .edit_template_draft(
            &new.draft,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::Text,
            },
            &content,
        )
        .unwrap();
    let old_field = first.template.as_ref().unwrap().fields[0].clone();
    let deleted = project
        .edit_template_draft(
            &first.draft,
            &Edit::DeleteField {
                field_id: old_field.id.clone(),
            },
            &content,
        )
        .unwrap();
    let resumed: ProjectTemplateDraft =
        serde_json::from_slice(&serde_json::to_vec(&deleted.draft).unwrap()).unwrap();
    let added = project
        .edit_template_draft(
            &resumed,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::Text,
            },
            &content,
        )
        .unwrap();
    let new_field = &added.template.as_ref().unwrap().fields[0];
    assert_ne!(new_field.id, old_field.id);
    assert_ne!(new_field.key, old_field.key);
}

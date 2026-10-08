use super::*;
use worldline_core::project_templates::{ProjectTemplatePreview, TemplateImpactLimits};

fn preview(project: &Project, document: &Value) -> ProjectTemplatePreview {
    let revision = Revision::default();
    let mutation = project
        .template_mutation_from_bytes(&serde_json::to_vec(document).unwrap())
        .unwrap();
    project
        .preview_template_mutation(revision, &command(project, revision, mutation))
        .unwrap()
}

fn registered_properties(name: &str) -> (Project, Value) {
    let mut project = project(name);
    let bytes = template("project:properties", "text", "x-unknown");
    let value = serde_json::from_slice(&bytes).unwrap();
    register(&mut project, bytes);
    (project, value)
}

#[test]
fn label_only_change_carries_exact_frozen_property_snapshots() {
    let (mut project, mut document) = registered_properties("draft-snapshot-label");
    document["fields"][0]["label"] = "预览中的新标签".into();
    let impact = preview(&project, &document);
    let change = impact
        .field_changes
        .iter()
        .find(|change| change.field_id == "note_field")
        .unwrap();
    assert_eq!(change.change, "label_changed");
    let old = change.old_properties.as_ref().unwrap();
    let new = change.new_properties.as_ref().unwrap();
    assert_eq!(old.label, "记录");
    assert_eq!(new.label, "预览中的新标签");
    assert_eq!(old.key.as_deref(), Some("note"));
    assert_eq!(new.field_type, FieldType::Text);
    assert!(!old.required && !new.required);
    assert!(old.choices.is_empty() && new.choices.is_empty());
    assert!(old.target.is_none() && new.target.is_none());
    assert!(old.default.is_none() && new.default.is_none());
    document["fields"][0]["label"] = "预览以后继续改过的标签".into();
    project
        .set_authoring_document(
            &project.root.join(".world/templates/properties.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
    assert_eq!(change.old_properties.as_ref().unwrap().label, "记录");
    assert_eq!(
        change.new_properties.as_ref().unwrap().label,
        "预览中的新标签"
    );
    let snapshot = serde_json::to_value(new).unwrap();
    assert!(snapshot.get("id").is_none());
    assert!(snapshot.get("fields").is_none());
}

#[test]
fn default_choices_and_compound_constraint_edits_keep_both_complete_property_values() {
    let (project, original) = registered_properties("draft-snapshot-constraints");
    let mut changed_default = original.clone();
    changed_default["fields"][1]["default"] = "closed".into();
    let impact = preview(&project, &changed_default);
    let change = impact
        .field_changes
        .iter()
        .find(|change| change.field_id == "status_field")
        .unwrap();
    assert_eq!(change.change, "constraints_changed");
    assert_eq!(
        change.old_properties.as_ref().unwrap().default,
        Some(json!("active"))
    );
    assert_eq!(
        change.new_properties.as_ref().unwrap().default,
        Some(json!("closed"))
    );
    let mut changed_choices = original.clone();
    changed_choices["fields"][1]["choices"] = json!(["active", "paused"]);
    let impact = preview(&project, &changed_choices);
    let change = impact
        .field_changes
        .iter()
        .find(|change| change.field_id == "status_field")
        .unwrap();
    assert_eq!(
        change.old_properties.as_ref().unwrap().choices,
        ["active", "closed"]
    );
    assert_eq!(
        change.new_properties.as_ref().unwrap().choices,
        ["active", "paused"]
    );
    changed_choices["fields"][1]["label"] = "新状态".into();
    changed_choices["fields"][1]["required"] = true.into();
    changed_choices["fields"][1]["default"] = "paused".into();
    let impact = preview(&project, &changed_choices);
    let change = impact
        .field_changes
        .iter()
        .find(|change| change.field_id == "status_field")
        .unwrap();
    assert_eq!(change.change, "label_changed");
    let old = change.old_properties.as_ref().unwrap();
    let new = change.new_properties.as_ref().unwrap();
    assert_eq!(old.label, "状态");
    assert_eq!(new.label, "新状态");
    assert!(!old.required && new.required);
    assert_eq!(new.default, Some(json!("paused")));
    assert_eq!(new.choices, ["active", "paused"]);
    assert_eq!(new.field_type, FieldType::Enum);
}

#[test]
fn reference_constraint_snapshots_include_kind_entity_type_and_default_on_each_side() {
    let mut project = project("draft-snapshot-reference");
    register(&mut project, super::operations::grouped());
    let mut document: Value = serde_json::from_slice(&super::operations::grouped()).unwrap();
    document["fields"][2]["target"]
        .as_object_mut()
        .unwrap()
        .remove("entity_type");
    let impact = preview(&project, &document);
    let change = impact
        .field_changes
        .iter()
        .find(|change| change.field_id == "ref_field")
        .unwrap();
    let old = change.old_properties.as_ref().unwrap();
    let new = change.new_properties.as_ref().unwrap();
    assert_eq!(
        old.target.as_ref().unwrap().entity_type.as_deref(),
        Some("place")
    );
    assert_eq!(new.target.as_ref().unwrap().entity_type, None);
    assert_eq!(old.default, Some(json!({"kind":"entity","id":"harbor"})));
    assert_eq!(old.default, new.default);
    document["fields"][2]["target"]["kind"] = "relation".into();
    document["fields"][2]
        .as_object_mut()
        .unwrap()
        .remove("default");
    let impact = preview(&project, &document);
    let change = impact
        .field_changes
        .iter()
        .find(|change| change.field_id == "ref_field")
        .unwrap();
    assert_eq!(change.change, "constraints_changed");
    assert_eq!(
        change
            .old_properties
            .as_ref()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .kind,
        "entity"
    );
    assert_eq!(
        change
            .new_properties
            .as_ref()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .kind,
        "relation"
    );
    assert_eq!(change.new_properties.as_ref().unwrap().default, None);
}

#[test]
fn added_deleted_and_position_only_records_have_shallow_property_snapshots_on_correct_sides() {
    let mut project = project("draft-snapshot-lifecycle");
    let document: Value = serde_json::from_slice(&super::operations::grouped()).unwrap();
    let imported = preview(&project, &document);
    assert!(imported
        .field_changes
        .iter()
        .all(|change| change.old_properties.is_none() && change.new_properties.is_some()));
    let group = imported
        .field_changes
        .iter()
        .find(|change| change.field_id == "left")
        .unwrap()
        .new_properties
        .as_ref()
        .unwrap();
    assert_eq!(group.field_type, FieldType::Group);
    assert!(group.key.is_none() && group.default.is_none() && group.target.is_none());
    assert!(!group.required && group.choices.is_empty());
    assert!(serde_json::to_value(group).unwrap().get("fields").is_none());
    let mut revision = Revision::default();
    project
        .apply_template_mutation(&mut revision, imported)
        .unwrap();
    let mut reordered = document;
    let reference = reordered["fields"].as_array_mut().unwrap().remove(2);
    reordered["fields"]
        .as_array_mut()
        .unwrap()
        .insert(0, reference);
    let moved = preview(&project, &reordered);
    assert!(moved
        .field_changes
        .iter()
        .all(|change| change.change == "position_changed"
            && change.old_properties == change.new_properties));
    let deleted = project
        .preview_template_mutation(
            revision,
            &command(
                &project,
                revision,
                ProjectTemplateMutation::Delete {
                    id: "project:groups".into(),
                },
            ),
        )
        .unwrap();
    assert!(deleted
        .field_changes
        .iter()
        .all(|change| change.old_properties.is_some() && change.new_properties.is_none()));
}

#[test]
fn long_default_and_choice_snapshots_are_streamed_before_owned_report_allocation() {
    let project = project("draft-snapshot-budget");
    let long = "长属性".repeat(32_768);
    for field in [
        json!({"id":"f","key":"hint","label":"字段","type":"text","required":false,"default":long}),
        json!({"id":"f","key":"hint","label":"字段","type":"enum","required":false,"choices":[long]}),
    ] {
        let document = json!({"schema_version":1,"id":"project:long_snapshot","title":"属性快照",
            "applies_to":{"kind":"character"},"fields":[field]});
        let revision = Revision::default();
        let request = command(
            &project,
            revision,
            project
                .template_mutation_from_bytes(&serde_json::to_vec(&document).unwrap())
                .unwrap(),
        );
        let baseline = project.content_baseline();
        let error = project
            .preview_template_mutation_with_limits(
                revision,
                &request,
                &TemplateImpactLimits {
                    max_instances: 1,
                    max_field_values: 1,
                    max_output_bytes: 4096,
                },
            )
            .unwrap_err();
        assert!(error.starts_with("TemplateImpactLimit："), "{error}");
        assert_eq!(project.content_baseline(), baseline);
        let full = project
            .preview_template_mutation(revision, &request)
            .unwrap();
        let snapshot = full.field_changes[0].new_properties.as_ref().unwrap();
        assert!(
            snapshot
                .default
                .as_ref()
                .is_some_and(|value| value.as_str() == Some(long.as_str()))
                || snapshot.choices == [long.clone()]
        );
    }
}

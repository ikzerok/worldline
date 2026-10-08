use super::*;
use serde_json::{json, Value};
use worldline_core::project_templates::{
    ProjectTemplateDraft, ProjectTemplateDraftEdit as Edit, ProjectTemplateDraftProjection,
    ProjectTemplateDraftSource as Source, ProjectTemplateDraftTarget as DraftTarget,
    ProjectTemplateFieldProperties as Properties, ProjectTemplateFieldType as FieldType,
};
#[path = "drafts/completeness.rs"]
mod completeness;
#[path = "drafts/limits.rs"]
mod limits;
#[path = "drafts/operations.rs"]
mod operations;
#[path = "drafts/positions.rs"]
mod positions;
#[path = "drafts/property_snapshots.rs"]
mod property_snapshots;
#[path = "drafts/repair.rs"]
mod repair;
#[path = "drafts/reservations.rs"]
mod reservations;

fn draft(project: &Project, bytes: Vec<u8>) -> ProjectTemplateDraftProjection {
    project
        .template_draft(
            Source::Json { bytes },
            &project.compile_object_search_snapshot(),
        )
        .unwrap()
}

fn value(draft: &ProjectTemplateDraft) -> Value {
    serde_json::from_slice(&draft.source_bytes).unwrap()
}

fn register(project: &mut Project, bytes: Vec<u8>) {
    let mut revision = Revision::default();
    let mutation = project.template_mutation_from_bytes(&bytes).unwrap();
    let preview = project
        .preview_template_mutation(revision, &command(project, revision, mutation))
        .unwrap();
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
}

fn text_properties(key: &str) -> Properties {
    Properties {
        label: "改后标签".into(),
        key: Some(key.into()),
        field_type: FieldType::Text,
        required: false,
        choices: vec![],
        target: None,
        default: None,
    }
}

#[test]
fn new_and_all_builtin_copies_are_valid_unique_schema_one_without_project_changes() {
    let mut project = project("draft-new");
    let content = project.compile_object_search_snapshot();
    let baseline = project.content_baseline();
    let new = project.template_draft(Source::New, &content).unwrap();
    assert!(new.editable);
    let template = new.template.as_ref().unwrap();
    assert_eq!(template.id, "project:new_template_1");
    assert_eq!(template.title, "新工程模板");
    assert_eq!(template.applies_to.kind, "entity");
    assert_eq!(template.applies_to_entity_type.as_deref(), Some("place"));
    assert!(template.fields.is_empty());
    for builtin in &worldline_core::content_templates::builtin_templates().templates {
        let copy = project
            .template_draft(
                Source::Copy {
                    id: builtin.id.clone(),
                },
                &content,
            )
            .unwrap();
        assert!(copy.editable, "{:?}", copy.diagnostics);
        assert_eq!(
            copy.template.as_ref().unwrap().id,
            "project:template_copy_1"
        );
        assert_eq!(
            copy.template.as_ref().unwrap().fields.len(),
            builtin.fields.len()
        );
        assert_eq!(value(&copy.draft)["schema_version"], 1);
        assert!(value(&copy.draft).get("extensions").is_some());
        assert_eq!(
            value(&copy.draft)["fields"][0]["widget"],
            builtin.fields[0].widget
        );
    }
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    register(&mut project, new.draft.source_bytes);
    assert_eq!(
        project
            .template_draft(Source::New, &content)
            .unwrap()
            .template
            .unwrap()
            .id,
        "project:new_template_2"
    );
    let copy = project
        .template_draft(
            Source::Copy {
                id: "template_place".into(),
            },
            &content,
        )
        .unwrap();
    register(&mut project, copy.draft.source_bytes);
    assert_eq!(
        project
            .template_draft(
                Source::Copy {
                    id: "project:template_copy_1".into()
                },
                &content
            )
            .unwrap()
            .template
            .unwrap()
            .id,
        "project:template_copy_2"
    );
}

#[test]
fn raw_projection_preserves_bytes_and_separates_repairable_input_from_read_only_versions() {
    let project = project("draft-raw");
    let valid = template("project:raw", "text", "custom");
    let projection = draft(&project, valid.clone());
    assert!(projection.editable);
    assert_eq!(projection.draft.source_bytes, valid);
    for bytes in [
        b"{bad".to_vec(),
        b"{\"schema_version\":1,\"schema_version\":1}".to_vec(),
        vec![255, 0],
    ] {
        let projection = draft(&project, bytes.clone());
        assert!(!projection.editable);
        assert!(!projection.read_only);
        assert_eq!(projection.draft.source_bytes, bytes);
        assert!(project
            .edit_template_draft(
                &projection.draft,
                &Edit::AddField {
                    parent_id: None,
                    index: 0,
                    field_type: FieldType::Text,
                },
                &project.compile_object_search_snapshot()
            )
            .is_err());
    }
    for (key, replacement) in [
        ("schema_version", json!(2)),
        ("required_features", json!(["vendor.future"])),
    ] {
        let mut document: Value = serde_json::from_slice(&valid).unwrap();
        document[key] = replacement;
        let bytes = serde_json::to_vec(&document).unwrap();
        let projection = draft(&project, bytes.clone());
        assert!(!projection.editable);
        assert!(projection.read_only);
        assert_eq!(projection.draft.source_bytes, bytes);
    }
}

#[test]
fn existing_identity_is_bound_and_new_json_identity_requires_explicit_selection() {
    let mut project = project("draft-identity");
    register(
        &mut project,
        template("project:original", "text", "extension"),
    );
    let content = project.compile_object_search_snapshot();
    let mut existing = project
        .template_draft(
            Source::Existing {
                id: "project:original".into(),
            },
            &content,
        )
        .unwrap()
        .draft;
    assert!(
        matches!(project.template_mutation_from_draft(&existing).unwrap(), ProjectTemplateMutation::Replace { id, .. } if id == "project:original")
    );
    let mut document = value(&existing);
    document["id"] = "project:changed".into();
    existing.source_bytes = serde_json::to_vec(&document).unwrap();
    assert!(project
        .template_mutation_from_draft(&existing)
        .unwrap_err()
        .contains("ID 已改变"));
    assert!(
        !project
            .project_template_draft_projection(&existing, &content)
            .editable
    );
    assert!(
        matches!(project.template_mutation_from_bytes(&existing.source_bytes).unwrap(), ProjectTemplateMutation::Import { id, .. } if id == "project:changed")
    );
    assert!(project
        .template_repair_mutation_from_draft(&existing)
        .is_err());
}

#[test]
fn draft_operations_use_loaded_buffers_after_disk_disappears_and_roundtrip_public_dtos() {
    let mut project = project("draft-no-disk");
    register(
        &mut project,
        template("project:memory", "text", "extension"),
    );
    let content = project.compile_object_search_snapshot();
    let baseline = project.content_baseline();
    fs::remove_dir_all(&project.root).unwrap();
    let existing = project
        .template_draft(
            Source::Existing {
                id: "project:memory".into(),
            },
            &content,
        )
        .unwrap();
    let edited = project
        .edit_template_draft(
            &existing.draft,
            &Edit::SetMetadata {
                title: "纯缓冲".into(),
                applies_to: DraftTarget {
                    kind: "world".into(),
                    entity_type: None,
                },
            },
            &content,
        )
        .unwrap();
    assert_eq!(edited.template.as_ref().unwrap().title, "纯缓冲");
    let copy = project
        .template_draft(
            Source::Copy {
                id: "project:memory".into(),
            },
            &content,
        )
        .unwrap();
    assert!(copy.editable);
    let serialized = serde_json::to_vec(&edited).unwrap();
    let decoded: ProjectTemplateDraftProjection = serde_json::from_slice(&serialized).unwrap();
    assert_eq!(decoded, edited);
    let edit = Edit::MoveField {
        field_id: "note_field".into(),
        parent_id: None,
        index: 2,
    };
    assert_eq!(
        serde_json::from_value::<Edit>(serde_json::to_value(&edit).unwrap()).unwrap(),
        edit
    );
    assert_eq!(project.content_baseline(), baseline);
}

#[test]
fn reference_draft_edits_follow_language_and_character_dual_capability_guards() {
    for (language, supported) in [("1.9", false), ("1.10", true)] {
        let project =
            project_with_object_refs("draft-ref", language, false, "event start\n  -> END\n");
        let content = project.compile_object_search_snapshot();
        let new = project.template_draft(Source::New, &content).unwrap();
        let result = project.edit_template_draft(
            &new.draft,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::ObjectRef,
            },
            &content,
        );
        assert_eq!(result.is_ok(), supported);
        if let Ok(result) = result {
            assert_eq!(
                result.template.unwrap().fields[0]
                    .target
                    .as_ref()
                    .unwrap()
                    .kind,
                "entity"
            );
        }
    }
    let mut project = project_with_object_refs(
        "draft-character",
        "1.13",
        true,
        "character navigator\nevent start\n  -> END\n",
    );
    let path = project.root.join(".world/project.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push("content.character_refs.v1".into());
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    project.refresh().unwrap();
    let content = project.compile_object_search_snapshot();
    let new = project.template_draft(Source::New, &content).unwrap();
    let with_ref = project
        .edit_template_draft(
            &new.draft,
            &Edit::AddField {
                parent_id: None,
                index: 0,
                field_type: FieldType::ObjectRef,
            },
            &content,
        )
        .unwrap();
    let id = with_ref.template.as_ref().unwrap().fields[0].id.clone();
    let mut properties = text_properties("home");
    properties.field_type = FieldType::ObjectRef;
    properties.target = Some(DraftTarget {
        kind: "character".into(),
        entity_type: None,
    });
    properties.default = Some(json!({"kind":"character", "id":"navigator"}));
    let character = project
        .edit_template_draft(
            &with_ref.draft,
            &Edit::UpdateField {
                field_id: id.clone(),
                properties: properties.clone(),
            },
            &content,
        )
        .unwrap();
    assert_eq!(
        value(&character.draft)["required_features"],
        json!(["content.character_refs.v1"])
    );
    properties.default = Some(json!({"kind":"character", "id":"missing"}));
    assert!(project
        .edit_template_draft(
            &with_ref.draft,
            &Edit::UpdateField {
                field_id: id,
                properties
            },
            &content
        )
        .is_err());
}

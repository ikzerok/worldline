use super::*;
use worldline_core::presentation_commands::Revision;
use worldline_core::project_templates::{ProjectTemplateMutation, TemplateCommand};

fn template(kind: &str, id: &str) -> Vec<u8> {
    serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version":1,"id":"project:ship","title":"航船资料",
        "required_features":["content.character_refs.v1"],
        "applies_to":{"kind":"entity","entity_type":"ship"},
        "fields":[{"id":"crew_group","label":"船员","type":"group","fields":[{
            "id":"captain_id","key":"captain","label":"船长","type":"object_ref","required":true,
            "target":{"kind":"character"},"default":{"kind":kind,"id":id}
        }]}], "extensions":{"plain":"lin"}
    }))
    .unwrap()
}
fn import(project: &Project, bytes: Vec<u8>) -> TemplateCommand {
    TemplateCommand {
        expected_revision: Revision::default(),
        expected_baseline: project.content_baseline(),
        check_integrity: true,
        mutation: ProjectTemplateMutation::Import {
            id: "project:ship".into(),
            document: bytes,
        },
    }
}
#[test]
fn character_template_default_is_a_strong_ref_and_renames_with_the_person() {
    let mut f = fixture("template");
    let mut revision = Revision::default();
    let command = import(&f.project, template("character", "lin"));
    let preview = f
        .project
        .preview_template_mutation(revision, &command)
        .unwrap();
    assert_eq!(f.project.content_baseline(), command.expected_baseline);
    f.project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let index = f.project.template_index();
    assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
    let target = TargetRef::new("character", "lin");
    let impact = f.project.deletion_impact(&target);
    assert_eq!(impact.template_references.len(), 1);
    assert!(impact.complete);
    assert!(!impact.can_delete());
    let plan = f.project.plan_rename_target(&target, "navigator").unwrap();
    assert!(plan.changes.iter().any(|change| change.kind == "authoring"));
    f.project.apply_rename_plan(&plan).unwrap();
    let index = f.project.template_index();
    assert!(index.diagnostics.is_empty());
    let document = &index.projects["project:ship"];
    let value = document.source_document.as_ref().unwrap();
    assert_eq!(
        value["fields"][0]["fields"][0]["default"]["id"],
        "navigator"
    );
    assert_eq!(value["extensions"]["plain"], "lin");
}

#[test]
fn character_template_rejects_missing_wrong_kind_subtype_and_missing_document_feature() {
    let f = fixture("template-invalid");
    for bytes in [template("character", "absent"), template("entity", "boat")] {
        let command = import(&f.project, bytes);
        assert!(f
            .project
            .preview_template_mutation(Revision::default(), &command)
            .is_err());
    }
    for remove_feature in [true, false] {
        let mut value: serde_json::Value =
            serde_json::from_slice(&template("character", "lin")).unwrap();
        if remove_feature {
            value["required_features"] = serde_json::json!([]);
        } else {
            value["fields"][0]["fields"][0]["target"]["entity_type"] = "person".into();
        }
        let command = import(&f.project, serde_json::to_vec(&value).unwrap());
        assert!(f
            .project
            .preview_template_mutation(Revision::default(), &command)
            .is_err());
    }
}

#[test]
fn unsupported_manifest_capability_preserves_raw_bytes_and_refuses_edits() {
    let f = fixture("future");
    let path = f.root.join(".world/project.json");
    let bytes = fs::read(&path).unwrap();
    let future = String::from_utf8(bytes)
        .unwrap()
        .replace("content.character_refs.v1", "content.character_refs.v999")
        .into_bytes();
    fs::write(&path, &future).unwrap();
    let mut project = Project::open(&f.root).unwrap();
    assert_eq!(project.authoring_document(&path).unwrap().bytes(), future);
    assert!(project.authoring_document(&path).unwrap().is_read_only());
    assert!(!project.authoring_diagnostics().is_empty());
    assert!(project
        .plan_rename_target(&TargetRef::new("character", "lin"), "navigator")
        .is_err());
    assert!(project
        .set_authoring_document(&path, b"{}".to_vec())
        .is_err());
    assert_eq!(fs::read(path).unwrap(), future);
}

#[test]
fn reader_field_selection_never_discloses_unselected_character_or_biography() {
    let f = fixture("reader");
    let selection: worldline_core::reader_export::ReaderExportSelection =
        serde_json::from_value(serde_json::json!({
            "schema_version":2,"required_features":["reader.fields.v1"],"site_title":"船只",
            "objects":[{"kind":"entity","id":"boat"}],"manuscripts":[],"attachments":[],
            "fields":[{"target":{"kind":"entity","id":"boat"},"keys":["captain"]}]
        }))
        .unwrap();
    let preview = f.project.preview_reader_export(&selection).unwrap();
    let files = f
        .project
        .build_reader_export(&selection, &preview.plan_digest)
        .unwrap();
    let all = files
        .iter()
        .map(|(path, bytes)| format!("{} {}", path.display(), String::from_utf8_lossy(bytes)))
        .collect::<String>();
    assert!(!all.contains("林舟"));
    assert!(!all.contains("character:lin"));
    assert!(!all.contains("\"lin\""));
}

#[test]
fn template_ref_target_constraint_change_is_previewed_without_rewriting_instances() {
    let mut f = fixture("template-target-change");
    let mut revision = Revision::default();
    let mut original: serde_json::Value =
        serde_json::from_slice(&template("character", "lin")).unwrap();
    original["fields"][0]["fields"][0]
        .as_object_mut()
        .unwrap()
        .remove("default");
    let command = import(&f.project, serde_json::to_vec(&original).unwrap());
    let preview = f
        .project
        .preview_template_mutation(revision, &command)
        .unwrap();
    f.project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let baseline = f.project.content_baseline();
    let sources = f.project.sources();
    let mut changed = original;
    let field = &mut changed["fields"][0]["fields"][0];
    field["target"] = serde_json::json!({"kind":"entity","entity_type":"ship"});
    let command = TemplateCommand {
        expected_revision: revision,
        expected_baseline: baseline.clone(),
        check_integrity: true,
        mutation: ProjectTemplateMutation::Replace {
            id: "project:ship".into(),
            document: serde_json::to_vec(&changed).unwrap(),
        },
    };
    let preview = f
        .project
        .preview_template_mutation(revision, &command)
        .unwrap();
    assert_eq!(preview.field_changes.len(), 1);
    assert!(preview
        .field_changes
        .iter()
        .any(|change| change.field_id == "captain_id" && change.change == "constraints_changed"));
    assert!(preview
        .instances
        .iter()
        .flat_map(|instance| &instance.fields)
        .any(|field| field.field_id == "captain_id" && field.type_matches == Some(false)));
    assert_eq!(f.project.content_baseline(), baseline);
    assert_eq!(f.project.sources(), sources);
}

#[test]
fn importing_character_template_never_enables_missing_object_ref_capability() {
    let mut f = fixture("template-missing-object-feature");
    f.project
        .set_text(
            &f.root.join("world.wl"),
            "entity boat kind ship\nevent start\n  -> END\n".into(),
        )
        .unwrap();
    f.project
        .set_text(&f.root.join("people.wl"), "character lin\n".into())
        .unwrap();
    let manifest = f.root.join(".world/project.json");
    f.project.set_authoring_document(&manifest, br#"{"schema_version":1,"language_version":"1.13","required_features":["content.character_refs.v1","content.templates.v1"],"templates":{}}"#.to_vec()).unwrap();
    let baseline = f.project.content_baseline();
    let command = import(&f.project, template("character", "lin"));
    let error = f
        .project
        .preview_template_mutation(Revision::default(), &command)
        .unwrap_err();
    assert!(
        error.contains("TPL005") && error.contains("content.object_refs.v1"),
        "{error}"
    );
    assert_eq!(f.project.content_baseline(), baseline);
    assert!(!f.project.compile_options().object_refs);
    let mut old: serde_json::Value = serde_json::from_slice(&template("character", "lin")).unwrap();
    old["fields"][0]["fields"][0]["target"]["kind"] = "entity".into();
    old["fields"][0]["fields"][0]["default"] = serde_json::json!({"kind":"entity","id":"boat"});
    let command = import(&f.project, serde_json::to_vec(&old).unwrap());
    let preview = f
        .project
        .preview_template_mutation(Revision::default(), &command)
        .unwrap();
    let mut revision = Revision::default();
    f.project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    assert!(
        f.project.compile_options().object_refs,
        "既有entity模板导入自动能力行为保留"
    );
}

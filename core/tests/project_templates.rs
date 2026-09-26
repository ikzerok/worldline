use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::presentation_commands::Revision;
use worldline_core::project::Project;
use worldline_core::project_templates::{
    ProjectTemplateMutation, ProjectTemplateValueState, TemplateCommand,
};

fn root(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-project-templates-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn project(name: &str) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        concat!(
            "world setting as \"雾港\"\n",
            "  property motto = \"keep this prose\"\n",
            "entity harbor kind place as \"港口\"\n",
            "  description \"不可由模板重写的长正文。\"\n",
            "  property note = \"custom instance value\"\n",
            "  property status = \"active\"\n",
            "  property empty_note = \"\"\n",
            "character navigator as \"领航员\"\n",
            "  property home = \"user-owned alias-like value\"\n",
            "event arrival as \"抵达\"\n",
            "  -> END\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"harbor","language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","content.templates.v1"],"templates":{},"extension":{"keep":"manifest"}}"#,
    )
    .unwrap();
    Project::open(&root).unwrap()
}

fn project_with_object_refs(
    name: &str,
    language: &str,
    enable_refs: bool,
    source: &str,
) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    let mut features = vec!["content.entities.v1"];
    if enable_refs {
        features.push("content.object_refs.v1");
    }
    let features = serde_json::to_string(&features).unwrap();
    fs::write(
        root.join(".world/project.json"),
        format!(
            r#"{{"schema_version":1,"project_id":"harbor","language_version":"{language}","entry":"world.wl","required_features":{features},"templates":{{}}}}"#
        ),
    )
    .unwrap();
    Project::open(&root).unwrap()
}

fn template(id: &str, note_type: &str, unknown: &str) -> Vec<u8> {
    format!(
        r#"{{
  "schema_version": 1,
  "id": "{id}",
  "title": "港口档案",
  "applies_to": {{"kind":"entity", "entity_type":"place"}},
  "fields": [
    {{"id":"note_field", "key":"note", "label":"记录", "type":"{note_type}", "required":false, "{unknown}":{{"keep":true}}}},
    {{"id":"status_field", "key":"status", "label":"状态", "type":"enum", "required":false, "choices":["active","closed"], "default":"active"}},
    {{"id":"empty_field", "key":"empty_note", "label":"空记录", "type":"text", "required":false}},
    {{"id":"missing_field", "key":"not_present", "label":"尚未填写", "type":"text", "required":false}}
  ],
  "extensions": {{"vendor":"preserve me"}}
}}"#
    )
    .into_bytes()
}

fn command(
    project: &Project,
    revision: Revision,
    mutation: ProjectTemplateMutation,
) -> TemplateCommand {
    TemplateCommand {
        expected_revision: revision,
        expected_baseline: project.content_baseline(),
        mutation,
        check_integrity: true,
    }
}

fn import(id: &str, value: Vec<u8>) -> ProjectTemplateMutation {
    ProjectTemplateMutation::Import {
        id: id.into(),
        document: value,
    }
}

#[test]
fn template_index_keeps_the_sixteen_builtin_ids_and_isolates_bad_registered_files() {
    let mut project = project("bad-doc");
    let manifest = project.root.join(".world/project.json");
    let manifest_bytes = fs::read(&manifest).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    value["templates"]["project:broken"] = serde_json::json!(".world/templates/broken.json");
    fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
    fs::create_dir_all(project.root.join(".world/templates")).unwrap();
    fs::write(project.root.join(".world/templates/broken.json"), b"{bad").unwrap();
    project.refresh().unwrap();

    let index = project.template_index();
    assert_eq!(index.builtins.len(), 16);
    assert!(index
        .builtins
        .iter()
        .any(|item| item.id == "template_place"));
    let broken = index.projects.get("project:broken").unwrap();
    assert!(broken.read_only);
    assert!(broken.diagnostics.iter().any(|d| d.code.starts_with("TPL")));
    assert_eq!(
        project
            .authoring_document(&project.root.join(".world/templates/broken.json"))
            .unwrap()
            .bytes(),
        b"{bad"
    );
}

#[test]
fn import_preview_apply_save_and_reopen_never_write_instance_properties() {
    let mut project = project("lifecycle");
    let content = project.compile();
    assert!(!content.has_errors(), "{:?}", content.diagnostics);
    let fingerprint = content.analysis.fingerprint;
    let source_before = fs::read(project.root.join("world.wl")).unwrap();
    let baseline = project.content_baseline();
    let template_path = project.root.join(".world/templates/route.json");
    let revision = Revision::default();
    let request = command(
        &project,
        revision,
        import(
            "project:route",
            template("project:route", "text", "x-vendor"),
        ),
    );
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    assert!(!template_path.exists());
    let place = preview
        .instances
        .iter()
        .find(|item| item.target.id == "harbor")
        .unwrap();
    assert!(place
        .fields
        .iter()
        .any(|field| { field.key == "note" && field.state == ProjectTemplateValueState::Set }));
    assert!(place.fields.iter().any(|field| {
        field.key == "status" && field.state == ProjectTemplateValueState::Default
    }));
    assert!(place.fields.iter().any(|field| {
        field.key == "empty_note" && field.state == ProjectTemplateValueState::Empty
    }));
    assert!(place.fields.iter().any(|field| {
        field.key == "not_present" && field.state == ProjectTemplateValueState::Missing
    }));

    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    assert_eq!(revision.presentation_generation, 1);
    assert_eq!(
        fs::read(project.root.join("world.wl")).unwrap(),
        source_before
    );
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let bytes = project.authoring_document(&template_path).unwrap().bytes();
    let document: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(document["fields"][0]["x-vendor"]["keep"], true);
    assert_eq!(document["extensions"]["vendor"], "preserve me");
    assert_eq!(
        project.template_index().projects["project:route"]
            .template
            .as_ref()
            .unwrap()
            .title,
        "港口档案"
    );

    let root = project.root.clone();
    project.save().unwrap();
    let mut reopened = Project::open(&root).unwrap();
    assert!(reopened
        .template_index()
        .projects
        .contains_key("project:route"));
    assert_eq!(reopened.compile().analysis.fingerprint, fingerprint);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn replacement_previews_type_changes_preserves_unknowns_and_rejects_stale_baselines() {
    let mut project = project("replace");
    let revision = Revision::default();
    let first = command(
        &project,
        revision,
        import(
            "project:route",
            template("project:route", "text", "x-vendor"),
        ),
    );
    let first = project.preview_template_mutation(revision, &first).unwrap();
    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, first)
        .unwrap();

    let request = command(
        &project,
        revision,
        ProjectTemplateMutation::Replace {
            id: "project:route".into(),
            document: template("project:route", "number", "x-new"),
        },
    );
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(preview.field_changes.iter().any(|change| {
        change.field_id == "note_field"
            && change.old_type == Some("text".into())
            && change.new_type == Some("number".into())
    }));
    let harbor = preview
        .instances
        .iter()
        .find(|item| item.target.id == "harbor")
        .unwrap();
    assert!(harbor.fields.iter().any(|field| {
        field.template_state == "proposed"
            && field.key == "note"
            && field.state == ProjectTemplateValueState::TypeMismatch
    }));
    assert!(preview
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "TPL006"));
    let world = project.documents[&project.root.join("world.wl")]
        .text
        .clone();
    project
        .set_text(&project.root.join("world.wl"), format!("{world}\n"))
        .unwrap();
    assert!(project
        .apply_template_mutation(&mut revision, preview)
        .is_err());
    assert_eq!(
        project.template_index().projects["project:route"]
            .template
            .as_ref()
            .unwrap()
            .fields[0]
            .field_type,
        "text"
    );

    let request = command(
        &project,
        revision,
        ProjectTemplateMutation::Replace {
            id: "project:route".into(),
            document: template("project:route", "number", "x-new"),
        },
    );
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let document: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&project.root.join(".world/templates/route.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(document["fields"][0]["x-vendor"]["keep"], true);
    assert_eq!(document["fields"][0]["x-new"]["keep"], true);
}

#[test]
fn field_rename_preview_keeps_old_and_proposed_value_states_separate() {
    let mut project = project("rename-field");
    let mut revision = Revision::default();
    let create = command(
        &project,
        revision,
        import(
            "project:route",
            template("project:route", "text", "x-vendor"),
        ),
    );
    let preview = project
        .preview_template_mutation(revision, &create)
        .unwrap();
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();

    let mut replacement = String::from_utf8(template("project:route", "text", "x-new")).unwrap();
    replacement = replacement.replace("\"key\":\"note\"", "\"key\":\"summary\"");
    let request = command(
        &project,
        revision,
        ProjectTemplateMutation::Replace {
            id: "project:route".into(),
            document: replacement.into_bytes(),
        },
    );
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(preview.field_changes.iter().any(|change| {
        change.field_id == "note_field"
            && change.change == "renamed"
            && change.old_key.as_deref() == Some("note")
            && change.new_key.as_deref() == Some("summary")
    }));
    let harbor = preview
        .instances
        .iter()
        .find(|item| item.target.id == "harbor")
        .unwrap();
    assert!(harbor.fields.iter().any(|field| {
        field.template_state == "current"
            && field.key == "note"
            && field.state == ProjectTemplateValueState::Set
    }));
    assert!(harbor.fields.iter().any(|field| {
        field.template_state == "proposed"
            && field.key == "summary"
            && field.state == ProjectTemplateValueState::Missing
    }));
}

#[test]
fn importing_first_template_adds_required_root_capability() {
    let mut project = project_with_object_refs(
        "upgrade-template-feature",
        "1.10",
        false,
        "event arrival\n  -> END\n",
    );
    let mut revision = Revision::default();
    let request = command(
        &project,
        revision,
        import(
            "project:route",
            template("project:route", "text", "x-vendor"),
        ),
    );
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&project.root.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert!(manifest["required_features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "content.templates.v1"));
}

#[test]
fn unknown_template_required_capability_is_read_only_and_preserves_original_bytes() {
    let root = root("unknown-capability");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/templates")).unwrap();
    fs::write(root.join("world.wl"), "event arrival\n  -> END\n").unwrap();
    let bytes = br#"{"schema_version":1,"id":"project:future","title":"future","applies_to":{"kind":"entity"},"required_features":["vendor.future.v2"],"fields":[]}"#;
    fs::write(root.join(".world/templates/future.json"), bytes).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"future","language_version":"1.10","entry":"world.wl","required_features":["content.templates.v1"],"templates":{"project:future":".world/templates/future.json"}}"#,
    ).unwrap();
    let project = Project::open(&root).unwrap();
    let entry = &project.template_index().projects["project:future"];
    assert!(entry.read_only);
    assert_eq!(entry.source_bytes, bytes);
    assert!(entry
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "TPL002"));
}

#[test]
fn delete_only_unregisters_and_tombstones_the_template_document() {
    let mut project = project("delete");
    let revision = Revision::default();
    let request = command(
        &project,
        revision,
        import(
            "project:route",
            template("project:route", "text", "x-vendor"),
        ),
    );
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let source_before = project.documents[&project.root.join("world.wl")]
        .text
        .clone();
    let delete = command(
        &project,
        revision,
        ProjectTemplateMutation::Delete {
            id: "project:route".into(),
        },
    );
    let preview = project
        .preview_template_mutation(revision, &delete)
        .unwrap();
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    assert!(!project
        .template_index()
        .projects
        .contains_key("project:route"));
    assert!(project
        .authoring_document(&project.root.join(".world/templates/route.json"))
        .unwrap()
        .is_deleted());
    assert_eq!(
        project.documents[&project.root.join("world.wl")].text,
        source_before
    );
}

#[test]
fn duplicate_field_identity_and_invalid_target_have_source_line_diagnostics() {
    let mut project = project("validation");
    let invalid = r#"{
  "schema_version":1,
  "id":"project:bad",
  "title":"坏模板",
  "applies_to":{"kind":"not_a_target"},
  "fields":[
    {"id":"same","key":"one","label":"一","type":"text","required":false},
    {"id":"same","key":"two","label":"二","type":"text","required":false}
  ]
}"#;
    let revision = Revision::default();
    let request = command(
        &project,
        revision,
        import("project:bad", invalid.as_bytes().to_vec()),
    );
    let error = project
        .preview_template_mutation(revision, &request)
        .unwrap_err();
    assert!(error.contains("TPL"));
    assert!(project.template_index().projects.is_empty());

    let template_path = project.root.join(".world/templates/bad.json");
    fs::create_dir_all(template_path.parent().unwrap()).unwrap();
    let duplicate_document = invalid.replace("not_a_target", "entity");
    fs::write(&template_path, duplicate_document.as_bytes()).unwrap();
    let target_path = project.root.join(".world/templates/target.json");
    let target_document = invalid.replace("project:bad", "project:target");
    fs::write(&target_path, target_document.as_bytes()).unwrap();
    let manifest_path = project.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(fs::read(&manifest_path).unwrap().as_slice()).unwrap();
    manifest["templates"]["project:bad"] = serde_json::json!(".world/templates/bad.json");
    manifest["templates"]["project:target"] = serde_json::json!(".world/templates/target.json");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    project.refresh().unwrap();
    let indexed = project.template_index();
    let duplicate = indexed.projects["project:bad"]
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPL004" && diagnostic.message.contains("重复"))
        .unwrap();
    assert_eq!(duplicate.span.line, 8);
    let invalid_target = indexed.projects["project:target"]
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPL004" && diagnostic.message.contains("适用目标"))
        .unwrap();
    assert_eq!(invalid_target.span.line, 5);
}

#[test]
fn project_template_ids_are_namespaced_and_builtin_replacement_is_rejected() {
    let project = project("namespace");
    let revision = Revision::default();
    let request = command(
        &project,
        revision,
        import(
            "template_place",
            template("template_place", "text", "x-vendor"),
        ),
    );
    assert!(project
        .preview_template_mutation(revision, &request)
        .is_err());
    let request = command(
        &project,
        revision,
        ProjectTemplateMutation::Replace {
            id: "template_place".into(),
            document: template("template_place", "text", "x-vendor"),
        },
    );
    assert!(project
        .preview_template_mutation(revision, &request)
        .is_err());
}

#[test]
fn schema_examples_are_json_and_template_contract_accepts_only_the_valid_example() {
    let schema: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../spec/schemas/project-template.schema.json"
    ))
    .unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );

    let project = project("schema-examples");
    let valid = include_bytes!("../../spec/examples/project-template.valid.json").to_vec();
    let revision = Revision::default();
    let request = command(&project, revision, import("project:guide", valid));
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);

    let invalid = include_bytes!("../../spec/examples/project-template.invalid.json").to_vec();
    let request = command(&project, revision, import("project:invalid", invalid));
    let error = project
        .preview_template_mutation(revision, &request)
        .unwrap_err();
    assert!(error.contains("TPL004"), "{error}");
}

#[test]
fn template_validation_rejects_schema_shape_errors_before_import() {
    let mut project = project("schema-shape-errors");
    let manifest_path = project.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("content.object_refs.v1"));
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    project.refresh().unwrap();

    let revision = Revision::default();
    for (id, document) in [
        (
            "project:bad_type",
            r#"{"schema_version":1,"id":"project:bad_type","title":"bad","applies_to":{"kind":"entity","entity_type":7},"fields":[]}"#,
        ),
        (
            "project:bad_default",
            r#"{"schema_version":1,"id":"project:bad_default","title":"bad","applies_to":{"kind":"entity"},"fields":[{"id":"home","key":"home","label":"Home","type":"object_ref","required":false,"target":{"kind":"entity"},"default":{"kind":"entity","id":"harbor","extra":true}}]}"#,
        ),
    ] {
        let request = command(&project, revision, import(id, document.as_bytes().to_vec()));
        assert!(project
            .preview_template_mutation(revision, &request)
            .is_err());
    }
}

#[test]
fn template_object_ref_target_kind_is_restricted_with_source_location() {
    let source = concat!(
        "character keeper as \"守灯人\"\n",
        "entity harbor kind place as \"港口\"\n",
        "  description \"雾港\"\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut project = project_with_object_refs("unsupported-template-target", "1.10", true, source);
    let manifest_path = project.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("content.templates.v1"));
    manifest["templates"]["project:bad_target"] =
        serde_json::json!(".world/templates/bad_target.json");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::create_dir_all(project.root.join(".world/templates")).unwrap();
    fs::write(
        project.root.join(".world/templates/bad_target.json"),
        r#"{
  "schema_version":1,
  "id":"project:bad_target",
  "title":"不支持的目标",
  "applies_to":{"kind":"entity","entity_type":"place"},
  "fields":[
    {"id":"keeper_field","key":"keeper","label":"守灯人","type":"object_ref","required":false,"target":{"kind":"character"}}
  ]
}"#,
    )
    .unwrap();
    project.refresh().unwrap();

    let entry = &project.template_index().projects["project:bad_target"];
    let diagnostic = entry
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "TPL004")
        .unwrap();
    assert_eq!(diagnostic.span.line, 7);
}

#[test]
fn explicit_object_refs_are_tracked_rewritten_and_do_not_promote_plain_strings() {
    let source = concat!(
        "entity harbor kind place as \"港口\"\n",
        "  description \"雾港\"\n",
        "entity keeper kind person as \"守灯人\"\n",
        "  description \"资料\"\n",
        "  property home = ref(\"entity\", \"harbor\")\n",
        "  property note = \"entity:harbor\"\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut project = project_with_object_refs("object-ref-rename", "1.10", true, source);
    let before = project.compile();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    assert_eq!(
        before
            .analysis
            .catalog
            .references_to(&worldline_core::TargetRef::new("entity", "harbor"))
            .iter()
            .filter(|reference| reference.kind == "对象属性引用")
            .count(),
        1
    );
    let impact = project.deletion_impact(&worldline_core::TargetRef::new("entity", "harbor"));
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete());
    assert!(impact
        .content_references
        .iter()
        .any(|reference| reference.kind == "对象属性引用"));

    let fingerprint = before.analysis.fingerprint;
    let plan = project
        .plan_rename_target(
            &worldline_core::TargetRef::new("entity", "harbor"),
            "beacon",
        )
        .unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let after = project.compile();
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    assert_eq!(after.analysis.fingerprint, fingerprint);
    let source = project.document(&project.root.join("world.wl")).unwrap();
    assert!(source.contains("ref(\"entity\", \"beacon\")"));
    assert!(source.contains("property note = \"entity:harbor\""));
    assert!(after
        .analysis
        .catalog
        .references_to(&worldline_core::TargetRef::new("entity", "harbor"))
        .is_empty());
}

#[test]
fn renaming_an_entity_rewrites_its_self_reference_without_blocking_deletion_analysis() {
    let source = concat!(
        "entity harbor kind place as \"港口\"\n",
        "  description \"雾港\"\n",
        "  property origin = ref(\"entity\", \"harbor\")\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut project = project_with_object_refs("object-ref-self-rename", "1.10", true, source);
    let target = worldline_core::TargetRef::new("entity", "harbor");
    let before = project.compile();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    let impact = project.deletion_impact(&target);
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(impact.can_delete(), "{:?}", impact.content_references);

    let fingerprint = before.analysis.fingerprint;
    let plan = project.plan_rename_target(&target, "beacon").unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let after = project.compile();
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    assert_eq!(after.analysis.fingerprint, fingerprint);
    assert!(project
        .document(&project.root.join("world.wl"))
        .unwrap()
        .contains("ref(\"entity\", \"beacon\")"));
}

#[test]
fn relation_object_refs_are_protected_and_rewritten_with_relation_ids() {
    let source = concat!(
        "entity harbor kind place as \"港口\"\n",
        "entity keeper kind person as \"守灯人\"\n",
        "relation_type watches as \"守望\"\n",
        "  direction directed\n",
        "  from entity\n",
        "  to entity\n",
        "relation_def watch_1 type watches from entity keeper to entity harbor\n",
        "  property source = ref(\"relation\", \"watch_1\")\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut project = project_with_object_refs("relation-object-ref-rename", "1.10", true, source);
    let target = worldline_core::TargetRef::new("relation", "watch_1");
    let before = project.compile();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    assert!(before
        .analysis
        .catalog
        .references_to(&target)
        .iter()
        .any(|reference| reference.kind == "对象属性引用"));
    assert!(project.deletion_impact(&target).can_delete());

    let fingerprint = before.analysis.fingerprint;
    let plan = project.plan_rename_target(&target, "watch_2").unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let after = project.compile();
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    assert_eq!(after.analysis.fingerprint, fingerprint);
    let source = project.document(&project.root.join("world.wl")).unwrap();
    assert!(source.contains("relation_def watch_2 type"));
    assert!(source.contains("ref(\"relation\", \"watch_2\")"));
}

#[test]
fn renaming_rewrites_template_object_ref_defaults_but_preserves_unknown_objects() {
    let source = concat!(
        "entity harbor kind place as \"港口\"\n",
        "  description \"雾港\"\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut project = project_with_object_refs("object-ref-template-rename", "1.10", true, source);
    let manifest_path = project.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("content.templates.v1"));
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    project.refresh().unwrap();

    let document = r#"{
  "schema_version":1,
  "id":"project:links",
  "title":"链接",
  "applies_to":{"kind":"entity","entity_type":"place"},
  "fields":[
    {"id":"home_field","key":"home","label":"居所","type":"object_ref","required":false,"target":{"kind":"entity","entity_type":"place"},"default":{"kind":"entity","id":"harbor"},"x-vendor":{"kind":"entity","id":"harbor"}}
  ],
  "x-vendor":{"example":{"kind":"entity","id":"harbor"}}
}"#
    .as_bytes()
    .to_vec();
    let revision = Revision::default();
    let request = command(&project, revision, import("project:links", document));
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();

    let target = worldline_core::TargetRef::new("entity", "harbor");
    let plan = project.plan_rename_target(&target, "beacon").unwrap();
    project.apply_rename_plan(&plan).unwrap();
    let document: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&project.root.join(".world/templates/links.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(document["fields"][0]["default"]["id"], "beacon");
    assert_eq!(document["fields"][0]["x-vendor"]["id"], "harbor");
    assert_eq!(document["x-vendor"]["example"]["id"], "harbor");
}

#[test]
fn object_ref_syntax_requires_language_and_manifest_capability_and_reports_missing_targets() {
    let source = concat!(
        "character keeper as \"守灯人\"\n",
        "  property home = ref(\"entity\", \"missing\")\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut no_feature = project_with_object_refs("object-ref-no-feature", "1.10", false, source);
    assert!(no_feature
        .compile()
        .diagnostics
        .iter()
        .any(|item| item.code == "P004"));

    let mut old_language = project_with_object_refs("object-ref-old-language", "1.9", true, source);
    assert!(old_language
        .compile()
        .diagnostics
        .iter()
        .any(|item| item.code == "P004"));

    let mut missing_target = project_with_object_refs("object-ref-missing", "1.10", true, source);
    assert!(missing_target
        .compile()
        .diagnostics
        .iter()
        .any(|item| item.code == "A214"));

    let unsupported_kind = concat!(
        "character keeper as \"守灯人\"\n",
        "  property friend = ref(\"character\", \"keeper\")\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut unsupported = project_with_object_refs(
        "object-ref-unsupported-kind",
        "1.10",
        true,
        unsupported_kind,
    );
    assert!(unsupported
        .compile()
        .diagnostics
        .iter()
        .any(|item| item.code == "P004"));
}

#[test]
fn object_ref_template_defaults_resolve_and_serialize_as_target_refs() {
    let source = concat!(
        "entity harbor kind place as \"港口\"\n",
        "  description \"雾港\"\n",
        "entity keeper kind person as \"守灯人\"\n",
        "  description \"资料\"\n",
        "  property home = ref(\"entity\", \"harbor\")\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut project = project_with_object_refs("object-ref-template", "1.10", true, source);
    let document = r#"{
  "schema_version":1,
  "id":"project:people",
  "title":"人物关系",
  "applies_to":{"kind":"entity","entity_type":"person"},
  "fields":[
    {"id":"home_field","key":"home","label":"居所","type":"object_ref","required":false,"target":{"kind":"entity","entity_type":"place"},"default":{"kind":"entity","id":"harbor"}}
  ]
}"#
    .as_bytes()
    .to_vec();
    let revision = Revision::default();
    let request = command(&project, revision, import("project:people", document));
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(preview.instances.iter().any(|instance| {
        instance.target.id == "keeper"
            && instance.fields.iter().any(|field| {
                field.key == "home" && field.state == ProjectTemplateValueState::Default
            })
    }));
    let encoded = serde_json::to_value(worldline_core::ast::PropertyValue::Ref(
        worldline_core::TargetRef::new("entity", "harbor"),
    ))
    .unwrap();
    assert_eq!(encoded, serde_json::json!({"kind":"entity","id":"harbor"}));

    let mut revision = revision;
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    assert!(!project.template_index().projects["project:people"].read_only);
}

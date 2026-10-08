use super::*;
use worldline_core::Severity;

#[test]
fn source_errors_make_template_preview_incomplete_and_cannot_be_overridden_to_apply() {
    for source in [
        "include \"missing.wl\"\nevent start\n  -> END\n",
        "world !\nevent start\n  -> END\n",
    ] {
        let mut project = project("draft-incomplete-source");
        project
            .set_text(&project.entry.clone(), source.into())
            .unwrap();
        let content = project.compile_object_search_snapshot();
        assert!(content.has_errors());
        let baseline = project.content_baseline();
        let draft = project.template_draft(Source::New, &content).unwrap();
        let mutation = project.template_mutation_from_draft(&draft.draft).unwrap();
        let mut revision = Revision::default();
        let preview = project
            .preview_template_mutation(revision, &command(&project, revision, mutation))
            .unwrap();
        assert!(!preview.complete);
        assert!(preview
            .incomplete_reason
            .as_ref()
            .unwrap()
            .contains("编译错误"));
        assert!(preview
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error));
        assert!(project
            .apply_template_mutation(&mut revision, preview.clone())
            .is_err());
        let mut forged = preview;
        forged.complete = true;
        forged.incomplete_reason = None;
        forged.diagnostics.clear();
        assert!(project
            .apply_template_mutation(&mut revision, forged)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(revision, Revision::default());
    }
}

#[test]
fn skipping_integrity_is_explicitly_incomplete_even_when_sources_compile_successfully() {
    let mut project = project("draft-incomplete-unchecked");
    let revision = Revision::default();
    let mut request = command(
        &project,
        revision,
        import(
            "project:unchecked",
            template("project:unchecked", "text", "x"),
        ),
    );
    request.check_integrity = false;
    let mut preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(!preview.complete);
    assert!(preview
        .incomplete_reason
        .as_ref()
        .unwrap()
        .contains("未执行"));
    assert!(!preview
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error));
    let baseline = project.content_baseline();
    preview.complete = true;
    let mut next = revision;
    assert!(project.apply_template_mutation(&mut next, preview).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(next, revision);
}

fn scoped_template(
    kind: &str,
    entity_type: Option<&str>,
    field_type: &str,
    title: &str,
) -> Vec<u8> {
    let mut applies_to = json!({"kind":kind});
    if let Some(entity_type) = entity_type {
        applies_to["entity_type"] = entity_type.into();
    }
    serde_json::to_vec(&json!({
        "schema_version":1,"id":"project:scope","title":title,"applies_to":applies_to,
        "fields":[{"id":"shared_field","key":"note","label":"记录","type":field_type,"required":false}]
    })).unwrap()
}

fn scoped_project(name: &str) -> Project {
    let mut project = project(name);
    project
        .set_text(
            &project.entry.clone(),
            concat!(
                "character author\n  property note = \"仅人物文本\"\n",
                "entity harbor kind place\n  property note = 7\n",
                "entity item kind thing\n  property note = true\n",
                "event start\n  -> END\n",
            )
            .into(),
        )
        .unwrap();
    assert!(!project.compile_object_search_snapshot().has_errors());
    project
}

#[test]
fn kind_scope_change_reports_only_applicable_sides_and_core_summary_without_false_mismatches() {
    let mut project = scoped_project("draft-scope-kind");
    register(
        &mut project,
        scoped_template("character", None, "text", "人物记录"),
    );
    let sources = project.sources();
    let fingerprint = project
        .compile_object_search_snapshot()
        .analysis
        .fingerprint;
    let bytes = scoped_template("entity", Some("place"), "number", "地点记录");
    let mutation = project.template_mutation_from_bytes(&bytes).unwrap();
    let mut revision = Revision::default();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    assert!(preview.complete);
    assert_eq!(preview.incomplete_reason, None);
    let current = preview.current_template.as_ref().unwrap();
    let proposed = preview.proposed_template.as_ref().unwrap();
    assert_eq!(current.id, "project:scope");
    assert_eq!(current.title, "人物记录");
    assert_eq!(current.applies_to.kind, "character");
    assert_eq!(current.applies_to_entity_type, None);
    assert_eq!(proposed.title, "地点记录");
    assert_eq!(proposed.applies_to.kind, "entity");
    assert_eq!(proposed.applies_to_entity_type.as_deref(), Some("place"));
    assert_eq!(preview.instances.len(), 2);
    let author = preview
        .instances
        .iter()
        .find(|row| row.target.id == "author")
        .unwrap();
    assert!(author.current_applicable);
    assert!(!author.proposed_applicable);
    assert!(author
        .fields
        .iter()
        .all(|field| field.template_state == "current" && field.type_matches == Some(true)));
    let harbor = preview
        .instances
        .iter()
        .find(|row| row.target.id == "harbor")
        .unwrap();
    assert!(!harbor.current_applicable);
    assert!(harbor.proposed_applicable);
    assert!(harbor
        .fields
        .iter()
        .all(|field| field.template_state == "proposed" && field.type_matches == Some(true)));
    assert!(!preview
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "TPL006"));
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    assert_eq!(project.sources(), sources);
    assert_eq!(
        project
            .compile_object_search_snapshot()
            .analysis
            .fingerprint,
        fingerprint
    );
}

#[test]
fn entity_type_scope_change_marks_entering_and_leaving_rows_and_import_delete_summary_sides() {
    let mut project = scoped_project("draft-scope-entity-type");
    let mut revision = Revision::default();
    let bytes = scoped_template("entity", Some("place"), "number", "地点");
    let mutation = project.template_mutation_from_bytes(&bytes).unwrap();
    let import_preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    assert!(import_preview.current_template.is_none());
    assert!(import_preview.proposed_template.is_some());
    assert!(import_preview
        .instances
        .iter()
        .all(|row| !row.current_applicable && row.proposed_applicable));
    project
        .apply_template_mutation(&mut revision, import_preview)
        .unwrap();
    let mutation = project
        .template_mutation_from_bytes(&scoped_template("entity", Some("thing"), "boolean", "物品"))
        .unwrap();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    let harbor = preview
        .instances
        .iter()
        .find(|row| row.target.id == "harbor")
        .unwrap();
    let item = preview
        .instances
        .iter()
        .find(|row| row.target.id == "item")
        .unwrap();
    assert!(harbor.current_applicable && !harbor.proposed_applicable);
    assert!(!item.current_applicable && item.proposed_applicable);
    assert!(harbor
        .fields
        .iter()
        .all(|field| field.template_state == "current"));
    assert!(item
        .fields
        .iter()
        .all(|field| field.template_state == "proposed"));
    assert!(!preview
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "TPL006"));
    project
        .apply_template_mutation(&mut revision, preview)
        .unwrap();
    let mutation = ProjectTemplateMutation::Delete {
        id: "project:scope".into(),
    };
    let deleted = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    assert!(deleted.current_template.is_some());
    assert!(deleted.proposed_template.is_none());
    assert!(deleted
        .instances
        .iter()
        .all(|row| row.current_applicable && !row.proposed_applicable));
}

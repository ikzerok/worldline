use super::*;

fn broken_project(name: &str, bytes: &[u8]) -> Project {
    let mut project = project(name);
    let path = project.root.join(".world/project.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["templates"]["project:broken"] = ".world/templates/broken.json".into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::create_dir_all(project.root.join(".world/templates")).unwrap();
    fs::write(project.root.join(".world/templates/broken.json"), bytes).unwrap();
    project.refresh().unwrap();
    project
}

fn repair_draft(project: &Project) -> ProjectTemplateDraft {
    let mut draft = project
        .template_draft(
            Source::Existing {
                id: "project:broken".into(),
            },
            &project.compile_object_search_snapshot(),
        )
        .unwrap()
        .draft;
    draft.source_bytes = template("project:broken", "text", "replacement-extension");
    draft
}

#[test]
fn duplicate_key_repair_requires_explicit_mode_and_warning_then_save_reopens() {
    for old in [
        br#"{"schema_version":1,"schema_version":1,"id":"project:broken","x":{"a":1,"a":2}}"#
            .to_vec(),
        br#"{"schema_version":1,"id":"project:broken","id":"project:broken"}"#.to_vec(),
    ] {
        let mut project = broken_project("draft-repair", &old);
        let baseline = project.content_baseline();
        let before = project.clone();
        let draft = repair_draft(&project);
        let mut revision = Revision::default();
        let ordinary = project.template_mutation_from_draft(&draft).unwrap();
        assert!(project
            .preview_template_mutation(revision, &command(&project, revision, ordinary))
            .is_err());
        let repair = project.template_repair_mutation_from_draft(&draft).unwrap();
        assert!(
            matches!(&repair, ProjectTemplateMutation::RepairInvalid { id, .. } if id == "project:broken")
        );
        let preview = project
            .preview_template_mutation(revision, &command(&project, revision, repair))
            .unwrap();
        assert!(preview.diagnostics.iter().any(|d| d.code == "TPL007"
            && d.severity == worldline_core::Severity::Warning
            && d.message.contains("完整新文")
            && d.message.contains("先复制原文")));
        assert_eq!(project.content_baseline(), baseline);
        let path = project.root.join(".world/templates/broken.json");
        assert_eq!(project.authoring_document(&path).unwrap().bytes(), old);
        project
            .apply_template_mutation(&mut revision, preview)
            .unwrap();
        assert_eq!(
            project.authoring_document(&path).unwrap().bytes(),
            draft.source_bytes
        );
        assert_eq!(fs::read(&path).unwrap(), old);
        assert!(project.restore(before));
        assert_eq!(project.authoring_document(&path).unwrap().bytes(), old);
        let repair = project.template_repair_mutation_from_draft(&draft).unwrap();
        let preview = project
            .preview_template_mutation(revision, &command(&project, revision, repair))
            .unwrap();
        project
            .apply_template_mutation(&mut revision, preview)
            .unwrap();
        project.save().unwrap();
        let reopened = Project::open(&project.root).unwrap();
        assert_eq!(
            reopened.authoring_document(&path).unwrap().bytes(),
            draft.source_bytes
        );
        assert!(reopened.template_index().projects["project:broken"]
            .template
            .is_some());
    }
}

#[test]
fn invalid_repair_candidates_and_stale_disk_or_content_leave_old_raw_bytes_untouched() {
    let old = br#"{"schema_version":1,"id":"project:broken","id":"project:broken"}"#;
    let mut project = broken_project("draft-repair-fail", old);
    let revision = Revision::default();
    let baseline = project.content_baseline();
    for bytes in [
        b"{bad candidate".to_vec(),
        template("project:wrong_id", "text", "x"),
        br#"{"schema_version":1,"schema_version":1}"#.to_vec(),
    ] {
        let mutation = ProjectTemplateMutation::RepairInvalid {
            id: "project:broken".into(),
            document: bytes,
        };
        assert!(project
            .preview_template_mutation(revision, &command(&project, revision, mutation))
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
    let draft = repair_draft(&project);
    let mutation = project.template_repair_mutation_from_draft(&draft).unwrap();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation.clone()))
        .unwrap();
    let path = project.root.join(".world/templates/broken.json");
    fs::write(&path, b"external edit").unwrap();
    let mut next = revision;
    assert!(project.apply_template_mutation(&mut next, preview).is_err());
    assert_eq!(next, revision);
    assert_eq!(project.authoring_document(&path).unwrap().bytes(), old);
    fs::write(&path, old).unwrap();
    let preview = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap();
    project
        .set_authoring_document(&path, b"{different local broken text".to_vec())
        .unwrap();
    assert!(project.apply_template_mutation(&mut next, preview).is_err());
    assert_eq!(
        project.authoring_document(&path).unwrap().bytes(),
        b"{different local broken text"
    );
}

#[test]
fn explicit_invalid_repair_cannot_bypass_unknown_schema_features_or_language_guards() {
    for (key, replacement) in [
        ("schema_version", json!(2)),
        ("required_features", json!(["future.vendor"])),
    ] {
        let mut old: Value =
            serde_json::from_slice(&template("project:broken", "text", "x")).unwrap();
        old[key] = replacement;
        let old = serde_json::to_vec(&old).unwrap();
        let project = broken_project("draft-repair-protected", &old);
        let draft = repair_draft(&project);
        let mutation = project.template_repair_mutation_from_draft(&draft).unwrap();
        let revision = Revision::default();
        assert!(project
            .preview_template_mutation(revision, &command(&project, revision, mutation))
            .is_err());
        assert!(project
            .template_draft(
                Source::Copy {
                    id: "project:broken".into()
                },
                &project.compile_object_search_snapshot()
            )
            .is_err());
    }
    let mut project = broken_project("draft-repair-language", &super::operations::grouped());
    // 登记 ID 先对齐，随后降为旧语言；对象引用字段的 TPL005 不属于 JSON 救援。
    let manifest = project.root.join(".world/project.json");
    let mut manifest_value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    manifest_value["language_version"] = "1.9".into();
    fs::write(&manifest, serde_json::to_vec(&manifest_value).unwrap()).unwrap();
    let mut document: Value = serde_json::from_slice(&super::operations::grouped()).unwrap();
    document["id"] = "project:broken".into();
    fs::write(
        project.root.join(".world/templates/broken.json"),
        serde_json::to_vec(&document).unwrap(),
    )
    .unwrap();
    project.refresh().unwrap();
    let draft = repair_draft(&project);
    let mutation = project.template_repair_mutation_from_draft(&draft).unwrap();
    let revision = Revision::default();
    assert!(project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .is_err());
}

#[test]
fn draft_request_dtos_reject_unknown_machine_fields_without_rejecting_document_extensions() {
    assert!(serde_json::from_value::<Source>(json!({"kind":"new","unrecognized":true})).is_err());
    assert!(serde_json::from_value::<Edit>(
        json!({"kind":"delete_field","field_id":"field_1","unrecognized":true})
    )
    .is_err());
    assert!(serde_json::from_value::<DraftTarget>(
        json!({"kind":"entity","entity_type":null,"unrecognized":true})
    )
    .is_err());
    let project = project("draft-json-extension");
    assert!(
        draft(
            &project,
            template("project:extended", "text", "unrecognized")
        )
        .editable
    );
}

#[test]
fn invalid_repair_requires_complete_supported_metadata_across_all_duplicate_members() {
    for old in [
        br#"{"schema_version":1,"id":"project:broken","fields":["#.as_slice(),
        br#"{"id":"project:broken","id":"project:broken"}"#.as_slice(),
        br#"{"schema_version":2,"schema_version":1,"id":"project:broken"}"#.as_slice(),
        br#"{"schema_version":1,"schema_version":2,"id":"project:broken"}"#.as_slice(),
        br#"{"schema_version":1,"required_features":["vendor.future"],"required_features":[]}"#
            .as_slice(),
        br#"{"schema_version":1,"required_features":[],"required_features":["vendor.future"]}"#
            .as_slice(),
        br#"{"schema_version":1,"required_features":null,"required_features":[]}"#.as_slice(),
        br#"{"schema_version":1,"required_features":[42],"required_features":[]}"#.as_slice(),
    ] {
        let project = broken_project("draft-repair-metadata", old);
        let baseline = project.content_baseline();
        let draft = repair_draft(&project);
        let mutation = project.template_repair_mutation_from_draft(&draft).unwrap();
        let revision = Revision::default();
        let error = project
            .preview_template_mutation(revision, &command(&project, revision, mutation))
            .unwrap_err();
        assert!(error.contains("保护元数据"), "{error}");
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(
            project
                .authoring_document(&project.root.join(".world/templates/broken.json"))
                .unwrap()
                .bytes(),
            old
        );
    }
}

#[test]
fn invalid_repair_cannot_hide_reference_protections_in_duplicate_field_members() {
    let old = br#"{"schema_version":1,"id":"project:broken","fields":[{"id":"f","type":"object_ref","type":"text","target":{"kind":"character","kind":"entity"}}]}"#;
    let project = broken_project("draft-repair-refs", old);
    let draft = repair_draft(&project);
    let mutation = project.template_repair_mutation_from_draft(&draft).unwrap();
    let revision = Revision::default();
    let error = project
        .preview_template_mutation(revision, &command(&project, revision, mutation))
        .unwrap_err();
    assert!(error.contains("对象引用"), "{error}");
}

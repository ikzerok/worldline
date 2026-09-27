use super::*;

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

#[test]
fn importing_object_ref_template_adds_capability_without_writing_instances() {
    let source = concat!(
        "entity harbor kind place as \"港口\"\n",
        "  description \"雾港\"\n",
        "event arrival\n",
        "  -> END\n",
    );
    let mut project =
        project_with_object_refs("object-ref-template-capability", "1.10", false, source);
    let original_source = fs::read(project.root.join("world.wl")).unwrap();
    let document = r#"{
  "schema_version":1,
  "id":"project:places",
  "title":"地点",
  "applies_to":{"kind":"entity","entity_type":"place"},
  "fields":[
    {"id":"home_field","key":"home","label":"关联地点","type":"object_ref","required":false,"target":{"kind":"entity","entity_type":"place"},"default":{"kind":"entity","id":"harbor"}}
  ]
}"#
    .as_bytes()
    .to_vec();
    let revision = Revision::default();
    let request = command(&project, revision, import("project:places", document));
    let preview = project
        .preview_template_mutation(revision, &request)
        .unwrap();
    assert!(preview.instances.iter().any(|instance| {
        instance.target.id == "harbor"
            && instance.fields.iter().any(|field| {
                field.key == "home" && field.state == ProjectTemplateValueState::Missing
            })
    }));
    let mut revision = revision;
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
        .any(|feature| feature == "content.object_refs.v1"));
    assert_eq!(
        fs::read(project.root.join("world.wl")).unwrap(),
        original_source
    );
}

#[test]
fn importing_object_ref_template_requires_language_110() {
    let project = project_with_object_refs(
        "object-ref-template-old-language",
        "1.9",
        false,
        "event arrival\n  -> END\n",
    );
    let document = r#"{
  "schema_version":1,
  "id":"project:people",
  "title":"人物关系",
  "applies_to":{"kind":"entity"},
  "fields":[
    {"id":"home_field","key":"home","label":"居所","type":"object_ref","required":false,"target":{"kind":"entity"}}
  ]
}"#
    .as_bytes()
    .to_vec();
    let revision = Revision::default();
    let request = command(&project, revision, import("project:people", document));
    let error = project
        .preview_template_mutation(revision, &request)
        .unwrap_err();
    assert!(error.contains("TPL005"), "{error}");
}

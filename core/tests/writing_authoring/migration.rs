use super::*;

#[test]
fn entity_requires_explicit_enable_and_migrates_manifest_source_and_link_as_one_transaction() {
    let manifest = r#"{ "schema_version":1, "language_version" : "1.9", "required_features":[], "extension":{"number":1e2,"escaped":"\u6797"} }"#;
    let (work, mut project) = Workspace::new(&[("world.wl", SOURCE)], Some(manifest));
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    buffer.replace_source(SOURCE.replace("你看见", "纳入当前全文：你看见"));
    let input = buffer_state(&buffer);
    let mut command = request(&project, &buffer, "林😀", entity(&project.entry));
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let before = project.clone();
    let rejected = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .err()
        .unwrap();
    assert!(rejected.contains("1.10"), "{rejected}");
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer_state(&buffer), input);
    assert_eq!(work.bytes(), disk);
    command.enable_entities = true;
    let plan = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    assert!(plan.can_apply);
    assert!(plan.request().enable_entities);
    let migration = plan.migration.as_ref().unwrap();
    assert_eq!(migration.current_language, LanguageVersion::V1_9);
    assert_eq!(migration.target_language, LanguageVersion::V1_10);
    assert!(migration.can_apply);
    assert!(migration.manifest_changed);
    assert!(migration
        .required_features_after
        .contains(&"content.entities.v1".into()));
    assert!(migration
        .diagnostics_after
        .iter()
        .all(|diagnostic| diagnostic.severity != Severity::Error));
    assert_eq!(migration.manifest_bytes_before(), Some(manifest.as_bytes()));
    assert!(std::str::from_utf8(migration.manifest_bytes_after())
        .unwrap()
        .contains(r#""extension":{"number":1e2,"escaped":"\u6797"}"#));
    let manifest_path = work.0.join(".world/project.json");
    assert_eq!(plan.changes.len(), 2);
    assert!(plan
        .changes
        .iter()
        .any(|change| change.path == manifest_path));
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(work.bytes(), disk);
    let mutations: [fn(&mut WritingAuthoringPlan); 3] = [
        |plan| plan.migration.as_mut().unwrap().runtime_fingerprint_after ^= 1,
        |plan| plan.migration.as_mut().unwrap().target_language = LanguageVersion::V1_13,
        |plan| {
            plan.migration
                .as_mut()
                .unwrap()
                .required_features_after
                .clear()
        },
    ];
    for mutate in mutations {
        let mut changed = plan.clone();
        mutate(&mut changed);
        assert!(project
            .apply_writing_authoring(&[buffer.clone()], &changed)
            .is_err());
        assert_eq!(project.content_baseline(), baseline);
    }
    let mut omitted = plan.clone();
    omitted.migration = None;
    assert!(project
        .apply_writing_authoring(&[buffer.clone()], &omitted)
        .is_err());
    project
        .apply_writing_authoring(&[buffer.clone()], &plan)
        .unwrap();
    assert_eq!(project.language_version(), "1.10");
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("entity lin kind place"));
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("纳入当前全文：你看见[[entity:lin|林😀]]"));
    assert_eq!(
        project.authoring_document(&manifest_path).unwrap().bytes(),
        migration.manifest_bytes_after()
    );
    assert_eq!(buffer_state(&buffer), input);
    assert_eq!(work.bytes(), disk);
    let compiled = project.compile_object_search_snapshot();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(
        compiled.analysis.fingerprint,
        plan.runtime_fingerprint_after
    );
    assert_eq!(compiled.analysis.catalog.text_links.len(), 1);
    assert_eq!(
        compiled.analysis.catalog.text_links[0].target,
        TargetRef::new("entity", "lin")
    );
    let after = project.clone();
    assert!(project.restore(before));
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
    assert_eq!(
        project.authoring_document(&manifest_path).unwrap().bytes(),
        manifest.as_bytes()
    );
    assert!(project.restore(after));
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.language_version(), "1.10");
    assert_eq!(
        reopened
            .compile_object_search_snapshot()
            .analysis
            .catalog
            .text_links[0]
            .target,
        TargetRef::new("entity", "lin")
    );
    assert!(reopened
        .document(&reopened.entry)
        .unwrap()
        .contains("纳入当前全文"));
}

#[test]
fn first_entity_manifest_is_created_only_in_applied_memory_until_save() {
    let (work, mut project) = Workspace::new(&[("world.wl", SOURCE)], None);
    let buffer = project.open_writing_buffer(&start()).unwrap();
    let mut command = request(&project, &buffer, "林😀", entity(&project.entry));
    command.enable_entities = true;
    let baseline = project.content_baseline();
    let before = project.clone();
    let disk = work.bytes();
    let manifest = work.0.join(".world/project.json");
    let plan = project
        .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
        .unwrap();
    assert!(plan
        .migration
        .as_ref()
        .unwrap()
        .manifest_bytes_before()
        .is_none());
    assert!(plan
        .changes
        .iter()
        .any(|change| change.path == manifest && change.before.is_none()));
    assert_eq!(work.bytes(), disk);
    assert!(!work.0.join(".world").exists());
    project
        .apply_writing_authoring(std::slice::from_ref(&buffer), &plan)
        .unwrap();
    assert_eq!(project.language_version(), "1.10");
    assert!(project.authoring_document(&manifest).is_ok());
    assert!(!manifest.exists());
    assert_eq!(work.bytes(), disk);
    let after = project.clone();
    let entity_target = TargetRef::new("entity", "lin");
    assert!(project.restore(before.clone()));
    // restore 保留新文档墓碑供保存后撤销；会话库存基线不是磁盘活跃内容散列。
    assert_ne!(project.content_baseline(), baseline);
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
    let tombstone = project.authoring_document(&manifest).unwrap();
    assert!(tombstone.is_deleted());
    assert!(!tombstone.is_dirty());
    assert!(!project.is_dirty());
    assert!(project
        .compile_object_search_snapshot()
        .analysis
        .catalog
        .object(&entity_target)
        .is_none());
    assert!(project
        .compile_object_search_snapshot()
        .analysis
        .catalog
        .text_links
        .is_empty());
    project.save().unwrap();
    assert_eq!(work.bytes(), disk);
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.content_baseline(), baseline);
    assert_eq!(reopened.language_version(), "1.9");
    assert!(reopened.authoring_document(&manifest).is_err());
    assert_eq!(reopened.document(&reopened.entry).unwrap(), SOURCE);

    assert!(project.restore(after.clone()));
    project.save().unwrap();
    assert!(manifest.exists());
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.language_version(), "1.10");
    let saved_manifest: serde_json::Value =
        serde_json::from_slice(reopened.authoring_document(&manifest).unwrap().bytes()).unwrap();
    assert_eq!(saved_manifest["schema_version"], 1);
    assert_eq!(saved_manifest["language_version"], "1.10");
    assert!(reopened
        .compile_object_search_snapshot()
        .analysis
        .catalog
        .object(&entity_target)
        .is_some());

    // 保存以后再撤销，墓碑必须真正删除磁盘清单并恢复原源码；不能只改内存版本。
    assert!(project.restore(before));
    assert!(project.authoring_document(&manifest).unwrap().is_deleted());
    assert!(project.authoring_document(&manifest).unwrap().is_dirty());
    assert_eq!(project.document(&project.entry).unwrap(), SOURCE);
    project.save().unwrap();
    assert!(!manifest.exists());
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.language_version(), "1.9");
    assert_eq!(reopened.content_baseline(), baseline);
    assert_eq!(reopened.document(&reopened.entry).unwrap(), SOURCE);
    assert!(reopened.authoring_document(&manifest).is_err());
    assert!(reopened
        .compile_object_search_snapshot()
        .analysis
        .catalog
        .object(&entity_target)
        .is_none());

    assert!(project.restore(after));
    project.save().unwrap();
    let reopened = Project::open(&work.0).unwrap();
    assert_eq!(reopened.language_version(), "1.10");
    assert_eq!(
        reopened
            .compile_object_search_snapshot()
            .analysis
            .catalog
            .text_links[0]
            .target,
        entity_target
    );
    assert!(!reopened.compile_object_search_snapshot().has_errors());
}

#[test]
fn failed_migration_exposes_keyword_diagnostics_and_preserves_every_draft() {
    let source = "event start\n  entity old words\n  你看见林😀。\n  -> END\n";
    let (work, mut project) = Workspace::new(&[("world.wl", source)], None);
    assert!(!project.compile_object_search_snapshot().has_errors());
    let mut buffer = project.open_writing_buffer(&start()).unwrap();
    buffer.replace_source(source.replace("你看见", "尚未应用：你看见"));
    let keep = buffer_state(&buffer);
    let mut command = request(&project, &buffer, "林😀", entity(&project.entry));
    command.enable_entities = true;
    let baseline = project.content_baseline();
    let disk = work.bytes();
    let plan = project
        .preview_writing_authoring(&[buffer.clone()], &command)
        .unwrap();
    assert!(!plan.can_apply);
    let migration = plan.migration.as_ref().unwrap();
    assert!(!migration.can_apply);
    assert!(migration
        .diagnostics_after
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error));
    assert!(!migration.new_diagnostics.is_empty());
    assert!(migration
        .keyword_changes
        .iter()
        .any(|change| change.line == 2));
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer_state(&buffer), keep);
    assert_eq!(work.bytes(), disk);
    assert!(project
        .apply_writing_authoring(&[buffer.clone()], &plan)
        .is_err());
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(buffer_state(&buffer), keep);
    assert_eq!(work.bytes(), disk);
}

#[test]
fn already_enabled_entity_creation_keeps_current_version_and_manifest_bytes() {
    for version in ["1.10", "1.11", "1.12", "1.13"] {
        let manifest = format!(
            "{{\"schema_version\":1,\"language_version\":\"{version}\",\"required_features\":[],\"extension\":{{\"number\":1e2}}}}"
        );
        let (work, mut project) = Workspace::new(&[("world.wl", SOURCE)], Some(&manifest));
        let buffer = project.open_writing_buffer(&start()).unwrap();
        let mut command = request(&project, &buffer, "林😀", entity(&project.entry));
        command.enable_entities = true;
        let plan = project
            .preview_writing_authoring(std::slice::from_ref(&buffer), &command)
            .unwrap();
        assert!(plan.can_apply);
        assert!(plan.migration.is_none());
        assert_eq!(plan.changes.len(), 1);
        project.apply_writing_authoring(&[buffer], &plan).unwrap();
        assert_eq!(project.language_version(), version);
        assert_eq!(
            project
                .authoring_document(&work.0.join(".world/project.json"))
                .unwrap()
                .bytes(),
            manifest.as_bytes()
        );
        assert!(!project.compile_object_search_snapshot().has_errors());
        assert_eq!(fs::read_to_string(&project.entry).unwrap(), SOURCE);
    }
}

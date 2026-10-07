use super::*;

#[test]
fn existing_book_merges_unknown_fields_and_inserts_only_among_selected_siblings() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let mut revision = Revision::default();
    let first = request(&project, revision);
    let result = apply(&mut project, &mut revision, &first);
    let path = work.0.join(result.plan.manuscript_path);
    let mut value: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    value["extension"] = serde_json::json!({"opaque":[1,true,"原样"]});
    value["entries"][0]["extension"] = serde_json::json!({"keep":42});
    value["entries"][0]["target_ref"]["future"] = serde_json::json!("identity metadata");
    value["entries"].as_array_mut().unwrap().insert(
        0,
        serde_json::json!({"id":"part","kind":"section","title":"第一部","custom":"retain"}),
    );
    value["entries"][1]["parent_id"] = serde_json::json!("part");
    project
        .set_authoring_document(&path, serde_json::to_vec(&value).unwrap())
        .unwrap();
    let mut next = request(&project, revision);
    next.book = ManuscriptBookDestination::Existing { id: "novel".into() };
    next.chapter.id = "two".into();
    next.chapter.parent_section_id = Some("part".into());
    next.chapter.after_sibling_id = Some("chapter_one".into());
    let result = apply(&mut project, &mut revision, &next);
    assert_eq!(
        result.plan.runtime_fingerprint_before,
        result.plan.runtime_fingerprint_after
    );
    let actual: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(actual["extension"], value["extension"]);
    assert_eq!(actual["entries"][0], value["entries"][0]);
    assert_eq!(actual["entries"][1], value["entries"][1]);
    assert_eq!(actual["entries"][2]["id"], "two");
    assert_eq!(
        project
            .manuscript_index("novel")
            .unwrap()
            .page(0, 20)
            .chapters
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["chapter_one", "two"]
    );
    let snapshot = project.export_files().unwrap();
    let restored = Project::from_snapshot(&work.0, Path::new("world.wl"), &snapshot).unwrap();
    assert_eq!(
        restored
            .manuscript_index("novel")
            .unwrap()
            .page(0, 20)
            .total,
        2
    );
}

#[test]
fn source_version_gates_read_only_and_inactive_destinations_remain_enforced() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let manifest = work.0.join(".world/project.json");
    project
        .create_authoring_document(
            &manifest,
            br#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#.to_vec(),
        )
        .unwrap();
    project.set_text(&project.entry.clone(), "entity place kind location as \"同名\"\n  description \"资料\"\nfragment prose()\n  片段正文\n  return\nevent start\n  scene room\n    场景正文\n    -> END\n  -> END\n".into()).unwrap();
    for target in [
        TargetRef::new("entity", "place"),
        TargetRef::new("fragment", "prose"),
        TargetRef::new("scene", "start.room"),
    ] {
        let mut current = project.clone();
        let mut revision = Revision::default();
        let mut request = request(&current, revision);
        request.source = ManuscriptChapterSource::Existing {
            target: target.clone(),
        };
        assert_eq!(apply(&mut current, &mut revision, &request).target, target);
    }
    project.save().unwrap();
    fs::write(&manifest, r#"{"schema_version":1,"language_version":"1.13","required_features":["future.unknown.v99"]}"#).unwrap();
    let project = Project::open(&work.0).unwrap();
    let request = request(&project, Revision::default());
    assert_eq!(
        project
            .preview_manuscript_chapter_create(Revision::default(), &request)
            .unwrap_err()
            .code,
        ManuscriptChapterCreateFailureCode::ReadOnly
    );
}

#[test]
fn new_file_failure_after_source_creation_leaves_no_orphan() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    let mut revision = Revision::default();
    let first = request(&project, revision);
    let created = apply(&mut project, &mut revision, &first);
    let path = work.0.join(created.plan.manuscript_path);
    let mut invalid: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    // 重复旧节点仅在后段 prepare_manuscript 的结构验证中拒绝；新章本身合法。
    let repeated = invalid["entries"][0].clone();
    invalid["entries"].as_array_mut().unwrap().push(repeated);
    let invalid = serde_json::to_vec(&invalid).unwrap();
    project
        .set_authoring_document(&path, invalid.clone())
        .unwrap();
    assert!(!project.manuscript_index("novel").unwrap().read_only);
    let mut request = request(&project, revision);
    request.book = ManuscriptBookDestination::Existing { id: "novel".into() };
    request.chapter.id = "fresh_chapter".into();
    new_event(&mut request, "chapters/new.wl", true);
    let baseline = project.content_baseline();
    let error = project
        .preview_manuscript_chapter_create(revision, &request)
        .unwrap_err();
    assert_eq!(
        error.code,
        ManuscriptChapterCreateFailureCode::InvalidChapter
    );
    assert!(error.message.contains("节点 ID 重复"));
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.documents.len(), 1);
    assert_eq!(project.authoring_documents.len(), 2);
    assert_eq!(project.authoring_document(&path).unwrap().bytes(), invalid);
    assert!(!project
        .document(&project.entry)
        .unwrap()
        .contains("include"));
    assert!(!work.0.exists());
}

#[test]
fn inactive_archived_and_tombstoned_sources_are_not_activated_by_new_chapter() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    project.save().unwrap();
    fs::write(work.0.join("archive.wl"), "event archived\n  -> END\n").unwrap();
    fs::write(work.0.join("inactive.wl"), "event inactive\n  -> END\n").unwrap();
    fs::create_dir_all(work.0.join(".world")).unwrap();
    fs::write(work.0.join(".world/project.json"), r#"{"schema_version":1,"entry":"world.wl","required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["archive.wl"]}}"#).unwrap();
    let mut project = Project::open(&work.0).unwrap();
    for path in ["archive.wl", "inactive.wl"] {
        let mut request = request(&project, Revision::default());
        new_event(&mut request, path, false);
        let baseline = project.content_baseline();
        assert_eq!(
            project
                .preview_manuscript_chapter_create(Revision::default(), &request)
                .unwrap_err()
                .code,
            ManuscriptChapterCreateFailureCode::SourceUnavailable
        );
        assert_eq!(project.content_baseline(), baseline);
    }
    project
        .delete_document(&work.0.join("inactive.wl"))
        .unwrap();
    let mut request = request(&project, Revision::default());
    new_event(&mut request, "inactive.wl", true);
    assert_eq!(
        project
            .preview_manuscript_chapter_create(Revision::default(), &request)
            .unwrap_err()
            .code,
        ManuscriptChapterCreateFailureCode::InvalidDestination
    );
    assert_eq!(project.sources().len(), 1);
    assert!(project.manuscript_indices().is_empty());
}

#[test]
fn another_registered_presentation_file_cannot_be_taken_over() {
    let work = Workspace::new();
    let mut project = Project::new(&work.0);
    project.save().unwrap();
    fs::create_dir_all(work.0.join(".world/manuscripts")).unwrap();
    let original = br#"{"schema_version":1,"id":"atlas","extension":{"unchanged":true}}"#;
    fs::write(work.0.join(".world/manuscripts/novel.json"), original).unwrap();
    fs::write(work.0.join(".world/project.json"), r#"{"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{"atlas":".world/manuscripts/novel.json"}}"#).unwrap();
    let project = Project::open(&work.0).unwrap();
    assert!(project.authoring_diagnostics().is_empty());
    let request = request(&project, Revision::default());
    assert_eq!(
        project
            .preview_manuscript_chapter_create(Revision::default(), &request)
            .unwrap_err()
            .code,
        ManuscriptChapterCreateFailureCode::InvalidDestination
    );
    assert_eq!(
        fs::read(work.0.join(".world/manuscripts/novel.json")).unwrap(),
        original
    );
}

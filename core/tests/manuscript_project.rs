use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::manuscript::{ManuscriptCommand, ManuscriptDraft, ManuscriptEntryDraft};
use worldline_core::presentation_commands::Revision;
use worldline_core::project::Project;
use worldline_core::{ManuscriptEntryKind, ManuscriptReferenceRole};

fn root(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-manuscript-project-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn project(name: &str) -> Project {
    let root = root(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(
        root.join("world.wl"),
        concat!(
            "character traveler as \"旅人\"\n",
            "entity lighthouse kind place as \"雾港灯塔\"\n",
            "  description \"作者维护的灯塔资料。\"\n",
            "event arrival as \"抵达\"\n",
            "  旅人抵达雾港。\n",
            "  scene harbor\n",
            "    灯塔亮起。\n",
            "    -> END\n",
            "  -> END\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","presentation.manuscripts.v1"],"maps":{},"graph_views":{},"manuscripts":{},"extension":{"keep":"manifest"}}"#,
    )
    .unwrap();
    Project::open(&root).unwrap()
}

fn draft() -> ManuscriptDraft {
    ManuscriptDraft {
        id: "novel".into(),
        title: "雾港手稿".into(),
        entries: vec![
            ManuscriptEntryDraft {
                id: "volume_one".into(),
                kind: ManuscriptEntryKind::Section,
                parent_id: None,
                title: "第一卷".into(),
                target_ref: None,
                summary: None,
                pov: None,
                status: None,
                goal: None,
            },
            ManuscriptEntryDraft {
                id: "chapter_lighthouse".into(),
                kind: ManuscriptEntryKind::Chapter,
                parent_id: Some("volume_one".into()),
                title: "灯塔".into(),
                target_ref: Some(TargetRef::new("entity", "lighthouse")),
                summary: Some("灯塔设定".into()),
                pov: Some(TargetRef::new("character", "traveler")),
                status: Some("draft".into()),
                goal: Some("介绍灯塔".into()),
            },
            ManuscriptEntryDraft {
                id: "chapter_arrival".into(),
                kind: ManuscriptEntryKind::Chapter,
                parent_id: Some("volume_one".into()),
                title: "抵达".into(),
                target_ref: Some(TargetRef::new("event", "arrival")),
                summary: None,
                pov: None,
                status: Some("planned".into()),
                goal: Some("推进情节".into()),
            },
        ],
    }
}

fn command(
    project: &Project,
    revision: Revision,
    original: Option<&str>,
    draft: ManuscriptDraft,
) -> ManuscriptCommand {
    ManuscriptCommand {
        expected_revision: revision,
        expected_baseline: project.content_baseline(),
        original: original.map(str::to_owned),
        draft,
    }
}

#[test]
fn registered_manuscripts_load_apply_save_reopen_undo_and_export() {
    let mut project = project("lifecycle");
    let content = project.compile();
    assert!(!content.has_errors(), "{:?}", content.diagnostics);
    let fingerprint = content.analysis.fingerprint;
    assert!(project.manuscript_indices().is_empty());
    let before = project.clone();
    let mut revision = Revision::default();
    let request = command(&project, revision, None, draft());
    let baseline = project.content_baseline();
    let book_path = project.root.join(".world/manuscripts/novel.json");
    let preview = project.preview_manuscript(revision, &request).unwrap();
    assert_eq!(preview.page(0, 10).chapters.len(), 2);
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    assert!(!book_path.exists());

    let applied = project.apply_manuscript(&mut revision, request).unwrap();
    assert_eq!(revision.presentation_generation, 1);
    assert_eq!(applied.changed_files.len(), 2);
    assert!(project.is_dirty());
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    assert_eq!(
        project.manuscript_index("novel").unwrap().page(0, 10).total,
        2
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&project.root.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(manifest["extension"]["keep"], "manifest");

    assert!(project.restore(before));
    assert!(project.manuscript_indices().is_empty());
    let recreated = command(&project, revision, None, draft());
    project.apply_manuscript(&mut revision, recreated).unwrap();
    project.save().unwrap();
    assert!(!project.is_dirty());

    let saved_root = project.root.clone();
    let mut reopened = Project::open(&saved_root).unwrap();
    let loaded = reopened.manuscript_index("novel").unwrap();
    assert_eq!(loaded.page(0, 10).chapters[0].id, "chapter_lighthouse");
    assert_eq!(
        loaded.page(0, 10).chapters[0].summary.as_deref(),
        Some("灯塔设定")
    );
    assert_eq!(
        loaded.page(0, 10).chapters[0]
            .source
            .as_ref()
            .unwrap()
            .status,
        worldline_core::ManuscriptReferenceStatus::Resolved
    );
    assert_eq!(
        loaded.page(0, 10).chapters[0]
            .source
            .as_ref()
            .unwrap()
            .stats
            .unwrap()
            .han_characters,
        9
    );
    assert_eq!(reopened.compile().analysis.fingerprint, fingerprint);

    let appendix_path = saved_root.join(".world/manuscripts/appendix.json");
    fs::write(
        &appendix_path,
        r#"{"schema_version":1,"id":"appendix","title":"附录","entries":[]}"#,
    )
    .unwrap();
    let manifest_path = saved_root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["manuscripts"]["appendix"] = serde_json::json!(".world/manuscripts/appendix.json");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    assert!(reopened.refresh().unwrap().is_empty());
    assert_eq!(
        reopened
            .manuscript_index("appendix")
            .unwrap()
            .page(0, 10)
            .total,
        0
    );

    let export_root = root("export");
    let _ = fs::remove_dir_all(&export_root);
    reopened.save_as(&export_root).unwrap();
    let exported = Project::open(&export_root).unwrap();
    assert_eq!(
        exported
            .manuscript_index("novel")
            .unwrap()
            .page(0, 10)
            .total,
        2
    );
    assert_eq!(
        exported
            .manuscript_index("appendix")
            .unwrap()
            .page(0, 10)
            .total,
        0
    );
    let _ = fs::remove_dir_all(&saved_root);
    let _ = fs::remove_dir_all(&export_root);
}

#[test]
fn edits_preserve_unknown_fields_and_reject_stale_or_external_baselines() {
    let mut project = project("update");
    let mut revision = Revision::default();
    let create = command(&project, revision, None, draft());
    project.apply_manuscript(&mut revision, create).unwrap();
    let path = project.root.join(".world/manuscripts/novel.json");
    let mut source: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    source["future"] = serde_json::json!({"nested": [1, 2, 3]});
    source["entries"][1]["chapter_extension"] = serde_json::json!("preserve");
    source["entries"][1]["target_ref"]["future_ref_field"] = serde_json::json!({"keep": true});
    source["entries"][1]["pov"]["future_pov_field"] = serde_json::json!("preserve");
    project
        .set_authoring_document(&path, serde_json::to_vec(&source).unwrap())
        .unwrap();
    let manifest_path = project.root.join(".world/project.json");
    let manifest_before_edit = project
        .authoring_document(&manifest_path)
        .unwrap()
        .bytes()
        .to_vec();

    let current = project.manuscript_index("novel").unwrap();
    let mut changed = ManuscriptDraft::from_index(&current);
    changed.entries.swap(1, 2);
    changed.entries[2].status = Some("ready".into());
    let baseline = project.content_baseline();
    let request = command(&project, revision, Some("novel"), changed);
    let preview = project.preview_manuscript(revision, &request).unwrap();
    assert_eq!(preview.page(0, 10).chapters[0].id, "chapter_arrival");
    assert_eq!(project.content_baseline(), baseline);
    let updated = project.apply_manuscript(&mut revision, request).unwrap();
    assert_eq!(updated.changed_files, std::slice::from_ref(&path));
    assert_eq!(
        project.authoring_document(&manifest_path).unwrap().bytes(),
        manifest_before_edit
    );
    let written: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(written["future"], source["future"]);
    assert_eq!(written["entries"][2]["chapter_extension"], "preserve");
    assert_eq!(
        written["entries"][2]["target_ref"]["future_ref_field"],
        source["entries"][1]["target_ref"]["future_ref_field"]
    );
    assert_eq!(
        written["entries"][2]["pov"]["future_pov_field"],
        source["entries"][1]["pov"]["future_pov_field"]
    );
    assert_eq!(written["entries"][2]["status"], "ready");

    let stale_revision = revision;
    let stale_index = project.manuscript_index("novel").unwrap();
    let stale_draft = ManuscriptDraft::from_index(&stale_index);
    let stale = command(&project, stale_revision, Some("novel"), stale_draft.clone());
    let source_path = project.entry.clone();
    project
        .set_text(
            &source_path,
            format!("{}\n# changed\n", project.document(&source_path).unwrap()),
        )
        .unwrap();
    let before_failure = project.content_baseline();
    assert!(project.apply_manuscript(&mut revision, stale).is_err());
    assert_eq!(project.content_baseline(), before_failure);
    assert_eq!(revision, stale_revision);

    project.save().unwrap();
    let mut external = Project::open(&project.root).unwrap();
    let external_path = external.root.join(".world/manuscripts/novel.json");
    fs::write(&external_path, b"external version").unwrap();
    let request = command(&external, revision, Some("novel"), stale_draft);
    let before_external_rejection = external.content_baseline();
    assert!(external.apply_manuscript(&mut revision, request).is_err());
    assert_eq!(external.content_baseline(), before_external_rejection);
    let _ = fs::remove_dir_all(&project.root);
}

#[test]
fn manuscript_references_block_delete_and_participate_in_stable_id_rename() {
    let mut project = project("impact");
    let mut revision = Revision::default();
    let create = command(&project, revision, None, draft());
    project.apply_manuscript(&mut revision, create).unwrap();
    let manuscript_path = project.root.join(".world/manuscripts/novel.json");
    let mut manuscript: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&manuscript_path)
            .unwrap()
            .bytes(),
    )
    .unwrap();
    manuscript["future_target_like"] = serde_json::json!({"kind":"entity","id":"lighthouse"});
    project
        .set_authoring_document(&manuscript_path, serde_json::to_vec(&manuscript).unwrap())
        .unwrap();
    let target = TargetRef::new("entity", "lighthouse");
    let before = project.content_baseline();
    let impact = project.deletion_impact(&target);
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete());
    assert_eq!(impact.manuscripts.len(), 1);
    assert_eq!(impact.manuscripts[0].chapter_id, "chapter_lighthouse");
    assert_eq!(impact.manuscripts[0].role, ManuscriptReferenceRole::Source);
    assert!(project.remove_entity("lighthouse").is_err());
    assert_eq!(project.content_baseline(), before);

    let fingerprint = project.compile().analysis.fingerprint;
    let plan = project
        .plan_rename_target(&target, "beacon")
        .expect("重命名应改写书稿中的稳定引用");
    assert!(plan
        .changes
        .iter()
        .any(|change| change.path.ends_with("novel.json")));
    project.apply_rename_plan(&plan).unwrap();
    let renamed = project.manuscript_index("novel").unwrap();
    let rewritten_document: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&manuscript_path)
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(rewritten_document["future_target_like"]["id"], "lighthouse");
    let chapter = renamed
        .page(0, 10)
        .chapters
        .into_iter()
        .find(|chapter| chapter.id == "chapter_lighthouse")
        .unwrap();
    assert_eq!(
        chapter.target_ref.unwrap(),
        TargetRef::new("entity", "beacon")
    );
    assert_eq!(
        chapter.source.unwrap().status,
        worldline_core::ManuscriptReferenceStatus::Resolved
    );
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let _ = fs::remove_dir_all(&project.root);
}

#[test]
fn old_projects_do_not_adopt_unregistered_json_and_registry_cannot_alias_manifest() {
    let root = root("legacy");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/manuscripts")).unwrap();
    fs::write(root.join("world.wl"), "event opening\n  -> END\n").unwrap();
    let manifest = br#"{"schema_version":1,"language_version":"1.9","entry":"world.wl","required_features":[],"maps":{}}"#;
    fs::write(root.join(".world/project.json"), manifest).unwrap();
    fs::write(
        root.join(".world/manuscripts/novel.json"),
        r#"{"schema_version":1,"id":"novel","title":"未注册","entries":[]}"#.as_bytes(),
    )
    .unwrap();

    let project = Project::open(&root).unwrap();
    assert!(project.manuscript_indices().is_empty());
    assert!(project
        .authoring_document(&root.join(".world/manuscripts/novel.json"))
        .is_err());
    assert_eq!(
        project
            .authoring_document(&root.join(".world/project.json"))
            .unwrap()
            .bytes(),
        manifest
    );

    let alias_manifest = br#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["presentation.manuscripts.v1"],"manuscripts":{"self":".world/project.json"}}"#;
    fs::write(root.join(".world/project.json"), alias_manifest).unwrap();
    let project = Project::open(&root).unwrap();
    assert!(project.manuscript_index("self").is_err());
    assert!(project
        .authoring_diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "WS004"));
    assert!(
        !project
            .deletion_impact(&TargetRef::new("event", "opening"))
            .complete
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn registry_rejects_distinct_paths_to_the_same_physical_document() {
    let root = root("hardlink-registry");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/manuscripts")).unwrap();
    fs::write(root.join("world.wl"), "event opening\n  -> END\n").unwrap();
    let original = root.join(".world/manuscripts/novel.json");
    fs::write(
        &original,
        r#"{"schema_version":1,"id":"novel","title":"雾港","entries":[]}"#,
    )
    .unwrap();
    fs::hard_link(&original, root.join(".world/manuscripts/alias.json")).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["presentation.manuscripts.v1"],"manuscripts":{"novel":".world/manuscripts/novel.json","alias":".world/manuscripts/alias.json"}}"#,
    )
    .unwrap();
    let project = Project::open(&root).unwrap();
    assert!(project
        .authoring_diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "WS004"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn registered_manuscript_without_capability_is_read_only_and_path_escape_is_rejected() {
    let capability_root = root("capability");
    let _ = fs::remove_dir_all(&capability_root);
    fs::create_dir_all(capability_root.join(".world/manuscripts")).unwrap();
    fs::write(
        capability_root.join("world.wl"),
        "event opening\n  -> END\n",
    )
    .unwrap();
    fs::write(
        capability_root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[],"manuscripts":{"novel":".world/manuscripts/novel.json"}}"#,
    )
    .unwrap();
    fs::write(
        capability_root.join(".world/manuscripts/novel.json"),
        r#"{"schema_version":1,"id":"novel","title":"雾港","entries":[]}"#,
    )
    .unwrap();
    let mut project = Project::open(&capability_root).unwrap();
    let index = project.manuscript_index("novel").unwrap();
    assert!(index.read_only);
    assert!(index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MAN002"));
    let mut revision = Revision::default();
    let request = command(
        &project,
        revision,
        Some("novel"),
        ManuscriptDraft::from_index(&index),
    );
    assert!(project.apply_manuscript(&mut revision, request).is_err());
    assert!(!project.is_dirty());

    let escape_root = root("path-escape");
    let _ = fs::remove_dir_all(&escape_root);
    fs::create_dir_all(escape_root.join(".world")).unwrap();
    fs::write(escape_root.join("world.wl"), "event opening\n  -> END\n").unwrap();
    fs::write(
        escape_root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["presentation.manuscripts.v1"],"manuscripts":{"escape":"../outside.json"}}"#,
    )
    .unwrap();
    let escaped = Project::open(&escape_root).unwrap();
    assert!(escaped.manuscript_index("escape").is_err());
    assert!(escaped
        .authoring_diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == "WS004"));
    let _ = fs::remove_dir_all(&capability_root);
    let _ = fs::remove_dir_all(&escape_root);
}

#[test]
fn project_deletion_impact_includes_scene_references_owned_by_an_event() {
    let mut project = project("scene-impact");
    let mut revision = Revision::default();
    let mut scene_draft = draft();
    scene_draft
        .entries
        .retain(|entry| entry.kind == ManuscriptEntryKind::Chapter);
    scene_draft.entries.truncate(1);
    scene_draft.entries[0].id = "chapter_harbor".into();
    scene_draft.entries[0].parent_id = None;
    scene_draft.entries[0].title = "港口".into();
    scene_draft.entries[0].target_ref = Some(TargetRef::new("scene", "arrival.harbor"));
    let create = command(&project, revision, None, scene_draft);
    project.apply_manuscript(&mut revision, create).unwrap();

    let impact = project.deletion_impact(&TargetRef::new("event", "arrival"));
    assert!(!impact.can_delete());
    assert!(impact.manuscripts.iter().any(|reference| {
        reference.chapter_id == "chapter_harbor"
            && reference.target == TargetRef::new("scene", "arrival.harbor")
    }));
    assert!(project.remove_event("arrival").is_err());
    let _ = fs::remove_dir_all(&project.root);
}

#[test]
fn missing_sources_are_rejected_without_creating_or_mutating_a_manuscript() {
    let mut project = project("missing-source-write");
    let mut revision = Revision::default();
    let mut invalid = draft();
    invalid.entries[1].target_ref = Some(TargetRef::new("event", "not_in_the_project"));
    let command = command(&project, revision, None, invalid);
    let baseline = project.content_baseline();
    let book_path = project.root.join(".world/manuscripts/novel.json");

    assert!(project.preview_manuscript(revision, &command).is_err());
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.is_dirty());
    assert!(!book_path.exists());
    assert!(project.apply_manuscript(&mut revision, command).is_err());
    assert_eq!(revision, Revision::default());
    assert!(!project.is_dirty());
    assert!(!book_path.exists());
    let _ = fs::remove_dir_all(&project.root);
}

#[test]
fn explicit_creation_adds_only_the_manuscript_capability_to_a_manifestless_v19_project() {
    let root = root("explicit-v19-create");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "event opening as \"开篇\"\n  雾港起雾。\n  -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    let fingerprint = project.compile().analysis.fingerprint;
    let mut revision = Revision::default();
    let request = command(
        &project,
        revision,
        None,
        ManuscriptDraft {
            id: "novel".into(),
            title: "雾港手稿".into(),
            entries: vec![ManuscriptEntryDraft {
                id: "opening".into(),
                kind: ManuscriptEntryKind::Chapter,
                parent_id: None,
                title: "开篇".into(),
                target_ref: Some(TargetRef::new("event", "opening")),
                summary: None,
                pov: None,
                status: Some("draft".into()),
                goal: None,
            }],
        },
    );
    let preview = project.preview_manuscript(revision, &request).unwrap();
    assert_eq!(preview.page(0, 10).chapters[0].id, "opening");
    let applied = project.apply_manuscript(&mut revision, request).unwrap();
    assert_eq!(applied.changed_files.len(), 2);
    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    project.save().unwrap();

    let mut reopened = Project::open(&root).unwrap();
    assert_eq!(reopened.language_version(), "1.9");
    assert_eq!(reopened.compile().analysis.fingerprint, fingerprint);
    let manifest: serde_json::Value = serde_json::from_slice(
        reopened
            .authoring_document(&root.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(
        manifest["required_features"][0],
        "presentation.manuscripts.v1"
    );
    assert_eq!(
        manifest["manuscripts"]["novel"],
        ".world/manuscripts/novel.json"
    );
    let _ = fs::remove_dir_all(&root);
}

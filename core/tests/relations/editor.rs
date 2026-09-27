use super::{project_root, relation_source};
use std::fs;
use worldline_core::{RelationDirection, TargetRef};

#[test]
fn project_relation_crud_is_transactional_and_does_not_change_fingerprint() {
    let root = project_root(
        "crud",
        "entity a kind place\nentity b kind place\nrelation_type links as \"链接\"\n  inverse \"被链接\"\n",
    );
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let before = project.compile().analysis.fingerprint;
    project
        .write_relation(
            None,
            &worldline_core::RelationDraft {
                id: "edge".into(),
                relation_type: "links".into(),
                from: TargetRef::new("entity", "a"),
                to: TargetRef::new("entity", "b"),
                description: "资料关系".into(),
                source_note: Some("作者手记".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let created = project.compile();
    assert_eq!(created.analysis.fingerprint, before);
    assert_eq!(
        created.analysis.catalog.relations["edge"]
            .source_note
            .as_deref(),
        Some("作者手记")
    );
    project
        .write_relation(
            Some("edge"),
            &worldline_core::RelationDraft {
                id: "edge".into(),
                relation_type: "links".into(),
                from: TargetRef::new("entity", "a"),
                to: TargetRef::new("entity", "b"),
                description: "已修改".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        project.compile().analysis.catalog.relations["edge"].description,
        "已修改"
    );
    project.remove_relation("edge").unwrap();
    assert!(project.compile().analysis.catalog.relations.is_empty());
    let _ = fs::remove_dir_all(root);
}
#[test]
fn deleting_an_endpoint_is_blocked_by_relation_reference_impact() {
    let root = project_root("delete-impact", &relation_source(""));
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let error = project.remove_entity("keepers").unwrap_err();
    assert!(error.contains("引用") || error.contains("关系"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deleting_a_relation_is_blocked_by_its_catalog_and_body_references() {
    let root = project_root(
        "relation-delete-impact",
        "tag relation_tag as \"关系标签\"\nentity a kind place\nentity b kind place\nrelation_type links as \"链接\"\nrelation_def edge type links from entity a to entity b\nalias relation edge as \"边\"\nmark relation edge with relation_tag\nevent start\n  [[relation:edge|边]]\n  -> END\n",
    );
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let error = project.remove_relation("edge").unwrap_err();
    assert!(error.contains("引用"));
    assert!(project
        .compile()
        .analysis
        .catalog
        .relations
        .contains_key("edge"));
    let _ = fs::remove_dir_all(root);
}
#[test]
fn legacy_promotion_preview_then_batch_commit_preserves_notes_and_reports_fingerprint_delta() {
    let root = project_root(
        "promotion",
        "character a\n  relation b as \"旧关系\"\ncharacter b\nrelation_type knows as \"认识\"\n  inverse \"被认识\"\n",
    );
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let handle = project
        .compile()
        .analysis
        .catalog
        .legacy_relation_handles()
        .into_iter()
        .next()
        .unwrap();
    let draft = worldline_core::RelationDraft {
        id: "promoted".into(),
        relation_type: "knows".into(),
        from: handle.source.clone(),
        to: handle.target.clone(),
        description: "旧关系".into(),
        source_note: Some("由旧人物关系提升".into()),
        scope_refs: vec![TargetRef::new("character", "a")],
        properties: vec![(
            "confidence".into(),
            worldline_core::ast::PropertyValue::Num(0.8),
        )],
    };
    let source_before = project
        .document(&root.join("world.wl"))
        .unwrap()
        .to_string();
    let preview = project
        .preview_promote_legacy_relation(&handle, &draft)
        .unwrap();
    assert!(preview.fingerprint_changed);
    assert_eq!(preview.content_baseline, project.content_baseline());
    assert_eq!(preview.draft.scope_refs, draft.scope_refs);
    assert_eq!(preview.draft.properties, draft.properties);
    assert_eq!(
        project.document(&root.join("world.wl")).unwrap(),
        source_before
    );
    project.apply_legacy_relation_promotion(&preview).unwrap();
    let result = project.compile();
    assert!(result.analysis.catalog.legacy_relation_handles().is_empty());
    assert_eq!(
        result.analysis.catalog.relations["promoted"]
            .source_note
            .as_deref(),
        Some("由旧人物关系提升")
    );
    assert_eq!(
        result.analysis.catalog.relations["promoted"].scope_refs,
        draft.scope_refs
    );
    assert_eq!(
        result.analysis.catalog.relations["promoted"]
            .properties
            .get("confidence"),
        Some(&worldline_core::ast::PropertyValue::Num(0.8))
    );
    let _ = fs::remove_dir_all(root);
}
#[test]
fn promotion_rejects_new_content_after_preview_without_writing() {
    let root = project_root(
        "promotion-stale-content",
        "character a\n  relation b as \"旧关系\"\ncharacter b\nrelation_type knows as \"认识\"\n",
    );
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let handle = project
        .compile()
        .analysis
        .catalog
        .legacy_relation_handles()
        .into_iter()
        .next()
        .unwrap();
    let draft = worldline_core::RelationDraft {
        id: "promoted".into(),
        relation_type: "knows".into(),
        from: handle.source.clone(),
        to: handle.target.clone(),
        ..Default::default()
    };
    let preview = project
        .preview_promote_legacy_relation(&handle, &draft)
        .unwrap();
    let path = root.join("world.wl");
    let before = project.document(&path).unwrap().to_string();
    project
        .set_text(&path, format!("{before}\n// 新资料\n"))
        .unwrap();
    let changed = project.document(&path).unwrap().to_string();
    let error = project
        .apply_legacy_relation_promotion(&preview)
        .unwrap_err();
    assert!(error.contains("基线"));
    assert_eq!(project.document(&path).unwrap(), changed);
    assert!(!project
        .document(&path)
        .unwrap()
        .contains("relation_def promoted"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn promotion_rejects_inconsistent_client_preview_without_writing() {
    let root = project_root(
        "promotion-tampered",
        "character a\n  relation b as \"朋友\"\ncharacter b\nrelation_type knows as \"认识\"\n",
    );
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let handle = project
        .compile()
        .analysis
        .catalog
        .legacy_relation_handles()
        .remove(0);
    let draft = worldline_core::RelationDraft {
        id: "new_relation".into(),
        relation_type: "knows".into(),
        from: handle.source.clone(),
        to: handle.target.clone(),
        description: handle.label.clone(),
        ..Default::default()
    };
    let preview = project
        .preview_promote_legacy_relation(&handle, &draft)
        .unwrap();
    let before = project.sources();
    let mut inconsistent = preview.clone();
    inconsistent.draft.id = "other_relation".into();
    assert!(project
        .apply_legacy_relation_promotion(&inconsistent)
        .is_err());
    let mut forged_fingerprint = preview;
    forged_fingerprint.fingerprint_changed = false;
    forged_fingerprint.after_fingerprint = forged_fingerprint.before_fingerprint;
    assert!(project
        .apply_legacy_relation_promotion(&forged_fingerprint)
        .is_err());
    assert_eq!(before, project.sources());
    let _ = fs::remove_dir_all(root);
}
#[test]
fn relation_authoring_requires_declared_workspace_capability() {
    let root = project_root("missing-capability", "character a\n");
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"language_version":"1.10","required_features":[]}"#,
    )
    .unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let before = project.content_baseline();
    let error = project
        .write_relation_type(
            None,
            &worldline_core::RelationTypeDraft {
                id: "knows".into(),
                display: "认识".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(error.contains("content.relations.v1"));
    assert_eq!(project.content_baseline(), before);
    let _ = fs::remove_dir_all(root);
}
#[test]
fn file_relation_targets_resolve_relative_paths_and_round_trip_public_edits() {
    let root = project_root("file-targets", "entity a kind place\nrelation_type records as \"记载\"\nrelation_def record type records from entity a to file \"chapters/record one.wl\"\n  scope file \"chapters/record one.wl\"\n");
    fs::create_dir(root.join("chapters")).unwrap();
    fs::write(root.join("chapters/record one.wl"), "tag notes\n").unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let file = compiled
        .analysis
        .catalog
        .objects
        .iter()
        .find(|object| object.target.kind == "file" && object.target.id.ends_with("record one.wl"))
        .unwrap()
        .target
        .clone();
    assert_eq!(compiled.analysis.catalog.relations["record"].to_ref, file);
    project
        .write_relation(
            Some("record"),
            &worldline_core::RelationDraft {
                id: "record".into(),
                relation_type: "records".into(),
                from: TargetRef::new("entity", "a"),
                to: file.clone(),
                scope_refs: vec![file],
                description: "跨文件来源".into(),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(project
        .document(&root.join("world.wl"))
        .unwrap()
        .contains("file \"chapters/record one.wl\""));
    assert!(!project.compile().has_errors());

    let outside = root
        .parent()
        .unwrap()
        .join("worldline-relations-outside target.wl");
    let outside_target_id = outside.to_string_lossy().into_owned();
    let outside_target = TargetRef::new("file", &outside_target_id);
    let before = project.sources();
    let error = project
        .write_relation(
            None,
            &worldline_core::RelationDraft {
                id: "outside".into(),
                relation_type: "records".into(),
                from: TargetRef::new("entity", "a"),
                to: outside_target,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(error.contains("工作区"));
    assert_eq!(project.sources(), before);
    let _ = fs::remove_dir_all(root);
}
#[test]
fn relation_updates_preserve_block_comments() {
    let root = project_root(
        "relation-comments",
        r#"entity a kind place
entity b kind place
relation_type links as "链接"
  // 类型说明
  direction directed // 方向说明
relation_def edge type links from entity a to entity b
  // 关系说明
  description "旧描述" // 描述说明
"#,
    );
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    project
        .write_relation_type(
            Some("links"),
            &worldline_core::RelationTypeDraft {
                id: "links".into(),
                display: "关联".into(),
                direction: RelationDirection::Directed,
                ..Default::default()
            },
        )
        .unwrap();
    project
        .write_relation(
            Some("edge"),
            &worldline_core::RelationDraft {
                id: "edge".into(),
                relation_type: "links".into(),
                from: TargetRef::new("entity", "a"),
                to: TargetRef::new("entity", "b"),
                description: "新描述".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let text = project.document(&root.join("world.wl")).unwrap();
    assert!(text.contains("// 类型说明"));
    assert!(text.contains("// 方向说明"));
    assert!(text.contains("// 关系说明"));
    assert!(text.contains("// 描述说明"));
    assert!(text.contains("description \"新描述\""));
    assert!(!project.compile().has_errors());
    let _ = fs::remove_dir_all(root);
}

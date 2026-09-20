use std::fs;
use std::path::PathBuf;
use worldline_core::{
    compile_source, compile_source_with_options, LanguageVersion, RelationDirection,
    RelationQueryDirection, RelationQueryOptions, TargetRef,
};

fn relation_source(extra: &str) -> String {
    format!(
        r#"entity keepers kind organization as "守灯会"
entity lighthouse kind place as "灯塔"
entity harbor kind place as "港口"
period modern as "现代"
relation_type maintains as "维护"
  inverse "由其维护"
  direction directed
  from entity
  to entity
relation_def rel_1 type maintains from entity keepers to entity lighthouse
  description "守灯会维护灯塔。"
  source_note "设定稿"
  scope period modern
relation_def rel_2 type maintains from entity keepers to entity lighthouse
  description "守灯会也负责灯塔巡检。"
relation_def rel_3 type maintains from entity lighthouse to entity harbor
{extra}
"#
    )
}

#[test]
fn catalog_json_preserves_complete_relation_index_targets() {
    let result = compile_source_with_options(
        "world.wl",
        &relation_source(""),
        worldline_core::CompileOptions::v1_10(),
    );
    let value = serde_json::to_value(&result.analysis.catalog).unwrap();
    let index = value["relation_index"].as_array().unwrap();
    assert_eq!(index.len(), 3);
    let keepers = index
        .iter()
        .find(|entry| entry["target"]["id"] == "keepers")
        .unwrap();
    assert_eq!(keepers["target"]["kind"], "entity");
    assert_eq!(keepers["relations"], serde_json::json!(["rel_1", "rel_2"]));
    assert_eq!(value["relations"].as_object().unwrap().len(), 3);
}

#[test]
fn truncated_frontier_can_reveal_a_previously_omitted_edge() {
    let source = "entity hub kind place\nentity a kind place\nentity b kind place\nentity c kind place\nrelation_type links as \"连接\"\nrelation_def a type links from entity hub to entity a\nrelation_def b type links from entity hub to entity b\nrelation_def c type links from entity hub to entity c\n";
    let result =
        compile_source_with_options("world.wl", source, worldline_core::CompileOptions::v1_10());
    assert!(!result.has_errors());
    let options = RelationQueryOptions {
        max_nodes: 2,
        ..Default::default()
    };
    let first = result
        .analysis
        .catalog
        .query_relations(&TargetRef::new("entity", "hub"), options.clone());
    assert!(first.truncated);
    let initial: std::collections::BTreeSet<_> =
        first.edges.iter().map(|edge| edge.id.clone()).collect();
    let mut reachable = initial.clone();
    let next = result
        .analysis
        .catalog
        .continue_relations(&first.continuation.unwrap());
    reachable.extend(next.edges.into_iter().map(|edge| edge.id));
    assert!(
        reachable.len() > initial.len(),
        "继续展开应能读到首次截断的关系"
    );
}

#[test]
fn continuation_pages_parallel_edges_and_reaches_second_depth() {
    let mut source = "entity a kind place\nentity b kind place\nentity c kind place\nrelation_type links as \"连接\"\n".to_string();
    for index in 0..601 {
        source.push_str(&format!(
            "relation_def edge_{index:04} type links from entity a to entity b\n"
        ));
    }
    source.push_str("relation_def onward type links from entity b to entity c\n");
    let result =
        compile_source_with_options("world.wl", &source, worldline_core::CompileOptions::v1_10());
    assert!(!result.has_errors());
    let mut page = result.analysis.catalog.query_relations(
        &TargetRef::new("entity", "a"),
        RelationQueryOptions {
            depth: 2,
            ..Default::default()
        },
    );
    assert_eq!(page.edges.len(), 500);
    let mut ids = std::collections::BTreeSet::new();
    loop {
        assert!(page.nodes.len() <= 250 && page.edges.len() <= 500);
        for edge in &page.edges {
            assert!(ids.insert(edge.id.clone()), "不能重复已返回关系");
            assert!(page.nodes.iter().any(|node| node.target == edge.from_ref));
            assert!(page.nodes.iter().any(|node| node.target == edge.to_ref));
        }
        let Some(continuation) = page.continuation else {
            break;
        };
        assert_eq!(continuation.offset, ids.len());
        page = result.analysis.catalog.continue_relations(&continuation);
    }
    assert_eq!(ids.len(), 602);
    assert!(ids.contains("onward"));
}

#[test]
fn explicit_110_relations_have_stable_catalog_identity_and_reverse_projection() {
    let result = compile_source_with_options(
        "world.wl",
        &relation_source(""),
        worldline_core::CompileOptions::v1_10(),
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert_eq!(
        result.analysis.catalog.relation_types["maintains"].display,
        "维护"
    );
    assert_eq!(
        result.analysis.catalog.relations["rel_1"].from_ref,
        TargetRef::new("entity", "keepers")
    );
    assert_eq!(result.analysis.catalog.relations.len(), 3);

    let query = result.analysis.catalog.query_relations(
        &TargetRef::new("entity", "lighthouse"),
        RelationQueryOptions {
            depth: 1,
            direction: RelationQueryDirection::Both,
            ..Default::default()
        },
    );
    assert!(!query.truncated);
    assert_eq!(
        query
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["rel_1", "rel_2", "rel_3"]
    );
    assert_eq!(
        query
            .edges
            .iter()
            .find(|edge| edge.id == "rel_1")
            .unwrap()
            .label,
        "由其维护"
    );
    assert_eq!(query.nodes.len(), 3);
}

#[test]
fn default_19_does_not_enable_relation_keywords_and_relation_data_is_not_in_fingerprint() {
    let source = relation_source("");
    let legacy = compile_source("world.wl", "event start\n  正文。\n  -> END\n");
    let with_relation = compile_source_with_options(
        "world.wl",
        &format!("event start\n  正文。\n  -> END\n{source}"),
        worldline_core::CompileOptions::v1_10(),
    );
    assert_eq!(
        legacy.analysis.fingerprint,
        with_relation.analysis.fingerprint
    );
    let default = compile_source("world.wl", &source);
    assert!(default.has_errors());
    assert!(default.analysis.catalog.relations.is_empty());
}

#[test]
fn old_character_relations_are_projected_with_occurrence_without_becoming_ids() {
    let result = compile_source_with_options(
        "world.wl",
        "character a\n  relation b as \"朋友\"\n  relation b as \"朋友\"\ncharacter b\n",
        worldline_core::CompileOptions::v1_10(),
    );
    let handles = result.analysis.catalog.legacy_relation_handles();
    assert_eq!(handles.len(), 2);
    assert_eq!(handles[0].occurrence, 1);
    assert_eq!(handles[1].occurrence, 2);
    assert!(!handles[0].is_persistent());
    assert!(handles[0].stable_id().is_none());
}

#[test]
fn relation_diagnostics_identify_unknown_type_and_endpoints() {
    let source = r#"entity a kind place
relation_type links as "链接"
relation_def rel_missing type absent from entity a to entity nope
"#;
    let result = compile_source_with_options(
        "world.wl",
        source,
        worldline_core::CompileOptions::new(LanguageVersion::V1_10),
    );
    let codes: Vec<_> = result.diagnostics.iter().map(|d| d.code).collect();
    assert!(codes.contains(&"A221"));
    assert!(codes.contains(&"A222"));
}

#[test]
fn relation_type_rejects_duplicate_direction_and_endpoint_constraints() {
    for field in [
        "direction directed\n  direction undirected",
        "from entity\n  from_kind character",
        "to entity\n  to_kind character",
    ] {
        let result = compile_source_with_options(
            "world.wl",
            &format!("relation_type links as \"连接\"\n  {field}\n"),
            worldline_core::CompileOptions::v1_10(),
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "A220"),
            "{field}: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn relation_instances_can_be_referenced_as_complete_targets() {
    let source = r#"entity a kind place
entity b kind place
relation_type links as "链接"
relation_def outer type links from relation inner to entity a
  scope relation inner
relation_def inner type links from entity a to entity b
"#;
    let result =
        compile_source_with_options("world.wl", source, worldline_core::CompileOptions::v1_10());
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let relation = TargetRef::new("relation", "inner");
    assert!(result.analysis.catalog.object(&relation).is_some());
    assert_eq!(
        result.analysis.catalog.relations["outer"].from_ref,
        relation
    );
    let query = result
        .analysis
        .catalog
        .query_relations(&relation, RelationQueryOptions::default());
    assert_eq!(
        query
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["outer"]
    );
}

#[test]
fn depth_two_query_is_bounded_and_does_not_infer_transitive_edge() {
    let result = compile_source_with_options(
        "world.wl",
        &relation_source(""),
        worldline_core::CompileOptions::v1_10(),
    );
    let query = result.analysis.catalog.query_relations(
        &TargetRef::new("entity", "keepers"),
        RelationQueryOptions {
            depth: 2,
            max_nodes: 2,
            max_edges: 1,
            ..Default::default()
        },
    );
    assert!(query.truncated);
    assert!(query.edges.iter().all(|edge| edge.id != "inferred"));
    assert!(query.nodes.iter().all(|node| node.depth <= 2));
    let visible: std::collections::BTreeSet<_> =
        query.nodes.iter().map(|node| node.target.clone()).collect();
    assert!(query
        .edges
        .iter()
        .all(|edge| visible.contains(&edge.from_ref) && visible.contains(&edge.to_ref)));
    assert!(query
        .continuation
        .as_ref()
        .is_some_and(|continuation| continuation
            .frontier
            .contains(&TargetRef::new("entity", "keepers"))));

    let zero_nodes = result.analysis.catalog.query_relations(
        &TargetRef::new("entity", "keepers"),
        RelationQueryOptions {
            max_nodes: 0,
            ..Default::default()
        },
    );
    assert_eq!(zero_nodes.nodes.len(), 1);
    assert!(zero_nodes.edges.is_empty());
    assert!(zero_nodes.truncated);
}

#[test]
fn direction_filter_keeps_single_source_edge() {
    let result = compile_source_with_options(
        "world.wl",
        &relation_source(""),
        worldline_core::CompileOptions::v1_10(),
    );
    let query = result.analysis.catalog.query_relations(
        &TargetRef::new("entity", "lighthouse"),
        RelationQueryOptions {
            direction: RelationQueryDirection::Outgoing,
            ..Default::default()
        },
    );
    assert_eq!(
        query
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["rel_3"]
    );
    assert_eq!(
        result.analysis.catalog.relation_types["maintains"].direction,
        RelationDirection::Directed
    );
}

#[test]
fn relation_targets_are_available_to_body_alias_and_mark_navigation() {
    let source = r#"tag relation_tag as "关系标签"
entity keepers kind organization as "守灯会"
entity lighthouse kind place as "灯塔"
relation_type maintains as "维护"
relation_def lighthouse_care type maintains from entity keepers to entity lighthouse
  description "守灯会维护灯塔。"
alias relation lighthouse_care as "灯塔维护关系"
mark relation lighthouse_care with relation_tag
event start
  [[relation:lighthouse_care|灯塔维护关系]]
  -> END
"#;
    let result =
        compile_source_with_options("world.wl", source, worldline_core::CompileOptions::v1_10());
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let target = TargetRef::new("relation", "lighthouse_care");
    assert_eq!(
        result.analysis.catalog.aliases[0].target, target,
        "relation aliases must be resolved after relation collection"
    );
    assert_eq!(result.analysis.catalog.marks[0].target, target);
    assert_eq!(result.analysis.catalog.text_links[0].target, target);
    assert!(result
        .analysis
        .catalog
        .references
        .iter()
        .any(|reference| reference.target == target && reference.kind == "正文链接"));
}

fn project_root(name: &str, source: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("worldline-relations-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        br#"{"schema_version":1,"language_version":"1.10","required_features":["content.relations.v1"]}"#,
    )
    .unwrap();
    root
}

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

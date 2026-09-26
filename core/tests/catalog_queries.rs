use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::queries::{
    CatalogQuery, CatalogQueryFilter, CatalogQueryOptions, MissingCondition, PropertyScalar,
    QueryError, SavedQueryDraft, TodoKind,
};

fn project(name: &str, source: &str) -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-catalog-query-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"query_test","language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","content.relations.v1"],"maps":{},"graph_views":{}}"#,
    )
    .unwrap();
    Project::open(&root).unwrap()
}

#[test]
fn saved_query_registration_preserves_legacy_19_and_runtime_fingerprint() {
    static NEXT: AtomicUsize = AtomicUsize::new(1000);
    let root = std::env::temp_dir().join(format!(
        "worldline-saved-query-legacy-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "character traveler as \"旅人\"\nevent start\n  启程。\n  -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert_eq!(project.language_version(), "1.9");
    let fingerprint = project.compile().analysis.fingerprint;
    let draft = SavedQueryDraft {
        id: "draft_people".into(),
        name: "草稿人物".into(),
        query: CatalogQuery {
            schema_version: 1,
            filters: vec![CatalogQueryFilter::Kind {
                values: vec!["character".into()],
                negate: false,
            }],
        },
    };

    project
        .save_saved_query(draft, &project.content_baseline())
        .unwrap();

    assert_eq!(project.language_version(), "1.9");
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let manifest: serde_json::Value = serde_json::from_slice(
        project
            .authoring_document(&root.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(
        manifest["saved_queries"]["draft_people"],
        ".world/queries/draft_people.json"
    );
    assert!(manifest["required_features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|feature| feature == "catalog.saved_queries.v1"));
    assert_eq!(
        project.saved_query_index().queries["draft_people"]
            .draft
            .name,
        "草稿人物"
    );

    project.save().unwrap();
    let reopened = Project::open(&root).unwrap();
    assert_eq!(reopened.language_version(), "1.9");
    assert_eq!(
        reopened.saved_query_index().queries["draft_people"]
            .draft
            .query
            .filters
            .len(),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn saved_query_write_rejects_a_stale_project_baseline_without_creating_documents() {
    static NEXT: AtomicUsize = AtomicUsize::new(4000);
    let root = std::env::temp_dir().join(format!(
        "worldline-saved-query-stale-baseline-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("world.wl"),
        "character traveler as \"旅人\"\nevent start\n  启程。\n  -> END\n",
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    let original_baseline = project.content_baseline();
    project
        .set_text(
            &root.join("world.wl"),
            "character traveler as \"旅人\"\nevent start\n  新的启程。\n  -> END\n".into(),
        )
        .unwrap();

    let error = project
        .save_saved_query(
            SavedQueryDraft {
                id: "stale_query".into(),
                name: "过期表单".into(),
                query: CatalogQuery::default(),
            },
            &original_baseline,
        )
        .unwrap_err();

    assert!(error.contains("StaleBaseline"), "{error}");
    assert!(project
        .authoring_document(&root.join(".world/project.json"))
        .is_err());
    assert!(project
        .authoring_document(&root.join(".world/queries/stale_query.json"))
        .is_err());
    assert_eq!(
        project.document(&root.join("world.wl")).unwrap(),
        "character traveler as \"旅人\"\nevent start\n  新的启程。\n  -> END\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn saved_query_edits_preserve_unknown_fields_and_refuse_unknown_versions() {
    let mut project = project(
        "unknown_fields",
        "entity keepers kind organization as \"守灯会\"\n",
    );
    let query = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Kind {
            values: vec!["entity".into()],
            negate: false,
        }],
    };
    let initial = SavedQueryDraft {
        id: "organizations".into(),
        name: "组织".into(),
        query: query.clone(),
    };
    project
        .save_saved_query(initial.clone(), &project.content_baseline())
        .unwrap();
    let manifest_path = project.root.join(".world/project.json");
    let query_path = project.root.join(".world/queries/organizations.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&manifest_path).unwrap().bytes())
            .unwrap();
    manifest["extension"] = serde_json::json!({"preserve": true});
    project
        .set_authoring_document(&manifest_path, serde_json::to_vec(&manifest).unwrap())
        .unwrap();
    let mut source: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&query_path).unwrap().bytes()).unwrap();
    source["future_root"] = serde_json::json!({"keep": [1, 2]});
    source["query"]["future_query"] = serde_json::json!("preserve");
    source["query"]["filters"][0]["future_filter"] = serde_json::json!({"preserve": true});
    project
        .set_authoring_document(&query_path, serde_json::to_vec(&source).unwrap())
        .unwrap();

    let updated = SavedQueryDraft {
        id: "organizations".into(),
        name: "组织草稿".into(),
        query,
    };
    let stale = project.content_baseline();
    project.save_saved_query(updated.clone(), &stale).unwrap();
    let written: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&query_path).unwrap().bytes()).unwrap();
    assert_eq!(written["future_root"]["keep"][1], 2);
    assert_eq!(written["query"]["future_query"], "preserve");
    assert_eq!(
        written["query"]["filters"][0]["future_filter"]["preserve"],
        true
    );
    let manifest_written: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&manifest_path).unwrap().bytes())
            .unwrap();
    assert_eq!(manifest_written["extension"]["preserve"], true);

    let mut unknown = written.clone();
    unknown["query"]["schema_version"] = serde_json::json!(7);
    project
        .set_authoring_document(&query_path, serde_json::to_vec(&unknown).unwrap())
        .unwrap();
    let original_bytes = project
        .authoring_document(&query_path)
        .unwrap()
        .bytes()
        .to_vec();
    let error = project
        .save_saved_query(updated, &project.content_baseline())
        .unwrap_err();
    assert!(
        error.contains("未知") || error.contains("不支持"),
        "{error}"
    );
    assert_eq!(
        project.authoring_document(&query_path).unwrap().bytes(),
        original_bytes
    );
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn saved_query_filter_extensions_follow_their_dimension_when_reordered() {
    let mut project = project("filter-extension-order", "entity keeper kind place\n");
    let query_path = project.root.join(".world/queries/filters.json");
    let initial = SavedQueryDraft {
        id: "filters".into(),
        name: "筛选器".into(),
        query: CatalogQuery {
            schema_version: 1,
            filters: vec![
                CatalogQueryFilter::Kind {
                    values: vec!["entity".into()],
                    negate: false,
                },
                CatalogQueryFilter::Name {
                    values: vec!["keeper".into()],
                    negate: false,
                },
            ],
        },
    };
    project
        .save_saved_query(initial, &project.content_baseline())
        .unwrap();
    let mut source: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&query_path).unwrap().bytes()).unwrap();
    source["query"]["filters"][0]["future_filter"] = serde_json::json!({"owner":"kind"});
    source["query"]["filters"][1]["future_filter"] = serde_json::json!({"owner":"name"});
    project
        .set_authoring_document(&query_path, serde_json::to_vec(&source).unwrap())
        .unwrap();

    project
        .save_saved_query(
            SavedQueryDraft {
                id: "filters".into(),
                name: "筛选器已调整".into(),
                query: CatalogQuery {
                    schema_version: 1,
                    filters: vec![
                        CatalogQueryFilter::Name {
                            values: vec!["新名称".into()],
                            negate: false,
                        },
                        CatalogQueryFilter::Kind {
                            values: vec!["entity".into()],
                            negate: false,
                        },
                    ],
                },
            },
            &project.content_baseline(),
        )
        .unwrap();

    let written: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&query_path).unwrap().bytes()).unwrap();
    let filters = written["query"]["filters"].as_array().unwrap();
    assert_eq!(filters[0]["dimension"], "name");
    assert_eq!(filters[0]["future_filter"]["owner"], "name");
    assert_eq!(filters[1]["dimension"], "kind");
    assert_eq!(filters[1]["future_filter"]["owner"], "kind");
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn saved_query_with_unknown_required_capability_is_read_only_and_preserved() {
    static NEXT: AtomicUsize = AtomicUsize::new(2000);
    let root = std::env::temp_dir().join(format!(
        "worldline-saved-query-unknown-capability-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/queries")).unwrap();
    fs::write(root.join("world.wl"), "character traveler as \"旅人\"\n").unwrap();
    let manifest = br#"{"schema_version":1,"project_id":"future_query","language_version":"1.9","entry":"world.wl","required_features":["catalog.saved_queries.v1","catalog.future_query.v2"],"saved_queries":{"saved":".world/queries/saved.json"},"maps":{},"graph_views":{}}"#;
    let saved = r#"{"schema_version":1,"id":"saved","name":"旧查询","query":{"schema_version":1,"filters":[]},"future":"保留"}"#;
    fs::write(root.join(".world/project.json"), manifest).unwrap();
    fs::write(root.join(".world/queries/saved.json"), saved.as_bytes()).unwrap();
    let mut project = Project::open(&root).unwrap();
    let query_path = root.join(".world/queries/saved.json");
    let manifest_path = root.join(".world/project.json");
    let original_query = project
        .authoring_document(&query_path)
        .unwrap()
        .bytes()
        .to_vec();
    let original_manifest = project
        .authoring_document(&manifest_path)
        .unwrap()
        .bytes()
        .to_vec();
    assert!(project.saved_query_index().queries["saved"].read_only);

    let error = project
        .save_saved_query(
            SavedQueryDraft {
                id: "saved".into(),
                name: "覆盖尝试".into(),
                query: CatalogQuery::default(),
            },
            &project.content_baseline(),
        )
        .unwrap_err();

    assert!(
        error.contains("可写") || error.contains("未知") || error.contains("能力"),
        "{error}"
    );
    assert_eq!(
        project.authoring_document(&query_path).unwrap().bytes(),
        original_query
    );
    assert_eq!(
        project.authoring_document(&manifest_path).unwrap().bytes(),
        original_manifest
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn saved_query_missing_declared_capability_cannot_be_edited() {
    static NEXT: AtomicUsize = AtomicUsize::new(3000);
    let root = std::env::temp_dir().join(format!(
        "worldline-saved-query-missing-capability-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/queries")).unwrap();
    fs::write(root.join("world.wl"), "character traveler as \"旅人\"\n").unwrap();
    let manifest = r#"{"schema_version":1,"project_id":"missing_query_feature","language_version":"1.9","entry":"world.wl","required_features":[],"saved_queries":{"saved":".world/queries/saved.json"},"maps":{},"graph_views":{}}"#;
    let saved = r#"{"schema_version":1,"id":"saved","name":"旧查询","query":{"schema_version":1,"filters":[]},"future":"保留"}"#;
    fs::write(root.join(".world/project.json"), manifest).unwrap();
    fs::write(root.join(".world/queries/saved.json"), saved.as_bytes()).unwrap();
    let mut project = Project::open(&root).unwrap();
    let query_path = root.join(".world/queries/saved.json");
    let original = project
        .authoring_document(&query_path)
        .unwrap()
        .bytes()
        .to_vec();

    let error = project
        .save_saved_query(
            SavedQueryDraft {
                id: "saved".into(),
                name: "覆盖尝试".into(),
                query: CatalogQuery::default(),
            },
            &project.content_baseline(),
        )
        .unwrap_err();

    assert!(
        error.contains("只读") || error.contains("不支持"),
        "{error}"
    );
    assert!(project
        .saved_query_index()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "WS003"));
    assert_eq!(
        project.authoring_document(&query_path).unwrap().bytes(),
        original
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn saved_query_update_preserves_registered_document_with_mismatched_id() {
    static NEXT: AtomicUsize = AtomicUsize::new(5000);
    let root = std::env::temp_dir().join(format!(
        "worldline-saved-query-mismatched-id-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/queries")).unwrap();
    fs::write(root.join("world.wl"), "character traveler as \"旅人\"\n").unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"mismatched_query","language_version":"1.9","entry":"world.wl","required_features":["catalog.saved_queries.v1"],"saved_queries":{"saved":".world/queries/saved.json"},"maps":{},"graph_views":{}}"#,
    )
    .unwrap();
    let original = r#"{"schema_version":1,"id":"different","name":"不可见查询","query":{"schema_version":1,"filters":[]},"future":"保留"}"#;
    fs::write(root.join(".world/queries/saved.json"), original.as_bytes()).unwrap();
    let mut project = Project::open(&root).unwrap();
    let query_path = root.join(".world/queries/saved.json");
    let original_bytes = project
        .authoring_document(&query_path)
        .unwrap()
        .bytes()
        .to_vec();
    assert!(!project.saved_query_index().queries.contains_key("saved"));

    let result = project.save_saved_query(
        SavedQueryDraft {
            id: "saved".into(),
            name: "尝试覆盖错配文档".into(),
            query: CatalogQuery::default(),
        },
        &project.content_baseline(),
    );

    assert!(result.is_err(), "错配文档必须拒绝更新");
    assert_eq!(
        project.authoring_document(&query_path).unwrap().bytes(),
        original_bytes
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn query_cursor_pages_stably_and_rejects_a_changed_project_snapshot() {
    let mut project = project(
        "paging",
        concat!(
            "entity charlie kind person as \"Charlie\"\n",
            "entity alpha kind person as \"Alpha\"\n",
            "entity bravo kind person as \"Bravo\"\n",
        ),
    );
    let query = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Kind {
            values: vec!["entity".into()],
            negate: false,
        }],
    };
    let options = CatalogQueryOptions {
        page_size: 2,
        ..CatalogQueryOptions::default()
    };
    let source_path = project.root.join("world.wl");
    let disk_source = fs::read_to_string(&source_path).unwrap();
    fs::write(
        &source_path,
        format!("{disk_source}entity disk_only kind person as \"磁盘对象\"\n"),
    )
    .unwrap();
    let first = project.query_catalog(&query, options).unwrap();
    assert_eq!(first.total, 3);
    assert_eq!(first.items[0].target.id, "alpha");
    assert_eq!(first.items[1].target.id, "bravo");
    let cursor = first.next.unwrap();
    assert_eq!(cursor.offset, 2);

    let second = project.continue_catalog_query(&query, &cursor).unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].target.id, "charlie");
    assert!(second.next.is_none());
    assert!(project
        .continue_catalog_query(
            &CatalogQuery {
                schema_version: 1,
                filters: vec![CatalogQueryFilter::Kind {
                    values: vec!["character".into()],
                    negate: false,
                }],
            },
            &cursor,
        )
        .is_err());

    let mut source = project.document(&source_path).unwrap().to_owned();
    source.push_str("entity delta kind person as \"Delta\"\n");
    project.set_text(&source_path, source).unwrap();
    assert_eq!(
        project.continue_catalog_query(&query, &cursor).unwrap_err(),
        QueryError::StaleCursor
    );
    assert_eq!(project.query_catalog(&query, options).unwrap().total, 4);
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn query_cursor_preserves_the_initial_candidate_budget() {
    let mut source = String::new();
    for index in 0..10_001 {
        source.push_str(&format!("entity e{index} kind person\n"));
    }
    let project = project("cursor-budget", &source);
    let query = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Kind {
            values: vec!["entity".into()],
            negate: false,
        }],
    };
    let first = project
        .query_catalog(
            &query,
            CatalogQueryOptions {
                page_size: 1,
                max_candidates: 20_000,
                ..CatalogQueryOptions::default()
            },
        )
        .unwrap();
    assert_eq!(first.total, 10_001);
    let cursor = first.next.unwrap();
    assert_eq!(cursor.max_candidates, 20_000);

    let second = project.continue_catalog_query(&query, &cursor).unwrap();

    assert_eq!(second.offset, 1);
    assert_eq!(second.items.len(), 1);
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn query_semantics_cover_empty_or_negation_cycles_and_unknown_property_types() {
    let project = project(
        "semantics",
        concat!(
            "entity a kind organization as \"Alpha\"\n",
            "  property status = \"draft\"\n",
            "entity b kind place as \"Beta\"\n",
            "  property status = \"ready\"\n",
            "entity c kind place as \"Gamma\"\n",
            "tag linked_alpha as \"Alpha 标签\"\n",
            "tag linked_beta as \"Beta 标签\"\n",
            "mark tag linked_alpha with linked_beta\n",
            "mark tag linked_beta with linked_alpha\n",
            "mark entity a with linked_beta\n",
            "relation_type connects as \"连接\"\n",
            "relation_def ab type connects from entity a to entity b\n",
            "relation_def bc type connects from entity b to entity c\n",
            "relation_def ca type connects from entity c to entity a\n",
        ),
    );

    let empty_or = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Kind {
            values: vec![],
            negate: false,
        }],
    };
    assert_eq!(
        project
            .query_catalog(&empty_or, CatalogQueryOptions::default())
            .unwrap()
            .total,
        0
    );
    let negated_empty_or = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Kind {
            values: vec![],
            negate: true,
        }],
    };
    assert!(
        project
            .query_catalog(&negated_empty_or, CatalogQueryOptions::default())
            .unwrap()
            .total
            > 0
    );

    let negative_property = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Property {
            values: vec![worldline_core::queries::PropertyCondition {
                key: "status".into(),
                equals: PropertyScalar::String("draft".into()),
            }],
            negate: true,
        }],
    };
    let page = project
        .query_catalog(&negative_property, CatalogQueryOptions::default())
        .unwrap();
    assert!(page.items.iter().any(|item| item.target.id == "b"));
    assert!(!page.items.iter().any(|item| item.target.id == "a"));

    let recursive_tag = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Tag {
            values: vec!["linked_alpha".into()],
            recursive: true,
            negate: false,
        }],
    };
    assert!(project
        .query_catalog(&recursive_tag, CatalogQueryOptions::default())
        .unwrap()
        .items
        .iter()
        .any(|item| item.target == TargetRef::new("entity", "a")));

    let author_scope = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::AuthorScope {
            source_files: vec!["world.wl".into()],
            negate: false,
        }],
    };
    assert!(
        project
            .query_catalog(&author_scope, CatalogQueryOptions::default())
            .unwrap()
            .total
            > 0
    );
    let bad_author_scope = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::AuthorScope {
            source_files: vec!["../outside.wl".into()],
            negate: false,
        }],
    };
    assert!(project
        .query_catalog(&bad_author_scope, CatalogQueryOptions::default())
        .is_err());

    let outgoing = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Relation {
            values: vec![worldline_core::queries::RelationCondition {
                relation_type: Some("connects".into()),
                direction: worldline_core::queries::RelationDirection::Outgoing,
                related: Some(TargetRef::new("entity", "b")),
            }],
            negate: false,
        }],
    };
    let related = project
        .query_catalog(&outgoing, CatalogQueryOptions::default())
        .unwrap();
    assert_eq!(
        related
            .items
            .iter()
            .map(|item| item.target.id.as_str())
            .collect::<Vec<_>>(),
        ["a"]
    );

    let unknown_property_type = serde_json::json!({
        "schema_version": 1,
        "filters": [{
            "dimension": "property",
            "values": [{"key":"status","equals":{"type":"date","value":"today"}}]
        }]
    });
    assert!(serde_json::from_value::<CatalogQuery>(unknown_property_type).is_err());
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn query_summary_is_readable_and_or_values_are_bounded() {
    let project = project("summary", "entity keeper kind place as \"守护者\"\n");
    let query = CatalogQuery {
        schema_version: 1,
        filters: vec![
            CatalogQueryFilter::Relation {
                values: vec![worldline_core::queries::RelationCondition {
                    relation_type: Some("connects".into()),
                    direction: worldline_core::queries::RelationDirection::Outgoing,
                    related: Some(TargetRef::new("entity", "keeper")),
                }],
                negate: false,
            },
            CatalogQueryFilter::Property {
                values: vec![worldline_core::queries::PropertyCondition {
                    key: "status".into(),
                    equals: PropertyScalar::String("draft".into()),
                }],
                negate: false,
            },
        ],
    };
    assert_eq!(
        query.summary(),
        "属性：status = 「draft」 且 明确关系：connects 出边 entity:keeper"
    );

    let too_many_values = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Name {
            values: vec!["候选".into(); 101],
            negate: false,
        }],
    };
    assert!(matches!(
        project.query_catalog(&too_many_values, CatalogQueryOptions::default()),
        Err(QueryError::InvalidQuery(_))
    ));
}

#[test]
#[ignore = "manual D5 query profile; run with --ignored --nocapture to record cold/warm P95"]
fn d5_profile_1000_objects_3000_relations_and_long_text() {
    use std::time::Instant;

    let mut source = String::with_capacity(256_000);
    source.push_str("relation_type links as \"连接\"\n");
    for index in 0..1_000 {
        source.push_str(&format!("entity e{index} kind person as \"人物{index}\"\n"));
    }
    for index in 0..3_000 {
        source.push_str(&format!(
            "relation_def r{index} type links from entity e{} to entity e{}\n",
            index % 1_000,
            (index + 1) % 1_000
        ));
    }
    let long_text = "海港".repeat(8_000);
    source.push_str("event long_text\n  ");
    source.push_str(&long_text);
    source.push_str("\n  -> END\n");

    let project = project("d5_profile", &source);
    let query = CatalogQuery {
        schema_version: 1,
        filters: vec![CatalogQueryFilter::Relation {
            values: vec![worldline_core::queries::RelationCondition {
                relation_type: Some("links".into()),
                direction: worldline_core::queries::RelationDirection::Either,
                related: None,
            }],
            negate: false,
        }],
    };
    let started = Instant::now();
    let first = project
        .query_catalog(&query, CatalogQueryOptions::default())
        .unwrap();
    let cold_micros = started.elapsed().as_micros();
    assert_eq!(first.total, 1_000);

    let mut warm_micros = Vec::with_capacity(20);
    for _ in 0..20 {
        let started = Instant::now();
        let page = project
            .query_catalog(&query, CatalogQueryOptions::default())
            .unwrap();
        warm_micros.push(started.elapsed().as_micros());
        assert_eq!(page.total, 1_000);
    }
    warm_micros.sort_unstable();
    let p95_index = (warm_micros.len() * 95).div_ceil(100) - 1;
    let p95 = warm_micros[p95_index];
    println!(
        "D5 query profile: entities=1000, relations=3000, long_text_chars={}, cold_us={}, warm_samples={}, warm_p95_us={}, results={}",
        long_text.chars().count(),
        cold_micros,
        warm_micros.len(),
        p95,
        first.total,
    );
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn query_enforces_candidate_budget_and_returns_no_partial_results_when_cancelled() {
    let mut source = String::new();
    for index in 0..70 {
        source.push_str(&format!("entity e{index} kind thing as \"对象{index}\"\n"));
    }
    let project = project("budget", &source);
    let query = CatalogQuery::default();
    let budget = project
        .query_catalog(
            &query,
            CatalogQueryOptions {
                max_candidates: 10,
                ..CatalogQueryOptions::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        budget,
        QueryError::CandidateBudgetExceeded { candidates, budget: 10 } if candidates > 10
    ));
    let mut checks = 0;
    assert_eq!(
        project
            .query_catalog_cancellable(&query, CatalogQueryOptions::default(), || {
                checks += 1;
                checks == 2
            })
            .unwrap_err(),
        QueryError::Cancelled
    );
    assert_eq!(checks, 2);
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn compound_query_ors_within_a_dimension_and_ands_across_dimensions() {
    let project = project(
        "compound",
        concat!(
            "entity keepers kind organization as \"守灯会\"\n",
            "  property status = \"draft\"\n",
            "entity lighthouse kind place as \"雾港灯塔\"\n",
            "  property status = \"published\"\n",
            "alias entity keepers as \"灯塔组织\"\n",
        ),
    );
    let query = CatalogQuery {
        schema_version: 1,
        filters: vec![
            CatalogQueryFilter::Kind {
                values: vec!["entity".into(), "character".into()],
                negate: false,
            },
            CatalogQueryFilter::Name {
                values: vec!["不存在".into(), "灯塔组织".into()],
                negate: false,
            },
            CatalogQueryFilter::Property {
                values: vec![worldline_core::queries::PropertyCondition {
                    key: "status".into(),
                    equals: PropertyScalar::String("draft".into()),
                }],
                negate: false,
            },
            CatalogQueryFilter::Missing {
                values: vec![MissingCondition::Property {
                    key: "source".into(),
                }],
                negate: false,
            },
        ],
    };

    let page = project
        .query_catalog(&query, CatalogQueryOptions::default())
        .unwrap();

    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].target, TargetRef::new("entity", "keepers"));
    assert!(page.items[0].source.file.ends_with("world.wl"));
    assert!(page.items[0].source.line > 0);
    assert_eq!(page.items[0].reasons.len(), 4);
    assert!(page.summary.contains("名称"));
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn todo_projection_groups_missing_targets_and_is_read_only_and_deterministic() {
    static NEXT: AtomicUsize = AtomicUsize::new(2000);
    let root = std::env::temp_dir().join(format!(
        "worldline-catalog-todos-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/comments")).unwrap();
    fs::create_dir_all(root.join(".world/proposals")).unwrap();
    let source = concat!(
        "entity keepers kind organization as \"守灯会\"\n",
        "event start\n",
        "  参见 [[entity:missing_place|失落地点]] 与 [[entity:missing_place|另一个失落地点]]。\n",
        "  -> END\n",
    );
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"todo_test","language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","collaboration.comments.v1","collaboration.proposals.v1"],"maps":{},"graph_views":{},"comments":{"detached":".world/comments/detached.json","resolved":".world/comments/resolved.json"},"proposals":{"open":".world/proposals/open.json","accepted":".world/proposals/accepted.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/comments/detached.json"),
        r#"{"schema_version":1,"id":"detached","author":"甲","body":"检查失落地点","anchor":{"kind":"object","target":{"kind":"entity","id":"missing_place"}},"resolved":false}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/comments/resolved.json"),
        r#"{"schema_version":1,"id":"resolved","author":"甲","body":"已处理","anchor":{"kind":"object","target":{"kind":"entity","id":"missing_place"}},"resolved":true}"#,
    )
    .unwrap();
    for (id, status) in [("open", "open"), ("accepted", "accepted")] {
        fs::write(
            root.join(format!(".world/proposals/{id}.json")),
            format!(
                r#"{{"schema_version":1,"id":"{id}","author":"甲","reason":"审阅改动","status":"{status}","changes":[{{"path":"world.wl","domain":"content","base":"旧文本","proposed":"新文本"}}]}}"#
            ),
        )
        .unwrap();
    }
    let mut project = Project::open(&root).unwrap();
    let baseline = project.content_baseline();
    let sources = project.sources();
    let fingerprint = project.compile().analysis.fingerprint;

    let first = project.todo_projection();
    let second = project.todo_projection();

    let kinds: Vec<_> = first.items.iter().map(|item| item.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TodoKind::BrokenLink,
            TodoKind::BrokenLink,
            TodoKind::EntryToCreate,
            TodoKind::DetachedComment,
            TodoKind::OpenProposal,
        ]
    );
    assert_eq!(
        first.items[0].target,
        TargetRef::new("entity", "missing_place")
    );
    assert_eq!(
        first.items[1].target,
        TargetRef::new("entity", "missing_place")
    );
    assert!(first.items[2].reason.contains("2 处"));
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        second
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.sources(), sources);
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let _ = fs::remove_dir_all(root);
}

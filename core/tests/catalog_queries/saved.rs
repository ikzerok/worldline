use super::*;
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

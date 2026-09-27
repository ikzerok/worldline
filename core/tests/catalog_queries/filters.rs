use super::*;
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

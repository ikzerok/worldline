use super::*;
use serde_json::{json, Value};
use worldline_core::queries::{CatalogQuerySort, CatalogSortDirection, CatalogSortField};

fn sorted(field: CatalogSortField, direction: CatalogSortDirection) -> CatalogQuery {
    let mut query = CatalogQuery::default();
    query.filters.push(CatalogQueryFilter::Kind {
        values: vec!["entity".into(), "character".into()],
        negate: false,
    });
    query.set_sort(Some(CatalogQuerySort { field, direction }));
    query
}

fn ids(project: &Project, query: &CatalogQuery, size: usize) -> Vec<String> {
    let mut page = project
        .query_catalog(
            query,
            CatalogQueryOptions {
                page_size: size,
                ..CatalogQueryOptions::default()
            },
        )
        .unwrap();
    let total = page.total;
    let mut result = Vec::new();
    loop {
        assert_eq!(page.schema_version, 1);
        result.extend(
            page.items
                .iter()
                .map(|item| format!("{}:{}", item.target.kind, item.target.id)),
        );
        let Some(cursor) = page.next else { break };
        assert_eq!(cursor.schema_version, 1);
        page = project.continue_catalog_query(query, &cursor).unwrap();
    }
    assert_eq!(result.len(), total);
    assert_eq!(
        result
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        total
    );
    result
}

#[test]
fn explicit_name_sort_is_global_stable_unicode_and_missing_last_in_both_directions() {
    let mut project = project(
        "sort-names",
        concat!(
            "entity z kind place as \"Alpha\"\n",
            "entity a kind place as \"alpha\"\n",
            "character c as \"ALPHA\"\n",
            "entity ten kind place as \"10\"\n",
            "entity two kind place as \"2\"\n",
            "entity zhang kind place as \"张\"\n",
            "entity li kind place as \"李\"\n",
            "entity empty kind place as \"\"\n",
            "entity blank kind place as \"  \"\n",
        ),
    );
    let baseline = project.content_baseline();
    let sources = project.sources();
    let fingerprint = project.compile().analysis.fingerprint;
    let mut query = sorted(CatalogSortField::Name, CatalogSortDirection::Ascending);
    let ascending = ids(&project, &query, 2);
    assert_eq!(
        ascending,
        [
            "entity:ten",
            "entity:two",
            "character:c",
            "entity:a",
            "entity:z",
            "entity:zhang",
            "entity:li",
            "entity:blank",
            "entity:empty"
        ]
    );
    assert_eq!(ascending, ids(&project, &query, 100));
    query.set_sort(Some(CatalogQuerySort {
        field: CatalogSortField::Name,
        direction: CatalogSortDirection::Descending,
    }));
    let descending = ids(&project, &query, 3);
    assert_eq!(
        descending,
        [
            "entity:li",
            "entity:zhang",
            "character:c",
            "entity:a",
            "entity:z",
            "entity:two",
            "entity:ten",
            "entity:blank",
            "entity:empty"
        ]
    );
    assert_eq!(descending, ids(&project, &query, 100));
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.sources(), sources);
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn kind_sort_only_reverses_primary_key_and_restores_v1_identity_order() {
    let project = project("sort-kinds", "entity z kind place as \"A\"\nentity a kind place as \"Z\"\ncharacter z as \"A\"\ncharacter a as \"Z\"\n");
    let mut query = sorted(CatalogSortField::Kind, CatalogSortDirection::Descending);
    assert_eq!(
        ids(&project, &query, 1),
        ["entity:a", "entity:z", "character:a", "character:z"]
    );
    query.set_sort(Some(CatalogQuerySort {
        field: CatalogSortField::Kind,
        direction: CatalogSortDirection::Ascending,
    }));
    let ascending = ids(&project, &query, 2);
    query.set_sort(None);
    assert_eq!(query.schema_version, 1);
    assert!(serde_json::to_value(&query).unwrap().get("sort").is_none());
    assert_eq!(ids(&project, &query, 2), ascending);
    assert_eq!(
        serde_json::to_string(&CatalogQuery::default()).unwrap(),
        r#"{"schema_version":1,"filters":[]}"#
    );
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn sort_changes_invalidate_cursors_and_cancel_after_filtering_returns_no_page() {
    let project = project(
        "sort-cursor",
        "entity a kind place as \"Z\"\nentity b kind place as \"A\"\n",
    );
    let mut query = sorted(CatalogSortField::Name, CatalogSortDirection::Ascending);
    let page = project
        .query_catalog(
            &query,
            CatalogQueryOptions {
                page_size: 1,
                ..CatalogQueryOptions::default()
            },
        )
        .unwrap();
    assert_eq!(page.items[0].display, "A");
    let cursor = page.next.unwrap();
    query.set_sort(Some(CatalogQuerySort {
        field: CatalogSortField::Name,
        direction: CatalogSortDirection::Descending,
    }));
    assert_eq!(
        project.continue_catalog_query(&query, &cursor).unwrap_err(),
        QueryError::StaleCursor
    );
    query.set_sort(None);
    assert_eq!(
        project.continue_catalog_query(&query, &cursor).unwrap_err(),
        QueryError::StaleCursor
    );
    let baseline = project.content_baseline();
    for cancel_at in [2, 3] {
        let mut checks = 0;
        assert_eq!(
            project
                .query_catalog_cancellable(&query, CatalogQueryOptions::default(), || {
                    checks += 1;
                    checks == cancel_at
                })
                .unwrap_err(),
            QueryError::Cancelled
        );
    }
    assert_eq!(project.content_baseline(), baseline);
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn sort_dto_rejects_unknown_semantics_and_mismatched_versions() {
    let project = project("sort-invalid", "entity a kind place\n");
    for invalid in [
        json!({"schema_version":1,"sort":{"field":"name","direction":"ascending"}}),
        json!({"schema_version":2}),
        json!({"schema_version":7}),
    ] {
        let query: CatalogQuery = serde_json::from_value(invalid).unwrap();
        assert!(matches!(
            project.query_catalog(&query, CatalogQueryOptions::default()),
            Err(QueryError::InvalidQuery(_))
        ));
    }
    for sort in [
        json!({"field":"property","direction":"ascending"}),
        json!({"field":"name","direction":"sideways"}),
        json!({"field":"name","direction":"ascending","collation":"future"}),
    ] {
        assert!(
            serde_json::from_value::<CatalogQuery>(json!({"schema_version":2,"sort":sort}))
                .is_err()
        );
    }
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn saved_sort_is_document_scoped_and_default_removes_only_its_known_metadata() {
    let mut project = project("sort-saved", "entity a kind place\n");
    let sources = project.sources();
    let fingerprint = project.compile().analysis.fingerprint;
    let mut draft = SavedQueryDraft {
        id: "sorted".into(),
        name: "资料".into(),
        query: sorted(CatalogSortField::Name, CatalogSortDirection::Descending),
    };
    project
        .save_saved_query(draft.clone(), &project.content_baseline())
        .unwrap();
    let path = project.root.join(".world/queries/sorted.json");
    let manifest_path = project.root.join(".world/project.json");
    let manifest: Value =
        serde_json::from_slice(project.authoring_document(&manifest_path).unwrap().bytes())
            .unwrap();
    assert!(!manifest["required_features"]
        .as_array()
        .unwrap()
        .contains(&json!("catalog.query_sort.v1")));
    let mut value: Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(value["required_features"], json!(["catalog.query_sort.v1"]));
    value["required_features"]
        .as_array_mut()
        .unwrap()
        .push(json!("catalog.saved_queries.v1"));
    value["future"] = json!({"preserve":true});
    value["query"]["future_query"] = json!([3, 2, 1]);
    project
        .set_authoring_document(&path, serde_json::to_vec(&value).unwrap())
        .unwrap();
    project.save().unwrap();
    let mut project = Project::open(&project.root).unwrap();
    assert_eq!(project.saved_query_index().queries["sorted"].draft, draft);
    draft.query.set_sort(None);
    project
        .save_saved_query(draft, &project.content_baseline())
        .unwrap();
    let restored: Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(restored["query"]["schema_version"], 1);
    assert!(restored["query"].get("sort").is_none());
    assert_eq!(
        restored["required_features"],
        json!(["catalog.saved_queries.v1"])
    );
    assert_eq!(restored["future"], value["future"]);
    assert_eq!(
        restored["query"]["future_query"],
        value["query"]["future_query"]
    );
    project.save().unwrap();
    let mut project = Project::open(&project.root).unwrap();
    assert!(project.saved_query_index().queries["sorted"]
        .draft
        .query
        .sort
        .is_none());
    assert_eq!(project.sources(), sources);
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let _ = fs::remove_dir_all(project.root);
}

#[test]
fn malformed_sort_documents_are_preserved_without_partial_writes() {
    let mut project = project("sort-protect", "entity a kind place\n");
    let draft = SavedQueryDraft {
        id: "sorted".into(),
        name: "资料".into(),
        query: sorted(CatalogSortField::Name, CatalogSortDirection::Ascending),
    };
    project
        .save_saved_query(draft.clone(), &project.content_baseline())
        .unwrap();
    let path = project.root.join(".world/queries/sorted.json");
    let source: Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    let mut invalids = Vec::new();
    let mut missing = source.clone();
    missing.as_object_mut().unwrap().remove("required_features");
    invalids.push(missing);
    let mut unknown = source.clone();
    unknown["query"]["sort"]["field"] = json!("future");
    invalids.push(unknown);
    let mut old = source;
    old["query"]["schema_version"] = json!(1);
    invalids.push(old);
    for invalid in invalids {
        let mut candidate = project.clone();
        let bytes = serde_json::to_vec(&invalid).unwrap();
        candidate
            .set_authoring_document(&path, bytes.clone())
            .unwrap();
        let baseline = candidate.content_baseline();
        assert!(!candidate.saved_query_index().queries.contains_key("sorted"));
        assert!(candidate
            .save_saved_query(draft.clone(), &baseline)
            .is_err());
        assert_eq!(candidate.content_baseline(), baseline);
        assert_eq!(candidate.authoring_document(&path).unwrap().bytes(), bytes);
    }
    let _ = fs::remove_dir_all(project.root);
}

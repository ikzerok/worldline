use serde_json::{json, Value};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::queries::{
    CatalogQuery, CatalogQueryFilter, CatalogQueryOptions, CatalogQuerySort, CatalogSortDirection,
    CatalogSortField, PropertyScalar, QueryError, SavedQueryDraft,
    CATALOG_QUERY_REFERENCE_REQUIRED_FEATURE, CATALOG_QUERY_SORT_REQUIRED_FEATURE,
};

fn fixture() -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "wl-reference-query-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.13","entry":"world.wl","required_features":["content.entities.v1","content.relations.v1","content.object_refs.v1","content.character_refs.v1"]}"#).unwrap();
    fs::write(
        root.join("world.wl"),
        concat!(
            "entity shared kind place as \"同名目标\"\n",
            "character shared as \"同名目标\"\n",
            "entity other kind place as \"同名目标\"\n",
            "relation_type connects\n",
            "relation_def edge type connects from entity shared to entity other\n",
            "entity record_a kind record as \"甲\"\n",
            "  property destination = ref(\"entity\", \"shared\")\n",
            "  property note = \"\"\n",
            "  property amount = 0\n",
            "  property approved = false\n",
            "entity record_b kind record as \"乙\"\n",
            "  property destination = ref(\"character\", \"shared\")\n",
            "  property note = \" \"\n",
            "entity record_c kind record as \"丙\"\n",
            "entity record_d kind record as \"丁\"\n",
            "  property destination = \"shared\"\n",
            "entity record_e kind record as \"戊\"\n",
            "  property destination = ref(\"relation\", \"edge\")\n",
            "event start\n  -> END\n",
        ),
    )
    .unwrap();
    let mut project = Project::open(&root).unwrap();
    assert!(!project.compile().has_errors());
    project
}

fn query(value: Value) -> CatalogQuery {
    serde_json::from_value(json!({
        "schema_version": if value["type"] == "reference" { 3 } else { 1 },
        "filters": [
            {"dimension":"name","values":["record_"]},
            {"dimension":"property","values":[{"key":"destination","equals":value}]}
        ]
    }))
    .unwrap()
}

fn reference(kind: &str, id: &str) -> Value {
    json!({"type":"reference","value":{"kind":kind,"id":id}})
}

fn ids(project: &Project, query: &CatalogQuery) -> Vec<String> {
    project
        .query_catalog(query, CatalogQueryOptions::default())
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.target.id)
        .collect()
}

fn draft(query: CatalogQuery) -> SavedQueryDraft {
    SavedQueryDraft {
        id: "destinations".into(),
        name: "资料归属".into(),
        query,
    }
}

#[test]
fn reference_values_compare_full_typed_identity_without_string_or_name_inference() {
    let project = fixture();
    assert_eq!(
        ids(&project, &query(reference("entity", "shared"))),
        ["record_a"]
    );
    assert_eq!(
        ids(&project, &query(reference("character", "shared"))),
        ["record_b"]
    );
    assert_eq!(
        ids(&project, &query(reference("relation", "edge"))),
        ["record_e"]
    );
    assert!(ids(&project, &query(reference("entity", "other"))).is_empty());
    assert!(ids(&project, &query(reference("entity", "absent"))).is_empty());
    assert_eq!(
        ids(&project, &query(json!({"type":"string","value":"shared"}))),
        ["record_d"]
    );
    assert!(ids(
        &project,
        &query(json!({"type":"string","value":"entity:shared"}))
    )
    .is_empty());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn empty_space_missing_false_and_zero_keep_distinct_existing_semantics() {
    let project = fixture();
    for (key, value, expected) in [
        ("note", json!({"type":"string","value":""}), "record_a"),
        ("note", json!({"type":"string","value":" "}), "record_b"),
        ("amount", json!({"type":"number","value":0}), "record_a"),
        (
            "approved",
            json!({"type":"boolean","value":false}),
            "record_a",
        ),
    ] {
        let mut q = query(value);
        if let CatalogQueryFilter::Property { values, .. } = &mut q.filters[1] {
            values[0].key = key.into();
        }
        assert_eq!(ids(&project, &q), [expected]);
    }
    let missing: CatalogQuery = serde_json::from_value(json!({"schema_version":1,"filters":[
        {"dimension":"name","values":["record_"]},
        {"dimension":"missing","values":[{"kind":"property","key":"destination"}]}
    ]}))
    .unwrap();
    assert_eq!(ids(&project, &missing), ["record_c"]);
    let mut negated = query(reference("entity", "shared"));
    if let CatalogQueryFilter::Property { negate, .. } = &mut negated.filters[1] {
        *negate = true;
    }
    assert_eq!(
        ids(&project, &negated),
        ["record_b", "record_c", "record_d", "record_e"]
    );
    negated.filters.push(serde_json::from_value(json!({"dimension":"missing","negate":true,"values":[{"kind":"property","key":"destination"}]})).unwrap());
    assert_eq!(
        ids(&project, &negated),
        ["record_b", "record_d", "record_e"]
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn new_values_require_explicit_v3_and_edits_do_not_normalize_unknown_versions() {
    let project = fixture();
    let mut q = query(reference("entity", "shared"));
    for version in [1, 2, 4] {
        q.schema_version = version;
        assert!(matches!(
            project.query_catalog(&q, Default::default()),
            Err(QueryError::InvalidQuery(_))
        ));
    }
    q.schema_version = 3;
    let sort = Some(CatalogQuerySort {
        field: CatalogSortField::Name,
        direction: CatalogSortDirection::Descending,
    });
    q.set_sort(sort);
    assert_eq!(q.schema_version, 3);
    assert!(project.query_catalog(&q, Default::default()).is_ok());
    q.set_sort(None);
    assert_eq!(q.schema_version, 3);
    for (kind, id) in [
        ("event", "start"),
        ("entity", ""),
        ("entity", "shared other"),
    ] {
        assert!(project
            .query_catalog(&query(reference(kind, id)), Default::default())
            .is_err());
    }
    q.filters.clear();
    assert!(project.query_catalog(&q, Default::default()).is_err());
    q.sync_edited_version();
    assert_eq!(q.schema_version, 1);
    q.set_sort(sort);
    assert_eq!(q.schema_version, 2);
    q.schema_version = 99;
    q.sync_edited_version();
    q.set_sort(None);
    assert_eq!(q.schema_version, 99);
    assert!(project.query_catalog(&q, Default::default()).is_err());
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn typed_query_pagination_cancellation_and_stale_cursors_use_the_whole_condition() {
    let mut project = fixture();
    let mut q = query(reference("entity", "shared"));
    if let CatalogQueryFilter::Property { values, .. } = &mut q.filters[1] {
        let mut extra = values[0].clone();
        extra.equals = PropertyScalar::Reference(TargetRef::new("character", "shared"));
        values.push(extra);
    }
    let options = CatalogQueryOptions {
        page_size: 1,
        ..Default::default()
    };
    let first = project.query_catalog(&q, options).unwrap();
    assert_eq!(first.total, 2);
    let cursor = first.next.unwrap();
    assert_eq!(cursor.schema_version, 1);
    assert_eq!(
        project.continue_catalog_query(&q, &cursor).unwrap().items[0]
            .target
            .id,
        "record_b"
    );
    assert!(matches!(
        project.query_catalog_cancellable(&q, options, || true),
        Err(QueryError::Cancelled)
    ));
    assert!(matches!(
        project.continue_catalog_query(&query(reference("relation", "edge")), &cursor),
        Err(QueryError::StaleCursor)
    ));
    let path = project.root.join("world.wl");
    project
        .set_text(
            &path,
            format!("{}// 改稿\n", project.document(&path).unwrap()),
        )
        .unwrap();
    assert!(matches!(
        project.continue_catalog_query(&q, &cursor),
        Err(QueryError::StaleCursor)
    ));
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn saved_v3_capabilities_roundtrip_and_explicit_removal_preserve_extensions_and_sort() {
    let mut project = fixture();
    let mut q = query(reference("entity", "shared"));
    q.set_sort(Some(CatalogQuerySort {
        field: CatalogSortField::Name,
        direction: CatalogSortDirection::Ascending,
    }));
    let mut requested = draft(q);
    let fingerprint = project.compile().analysis.fingerprint;
    project
        .save_saved_query(requested.clone(), &project.content_baseline())
        .unwrap();
    let path = project.root.join(".world/queries/destinations.json");
    let mut document: Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    assert!(document["required_features"]
        .as_array()
        .unwrap()
        .contains(&json!(CATALOG_QUERY_REFERENCE_REQUIRED_FEATURE)));
    assert!(document["required_features"]
        .as_array()
        .unwrap()
        .contains(&json!(CATALOG_QUERY_SORT_REQUIRED_FEATURE)));
    document["extension"] = json!({"keep":"shared"});
    document["query"]["filters"][1]["extra"] = json!({"keep":true});
    project
        .set_authoring_document(&path, serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    project.save().unwrap();
    let mut reopened = Project::open(&project.root).unwrap();
    assert_eq!(
        reopened.saved_query_index().queries["destinations"]
            .draft
            .query,
        requested.query
    );
    if let CatalogQueryFilter::Property { values, .. } = &mut requested.query.filters[1] {
        values[0].equals = PropertyScalar::String("shared".into());
    }
    requested.query.sync_edited_version();
    assert_eq!(requested.query.schema_version, 2);
    reopened
        .save_saved_query(requested, &reopened.content_baseline())
        .unwrap();
    let saved: Value =
        serde_json::from_slice(reopened.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(saved["extension"], document["extension"]);
    assert_eq!(
        saved["query"]["filters"][1]["extra"],
        document["query"]["filters"][1]["extra"]
    );
    assert_eq!(
        saved["required_features"],
        json!([CATALOG_QUERY_SORT_REQUIRED_FEATURE])
    );
    assert_eq!(reopened.compile().analysis.fingerprint, fingerprint);
    assert_eq!(reopened.language_version(), "1.13");
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn saved_reference_conditions_rename_exactly_without_touching_other_json_bytes() {
    let mut project = fixture();
    project
        .save_saved_query(
            draft(query(reference("entity", "shared"))),
            &project.content_baseline(),
        )
        .unwrap();
    let path = project.root.join(".world/queries/destinations.json");
    let before = r#"{
  "schema_version": 1, "id": "destinations", "name": "资料 shared",
  "required_features": ["catalog.query_reference_values.v1"],
  "extra": {"id":"shared", "kind":"entity"},
  "query": {"schema_version":3, "filters":[
    {"dimension":"property", "values":[
      {"key":"destination", "equals":{"type":"reference", "value":{"kind":"entity", "id":"shared"}}},
      {"key":"destination", "equals":{"type":"string", "value":"shared"}}
    ]}
  ]}
}
"#;
    project
        .set_authoring_document(&path, before.as_bytes().to_vec())
        .unwrap();
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "shared"), "renamed")
        .unwrap();
    assert_eq!(
        project.authoring_document(&path).unwrap().bytes(),
        before.as_bytes()
    );
    let change = plan
        .changes
        .iter()
        .find(|change| change.path == path)
        .unwrap();
    assert_eq!(change.occurrences.len(), 1);
    assert!(change.occurrences[0]
        .field
        .as_deref()
        .expect("查询引用的 JSON Pointer 字段必须存在")
        .ends_with("/equals/value/id"));
    project.apply_rename_plan(&plan).unwrap();
    let expected = before.replace(
        "\"kind\":\"entity\", \"id\":\"shared\"",
        "\"kind\":\"entity\", \"id\":\"renamed\"",
    );
    assert_eq!(
        project.authoring_document(&path).unwrap().bytes(),
        expected.as_bytes()
    );
    assert_eq!(
        project.saved_query_index().queries["destinations"]
            .draft
            .query
            .schema_version,
        3
    );
    assert_eq!(
        ids(&project, &query(reference("entity", "renamed"))),
        ["record_a"]
    );
    fs::remove_dir_all(project.root).unwrap();
}

#[test]
fn missing_capability_unknown_feature_and_external_changes_refuse_saved_writes() {
    for mode in ["missing", "unknown", "external"] {
        let mut project = fixture();
        let requested = draft(query(reference("entity", "shared")));
        project
            .save_saved_query(requested.clone(), &project.content_baseline())
            .unwrap();
        let path = project.root.join(".world/queries/destinations.json");
        let mut doc: Value =
            serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
        match mode {
            "missing" => {
                doc["required_features"] = json!([]);
            }
            "unknown" => {
                doc["required_features"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("future.query.v8"));
            }
            _ => {}
        }
        if mode == "unknown" {
            project.save().unwrap();
            fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
            project = Project::open(&project.root).unwrap();
        } else {
            project
                .set_authoring_document(&path, serde_json::to_vec(&doc).unwrap())
                .unwrap();
            if mode == "external" {
                project.save().unwrap();
                fs::write(&path, b"external changed bytes").unwrap();
            }
        }
        let before = project.authoring_document(&path).unwrap().bytes().to_vec();
        let baseline = project.content_baseline();
        assert!(
            project.save_saved_query(requested, &baseline).is_err(),
            "{mode}"
        );
        assert_eq!(project.authoring_document(&path).unwrap().bytes(), before);
        assert_eq!(project.content_baseline(), baseline);
        if mode == "external" {
            assert_eq!(fs::read(&path).unwrap(), b"external changed bytes");
        }
        fs::remove_dir_all(project.root).unwrap();
    }
}

#[path = "catalog_reference_queries/refactor_guards.rs"]
mod refactor_guards;

#[test]
fn existing_end_identity_is_queryable_for_each_static_reference_kind() {
    let mut project = fixture();
    let path = project.root.join("world.wl");
    let extra = concat!(
        "entity END kind place as \"静态结束地\"\n",
        "character END as \"静态结束人物\"\n",
        "relation_def END type connects from entity END to entity other\n",
        "entity record_end_entity kind record\n",
        "  property destination = ref(\"entity\", \"END\")\n",
        "entity record_end_character kind record\n",
        "  property destination = ref(\"character\", \"END\")\n",
        "entity record_end_relation kind record\n",
        "  property destination = ref(\"relation\", \"END\")\n",
    );
    project
        .set_text(
            &path,
            format!("{}{extra}", project.document(&path).unwrap()),
        )
        .unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    for kind in ["entity", "character", "relation"] {
        assert_eq!(
            ids(&project, &query(reference(kind, "END"))),
            [format!("record_end_{kind}")]
        );
    }
    fs::remove_dir_all(project.root).unwrap();
}

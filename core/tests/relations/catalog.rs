use super::relation_source;
use worldline_core::{
    compile_source, compile_source_with_options, LanguageVersion, RelationQueryDirection,
    RelationQueryOptions, TargetRef,
};

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

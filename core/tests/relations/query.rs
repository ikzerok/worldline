use super::relation_source;
use worldline_core::{
    compile_source_with_options, RelationDirection, RelationQueryDirection, RelationQueryOptions,
    TargetRef,
};

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
    let zero_budgets = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("entity", "a"),
            worldline_core::TopicProjectionOptions {
                role_mapping: std::collections::BTreeMap::from([("links".into(), "连接".into())]),
                depth: 2,
                max_nodes: 0,
                max_edges: 0,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(zero_budgets.relations.edges.is_empty());
    assert_eq!(zero_budgets.relations.nodes.len(), 1);
    assert!(zero_budgets.relations.truncated);
    assert!(zero_budgets.relations.continuation.is_none());
    assert!(zero_budgets.history.next_offset.is_none());
    assert!(zero_budgets.truncated);
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

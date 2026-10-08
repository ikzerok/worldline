use super::*;
use crate::project::Project;
use crate::queries::{CatalogQueryFilter, CatalogQueryOptions};
use crate::relations::RelationQueryOptions;
use serde_json::json;

pub(super) fn fixture(name: &str, count: usize) -> Project {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "catalog-scope-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut project = Project::new(&root);
    let root = project.root.clone();
    let source = format!("event start\n  -> END\n{}\nentity context kind place as \"同名\"\nrelation_type linked as \"正式关系\"\nrelation_def edge type linked from entity e000 to entity context\n",
        (0..count).map(|i| format!("entity e{i:03} kind place as \"同名\"\n")).collect::<String>());
    project.set_text(&project.entry.clone(), source).unwrap();
    project.create_authoring_document(&root.join(".world/project.json"), serde_json::to_vec(&json!({
        "schema_version":1,"project_id":"scope","language_version":"1.10","entry":"world.wl",
        "required_features":["content.entities.v1","content.relations.v1"],
        "maps":{"first":".world/maps/first.json","second":".world/maps/second.json"}
    })).unwrap()).unwrap();
    for id in ["first", "second"] {
        let mut map = json!({
            "schema_version":1,"id":id,"title":"同名地图","canvas":{"width":1000,"height":800,"unit":"normalized"},
            "layer_order":["places"],"layers":{"places":{"title":"地点","visible_default":id=="first","locked":id=="second"}},
            "placements":{"one":{"layer_id":"places","target_ref":{"kind":"entity","id":"e000"},"geometry":{"kind":"point","position":[0.2,0.3]},"annotation":"","role":"location"}},"extensions":{}
        });
        if id == "first" {
            map["placements"]["two"] = map["placements"]["one"].clone();
            map["placements"]["broken"] = json!({"layer_id":"places","target_ref":{"kind":"entity","id":"missing"},"geometry":{"kind":"point","position":[0.4,0.5]},"annotation":"","role":"location"});
            let mut scene = crate::vector_scene::MapScene::new(1000.0, 800.0);
            let mut node = crate::vector_scene::SceneNode::new(
                "scene",
                "places",
                crate::vector_scene::SceneGeometry::Point {
                    position: [100.0, 200.0],
                },
            );
            node.target_ref = Some(TargetRef::new("entity", "e000"));
            scene
                .root_order
                .insert("places".into(), vec!["scene".into()]);
            scene.nodes.insert("scene".into(), node);
            map["required_features"] = json!(["presentation.vector_scene.v1"]);
            map["scene"] = serde_json::to_value(scene).unwrap();
        }
        project
            .create_authoring_document(
                &root.join(format!(".world/maps/{id}.json")),
                serde_json::to_vec(&map).unwrap(),
            )
            .unwrap();
    }
    let maps = project.map_index();
    assert_eq!(
        maps.maps.len(),
        2,
        "fixture maps must be valid: {:?}",
        maps.diagnostics
    );
    project
}
pub(super) fn query() -> CatalogQuery {
    CatalogQuery {
        filters: vec![
            CatalogQueryFilter::Kind {
                values: vec!["entity".into()],
                negate: false,
            },
            CatalogQueryFilter::Name {
                values: vec!["e0".into()],
                negate: false,
            },
        ],
        ..Default::default()
    }
}
#[test]
fn snapshot_pages_complete_results_and_preserves_existing_query_cursor_identity() {
    let project = fixture("complete", 155);
    let query = CatalogQuery {
        filters: vec![CatalogQueryFilter::Kind {
            values: vec!["entity".into()],
            negate: false,
        }],
        ..Default::default()
    };
    let baseline = project.content_baseline();
    crate::problems::COMPILE_RUNS.with(|count| count.set(0));
    let scope = project.catalog_scope_snapshot(&query, 10_000).unwrap();
    assert_eq!(crate::problems::COMPILE_RUNS.with(|count| count.get()), 1);
    assert_eq!(scope.query().total(), 156);
    let mut page = scope.query().page(0, 50).unwrap();
    let mut found = page.items.len();
    while let Some(cursor) = page.next {
        page = scope.query().continue_page(&cursor).unwrap();
        found += page.items.len();
    }
    assert_eq!(found, 156);
    assert_eq!(crate::problems::COMPILE_RUNS.with(|count| count.get()), 1);
    let original = project
        .query_catalog(&query, CatalogQueryOptions::default())
        .unwrap();
    assert_eq!(scope.query().page(0, 50).unwrap().next, original.next);
    assert_eq!(project.content_baseline(), baseline);
}
#[test]
fn typed_roles_keep_multiple_bindings_hidden_layers_unresolved_and_formal_context_separate() {
    let project = fixture("roles", 3);
    let source = project.sources();
    let baseline = project.content_baseline();
    let before = crate::fingerprint_program(&project.compile_current().program);
    let scope = project.catalog_scope_snapshot(&query(), 10_000).unwrap();
    assert_eq!(scope.counts().matching_objects, 3);
    assert_eq!(scope.counts().matching_placements, 4);
    assert_eq!(scope.counts().placed_objects, 1);
    assert_eq!(scope.counts().unplaced_objects, 2);
    assert_eq!(scope.counts().unresolved_placements, 1);
    assert_eq!(
        scope
            .placements_for(&TargetRef::new("entity", "e000"))
            .count(),
        4
    );
    assert!(scope
        .placements()
        .iter()
        .any(|p| p.kind == ScopePlacementKind::SceneNode));
    assert!(scope
        .placements()
        .iter()
        .any(|p| p.map_id == "second" && !p.visible && p.locked));
    let related = scope.query_relations(
        &TargetRef::new("entity", "e000"),
        RelationQueryOptions::default(),
    );
    assert_eq!(related.edges.len(), 1);
    assert_eq!(
        scope.role(&TargetRef::new("entity", "context")),
        ScopeRole::ContextOnly
    );
    assert_eq!(
        scope.role(&TargetRef::new("entity", "missing")),
        ScopeRole::Unresolved
    );
    assert_eq!(project.sources(), source);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        crate::fingerprint_program(&project.compile_current().program),
        before
    );
}
#[test]
fn frozen_wire_snapshot_stays_immutable_and_rejects_stale_query_cursor() {
    let mut project = fixture("wire", 3);
    let scope = project.catalog_scope_snapshot(&query(), 10_000).unwrap();
    let bytes = serde_json::to_vec(&scope).unwrap();
    let decoded: CatalogScopeSnapshot = serde_json::from_slice(&bytes).unwrap();
    decoded
        .validate_for(&query(), &project.content_baseline(), 10_000)
        .unwrap();
    let mut cursor = decoded.query().page(0, 1).unwrap().next.unwrap();
    cursor.query_fingerprint = "changed".into();
    assert_eq!(
        decoded.query().continue_page(&cursor).unwrap_err(),
        QueryError::StaleCursor
    );
    project
        .set_text(&project.entry.clone(), "event start\n  -> END\n".into())
        .unwrap();
    assert!(decoded
        .validate_for(&query(), &project.content_baseline(), 10_000)
        .is_err());
    assert_eq!(decoded.query().total(), 3);
    assert_eq!(decoded.placements().len(), 5);
}
#[test]
fn cancelled_and_over_budget_queries_return_no_partial_snapshot() {
    let project = fixture("cancel", 160);
    let mut checks = 0;
    let result = project.catalog_scope_snapshot_cancellable(&query(), 10_000, || {
        checks += 1;
        checks > 2
    });
    assert_eq!(result.unwrap_err(), QueryError::Cancelled);
    assert!(project.catalog_scope_snapshot(&query(), 1).is_err());
    let no_match = CatalogQuery {
        filters: vec![CatalogQueryFilter::Name {
            values: vec!["does-not-exist".into()],
            negate: false,
        }],
        ..Default::default()
    };
    let snapshot = project.catalog_scope_snapshot(&no_match, 10_000).unwrap();
    assert_eq!(snapshot.query().total(), 0);
    assert_eq!(snapshot.counts().matching_placements, 0);
}
#[test]
fn relation_budget_paginates_real_remaining_edges_and_matches_formal_core_direction() {
    let mut project = fixture("relations", 600);
    let mut source = project.document(&project.entry).unwrap().to_owned();
    for i in 1..600 {
        source.push_str(&format!(
            "relation_def r{i:03} type linked from entity e000 to entity e{i:03}\n"
        ));
    }
    project.set_text(&project.entry.clone(), source).unwrap();
    let scope = project.catalog_scope_snapshot(&query(), 10_000).unwrap();
    let focus = TargetRef::new("entity", "e000");
    let mut result = scope.query_relations(&focus, RelationQueryOptions::default());
    let mut ids = std::collections::BTreeSet::new();
    loop {
        for edge in &result.edges {
            assert!(ids.insert(edge.id.clone()));
        }
        let Some(next) = result.continuation else {
            break;
        };
        assert!(next.offset > 0);
        result = scope.continue_relations(&next);
    }
    assert_eq!(ids.len(), 600);
    let options = RelationQueryOptions {
        direction: crate::relations::RelationQueryDirection::Incoming,
        ..Default::default()
    };
    assert!(scope
        .query_relations(&focus, options.clone())
        .edges
        .is_empty());
    let current = project.compile_current();
    assert_eq!(
        serde_json::to_value(scope.query_relations(&focus, options.clone())).unwrap(),
        serde_json::to_value(current.analysis.catalog.query_relations(&focus, options)).unwrap()
    );
}
#[test]
fn unknown_map_capability_never_claims_complete_absence() {
    let mut project = fixture("unknown-map", 3);
    let path = project.root.join(".world/maps/second.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(project.authoring_document(&path).unwrap().bytes()).unwrap();
    value["required_features"] = json!(["future.map.v100"]);
    let future_bytes = serde_json::to_vec(&value).unwrap();
    assert!(project
        .set_authoring_document(&path, future_bytes.clone())
        .is_err());
    let mut files = project
        .sources()
        .into_iter()
        .map(|(path, source)| {
            (
                path.strip_prefix(&project.root).unwrap().to_owned(),
                source.into_bytes(),
            )
        })
        .collect::<crate::workspace_snapshot::Files>();
    for relative in [
        ".world/project.json",
        ".world/maps/first.json",
        ".world/maps/second.json",
    ] {
        files.insert(
            relative.into(),
            project
                .authoring_document(&project.root.join(relative))
                .unwrap()
                .bytes()
                .to_vec(),
        );
    }
    files.insert(".world/maps/second.json".into(), future_bytes.clone());
    let imported =
        Project::from_snapshot(&project.root, std::path::Path::new("world.wl"), &files).unwrap();
    assert!(imported.authoring_document(&path).unwrap().is_read_only());
    let baseline = imported.content_baseline();
    let scope = imported.catalog_scope_snapshot(&query(), 10_000).unwrap();
    assert!(scope.maps_incomplete());
    assert!(!scope.diagnostics().is_empty());
    assert_eq!(
        imported.authoring_document(&path).unwrap().bytes(),
        future_bytes
    );
    assert_eq!(imported.content_baseline(), baseline);
}

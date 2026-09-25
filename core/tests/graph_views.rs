//! WP-10：共享网络布局只通过 core 展示事务写入。
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::graph_views::{self, GraphViewCommand, GraphViewDraft};
use worldline_core::presentation_commands::Revision;
use worldline_core::project::Project;

fn project() -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-graph-views-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut p = Project::new(&root);
    let entry = p.entry.clone();
    p.documents.retain(|path, _| path == &entry);
    p.set_text(&p.entry.clone(), "entity a kind place as \"甲地\"\nentity b kind organization as \"乙会\"\nrelation_type knows as \"知道\"\nrelation_def r type knows from entity b to entity a\n".into()).unwrap();
    p.create_authoring_document(
        &root.join(".world/project.json"),
        br#"{
        "schema_version":1,"language_version":"1.10",
        "required_features":["content.entities.v1","content.relations.v1"],
        "maps":{},"graph_views":{},"future":{"keep":true}
    }"#
        .to_vec(),
    )
    .unwrap();
    assert!(!p.compile().has_errors(), "{:?}", p.compile().diagnostics);
    p
}

fn draft() -> GraphViewDraft {
    GraphViewDraft {
        id: "view_a".into(),
        title: "甲地关联".into(),
        focus: TargetRef::new("entity", "a"),
        positions: BTreeMap::from([
            ("entity:a".into(), [0.0, 0.0]),
            ("entity:b".into(), [160.0, 80.0]),
        ]),
        hidden_relation_ids: vec!["r".into()],
        ..Default::default()
    }
}

fn command(
    p: &Project,
    revision: Revision,
    original: Option<&str>,
    draft: GraphViewDraft,
) -> GraphViewCommand {
    GraphViewCommand {
        expected_revision: revision,
        expected_baseline: p.content_baseline(),
        original: original.map(str::to_owned),
        draft,
    }
}

#[test]
fn creation_is_one_presentation_transaction_and_reading_is_pure() {
    let mut p = project();
    let before = p.clone();
    let sources = p.sources();
    let fingerprint = p.compile().analysis.fingerprint;
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    let content = p.compile();
    graph_views::apply_with_content(&mut p, &mut revision, request, &content).unwrap();
    assert_eq!(revision.presentation_generation, 1);
    assert_eq!(revision.content_generation, 0);
    assert_eq!(p.sources(), sources);
    assert_eq!(p.compile().analysis.fingerprint, fingerprint);
    let baseline = p.content_baseline();
    for _ in 0..100 {
        let index = graph_views::build_graph_view_index(&p, &content);
        assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
        assert_eq!(
            index.views["view_a"].draft.focus,
            TargetRef::new("entity", "a")
        );
    }
    assert_eq!(p.content_baseline(), baseline);
    assert!(p.restore(before));
    assert!(graph_views::build_graph_view_index(&p, &content)
        .views
        .is_empty());
    assert_eq!(p.sources(), sources);
}

#[test]
fn old_revision_and_content_baseline_are_zero_write_failures() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    revision = revision.next_presentation();
    let before = p.content_baseline();
    assert!(graph_views::apply(&mut p, &mut revision, request.clone()).is_err());
    assert_eq!(p.content_baseline(), before);
    revision = Revision::default();
    let path = p.entry.clone();
    p.set_text(
        &path,
        format!("{}\n# 外部新资料\n", p.document(&path).unwrap()),
    )
    .unwrap();
    let before = p.content_baseline();
    assert!(graph_views::apply(&mut p, &mut revision, request).is_err());
    assert_eq!(p.content_baseline(), before);
    assert_eq!(revision, Revision::default());
}

#[test]
fn optional_unknown_fields_survive_layout_edits() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let path = p.root.join(".world/graph-views/view_a.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&path).unwrap().bytes()).unwrap();
    value["future"] = serde_json::json!({"nested":[1,2,3]});
    value["focus"]["future"] = serde_json::json!("保留");
    value["filters"]["future"] = serde_json::json!(true);
    p.set_authoring_document(&path, serde_json::to_vec(&value).unwrap())
        .unwrap();
    let mut changed = draft();
    changed.positions.insert("entity:a".into(), [50.0, -12.0]);
    let request = command(&p, revision, Some("view_a"), changed);
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let actual: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&path).unwrap().bytes()).unwrap();
    assert_eq!(actual["future"], value["future"]);
    assert_eq!(actual["focus"]["future"], value["focus"]["future"]);
    assert_eq!(actual["filters"]["future"], value["filters"]["future"]);
    assert_eq!(
        actual["positions"]["entity:a"],
        serde_json::json!([50.0, -12.0])
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        p.authoring_document(&p.root.join(".world/project.json"))
            .unwrap()
            .bytes(),
    )
    .unwrap();
    assert_eq!(manifest["future"]["keep"], true);
}

#[test]
fn graph_references_block_entity_and_relation_deletion() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let before = p.content_baseline();
    let impact = p.deletion_impact(&TargetRef::new("relation", "r"));
    assert!(!impact.can_delete());
    assert_eq!(impact.graph_views.len(), 1);
    assert!(p.remove_relation("r").is_err());
    assert!(p.edit(|p| p.remove_entity("a")).is_err());
    assert_eq!(p.content_baseline(), before);
}

#[test]
fn invalid_layouts_and_unregistered_paths_never_partially_register() {
    for invalid in 0..6 {
        let mut p = project();
        let mut view = draft();
        match invalid {
            0 => view.id = "../escape".into(),
            1 => view
                .positions
                .insert("entity:a".into(), [f64::NAN, 0.0])
                .map(|_| ())
                .unwrap_or(()),
            2 => view.focus = TargetRef::new("entity", "missing"),
            3 => view.filters.depth = 3,
            4 => view.hidden_relation_ids.push("missing".into()),
            _ => view
                .positions
                .insert("entity:missing".into(), [0.0, 0.0])
                .map(|_| ())
                .unwrap_or(()),
        }
        let mut revision = Revision::default();
        let request = command(&p, revision, None, view);
        let before = p.content_baseline();
        assert!(graph_views::apply(&mut p, &mut revision, request).is_err());
        assert_eq!(p.content_baseline(), before);
        assert_eq!(revision, Revision::default());
    }
}

#[test]
fn broken_view_keeps_raw_bytes_and_makes_reference_analysis_incomplete() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let path = p.root.join(".world/graph-views/view_a.json");
    let broken = b"{broken JSON".to_vec();
    p.set_authoring_document(&path, broken.clone()).unwrap();
    let content = p.compile();
    assert!(!content.has_errors());
    let index = graph_views::build_graph_view_index(&p, &content);
    assert!(!index.diagnostics.is_empty());
    assert!(!p.deletion_impact(&TargetRef::new("relation", "r")).complete);
    assert_eq!(p.authoring_document(&path).unwrap().bytes(), broken);
}

#[test]
fn deleting_a_shared_view_is_reversible_and_never_deletes_content() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let before = p.clone();
    let content = p.compile();
    let request = graph_views::DeleteGraphViewCommand {
        id: "view_a".into(),
        expected_revision: revision,
        expected_baseline: p.content_baseline(),
    };
    graph_views::remove(&mut p, &mut revision, request.clone()).unwrap();
    assert_eq!(revision.presentation_generation, 2);
    assert!(graph_views::build_graph_view_index(&p, &content)
        .views
        .is_empty());
    assert_eq!(p.sources(), content.sources);
    assert!(p.compile().analysis.catalog.relations.contains_key("r"));
    let after = p.content_baseline();
    assert!(graph_views::remove(&mut p, &mut revision, request).is_err());
    assert_eq!(p.content_baseline(), after);
    assert!(p.restore(before));
    assert_eq!(
        graph_views::build_graph_view_index(&p, &content)
            .views
            .len(),
        1
    );
}

#[test]
fn multi_type_filter_and_continuation_preserve_explicit_edges() {
    let mut p = project();
    let path = p.entry.clone();
    let source = format!("{}\nrelation_type owns as \"拥有\"\nrelation_type sees as \"看见\"\nrelation_def r2 type owns from entity b to entity a\nrelation_def r3 type sees from entity b to entity a\n", p.document(&path).unwrap());
    p.set_text(&path, source).unwrap();
    let content = p.compile();
    assert!(!content.has_errors());
    let catalog = &content.analysis.catalog;
    let filters = graph_views::GraphViewFilters {
        relation_types: vec!["knows".into(), "owns".into()],
        max_edges: 1,
        ..Default::default()
    };
    let first = catalog.query_relations(&TargetRef::new("entity", "a"), filters.query_options(0));
    assert!(first.truncated);
    assert_eq!(first.edges.len(), 1);
    let second = catalog.continue_relations(first.continuation.as_ref().unwrap());
    assert_eq!(second.edges.len(), 1);
    assert!(!second.truncated);
    let ids = [first.edges[0].id.as_str(), second.edges[0].id.as_str()];
    assert!(ids.contains(&"r") && ids.contains(&"r2"));
    assert!(!ids.contains(&"r3"));
    assert_eq!(catalog.relations.len(), 3);
}

#[test]
fn stale_compilation_is_rejected_even_with_current_command_baseline() {
    let mut p = project();
    let content = p.compile();
    p.set_text(&p.entry.clone(), "entity other kind place\n".into())
        .unwrap();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    let before = p.content_baseline();
    assert!(graph_views::apply_with_content(&mut p, &mut revision, request, &content).is_err());
    assert_eq!(p.content_baseline(), before);
}

#[test]
fn a_saved_filter_protects_an_unused_relation_type_from_deletion() {
    let mut p = project();
    let path = p.entry.clone();
    p.set_text(
        &path,
        format!(
            "{}\nrelation_type optional as \"可选类型\"\n",
            p.document(&path).unwrap()
        ),
    )
    .unwrap();
    let mut view = draft();
    view.filters.relation_types = vec!["optional".into()];
    let mut revision = Revision::default();
    let request = command(&p, revision, None, view);
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let before = p.content_baseline();
    assert!(p.remove_relation_type("optional").is_err());
    assert_eq!(p.content_baseline(), before);
}

#[test]
fn a_graph_path_shared_with_another_registration_cannot_be_edited_or_deleted() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let manifest = p.root.join(".world/project.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(p.authoring_document(&manifest).unwrap().bytes()).unwrap();
    value["maps"]["alias"] = serde_json::json!(".world/graph-views/view_a.json");
    p.set_authoring_document(&manifest, serde_json::to_vec(&value).unwrap())
        .unwrap();
    let before = p.content_baseline();
    let request = command(&p, revision, Some("view_a"), draft());
    assert!(graph_views::apply(&mut p, &mut revision, request).is_err());
    let remove = graph_views::DeleteGraphViewCommand {
        id: "view_a".into(),
        expected_revision: revision,
        expected_baseline: before.clone(),
    };
    assert!(graph_views::remove(&mut p, &mut revision, remove).is_err());
    assert_eq!(p.content_baseline(), before);
}

#[test]
fn missing_targets_in_broken_content_are_unresolved_not_declared_deleted() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    p.set_text(&p.entry.clone(), "not valid worldline syntax\n".into())
        .unwrap();
    let content = p.compile();
    assert!(content.has_errors());
    let index = graph_views::build_graph_view_index(&p, &content);
    assert!(index.diagnostics.iter().any(|item| item.code == "GRAPH004"));
    assert!(!index.diagnostics.iter().any(|item| item.code == "GRAPH002"));
    assert!(!p.deletion_impact(&TargetRef::new("entity", "a")).complete);
    let mut updated = draft();
    updated.positions.insert("entity:a".into(), [80.0, 40.0]);
    let before = p.sources();
    let request = command(&p, revision, Some("view_a"), updated);
    graph_views::apply_with_content(&mut p, &mut revision, request, &content).unwrap();
    assert_eq!(p.sources(), before);
}

#[test]
fn duplicate_json_keys_are_visible_and_preserved() {
    let mut p = project();
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let path = p.root.join(".world/graph-views/view_a.json");
    let original = p.authoring_document(&path).unwrap().bytes();
    let text =
        String::from_utf8(original.to_vec())
            .unwrap()
            .replacen("{", "{\"title\":\"duplicate\",", 1);
    p.set_authoring_document(&path, text.as_bytes().to_vec())
        .unwrap();
    let baseline = p.content_baseline();
    let content = p.compile();
    let index = graph_views::build_graph_view_index(&p, &content);
    assert!(index.diagnostics.iter().any(|item| item.code == "GRAPH001"));
    assert!(index.views.is_empty());
    let request = command(&p, revision, Some("view_a"), draft());
    assert!(graph_views::apply(&mut p, &mut revision, request).is_err());
    assert_eq!(p.content_baseline(), baseline);
    assert_eq!(
        p.authoring_document(&path).unwrap().bytes(),
        text.as_bytes()
    );
}

#[test]
fn graph_document_lifecycle_saves_and_reopens_without_recompiling_layout_into_story() {
    let mut p = project();
    let root = p.root.clone();
    assert!(!root.exists());
    let mut revision = Revision::default();
    let request = command(&p, revision, None, draft());
    graph_views::apply(&mut p, &mut revision, request).unwrap();
    let fingerprint = p.compile().analysis.fingerprint;
    p.save().unwrap();
    assert!(!p.is_dirty());
    let mut reopened = Project::open(&p.entry).unwrap();
    let content = reopened.compile();
    let index = graph_views::build_graph_view_index(&reopened, &content);
    assert!(index.diagnostics.is_empty());
    assert_eq!(index.views["view_a"].draft, draft());
    assert_eq!(content.analysis.fingerprint, fingerprint);
    let path = reopened.root.join(".world/graph-views/view_a.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(reopened.authoring_document(&path).unwrap().bytes()).unwrap();
    value["required_features"] = serde_json::json!(["future.graph.v9"]);
    let unknown = serde_json::to_vec(&value).unwrap();
    std::fs::write(&path, &unknown).unwrap();
    reopened.refresh().unwrap();
    let baseline = reopened.content_baseline();
    let content = reopened.compile();
    let index = graph_views::build_graph_view_index(&reopened, &content);
    assert!(index.views["view_a"].read_only);
    assert!(index.diagnostics.iter().any(|item| item.code == "GRAPH003"));
    let request = command(&reopened, revision, Some("view_a"), draft());
    assert!(graph_views::apply(&mut reopened, &mut revision, request).is_err());
    let remove = graph_views::DeleteGraphViewCommand {
        id: "view_a".into(),
        expected_revision: revision,
        expected_baseline: baseline.clone(),
    };
    assert!(graph_views::remove(&mut reopened, &mut revision, remove).is_err());
    assert_eq!(reopened.content_baseline(), baseline);
    assert_eq!(std::fs::read(&path).unwrap(), unknown);
    std::fs::remove_dir_all(root).unwrap();
}

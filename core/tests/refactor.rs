//! WP-11：稳定 ID 跨源码与展示文档整批重命名。
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::graph_views;
use worldline_core::presentation_commands::map_index_with_content;
use worldline_core::project::Project;

fn project() -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-refactor-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project
        .set_text(
            &entry,
            r#"tag t as "标签"
anchor_def clue as "线索"
entity a kind place as "甲地"
entity b kind organization as "乙会"
relation_type knows as "知道"
relation_def r type knows from entity a to entity b
mark entity a with t
mark relation r with t
alias entity a as "旧称"
anchor_link clue entity a
event start
  [[entity:a|甲地]]与[[relation:r|知道关系]]
  -> END
"#
            .into(),
        )
        .unwrap();
    project.create_authoring_document(
        &root.join(".world/project.json"),
        br#"{
          "schema_version":1,"language_version":"1.10",
          "required_features":["content.entities.v1","content.relations.v1","presentation.graph_views.v1"],
          "maps":{"m":".world/maps/m.json"},
          "graph_views":{"v":".world/graph-views/v.json"}
        }"#.to_vec(),
    ).unwrap();
    project
        .create_authoring_document(
            &root.join(".world/maps/m.json"),
            r#"{
          "schema_version":1,"id":"m","title":"地图",
          "canvas":{"width":100,"height":100,"unit":"normalized"},
          "layer_order":["objects"],
          "layers":{"objects":{"title":"对象","visible_default":true,"locked":false}},
          "placements":{
            "a":{"layer_id":"objects","target_ref":{"kind":"entity","id":"a"},
                 "scope_refs":[{"kind":"relation","id":"r"}],
                 "geometry":{"kind":"point","position":[0.2,0.3]},"annotation":"","role":"入口"}
          }
        }"#
            .as_bytes()
            .to_vec(),
        )
        .unwrap();
    project
        .create_authoring_document(
            &root.join(".world/graph-views/v.json"),
            r#"{
          "schema_version":1,"id":"v","title":"网络",
          "focus":{"kind":"entity","id":"a"},
          "filters":{"depth":1,"relation_types":["knows"]},
          "positions":{"entity:a":[0,0],"entity:b":[120,0],"relation:r":[60,50]},
          "hidden_relation_ids":["r"]
        }"#
            .as_bytes()
            .to_vec(),
        )
        .unwrap();
    assert!(
        !project.compile().has_errors(),
        "{:?}",
        project.compile().diagnostics
    );
    project
}
#[test]
fn entity_id_rename_updates_sources_maps_and_graph_views_atomically() {
    let mut project = project();
    let fingerprint = project.compile().analysis.fingerprint;
    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "a"), "alpha")
        .unwrap();
    assert!(plan.explicit_references >= 6);
    assert!(plan.changes.iter().any(|change| change.kind == "source"));
    assert!(plan.changes.iter().any(|change| change.kind == "authoring"));

    project.apply_rename_plan(&plan).unwrap();
    let compiled = project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(compiled.analysis.fingerprint, fingerprint);
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("entity", "a"))
        .is_none());
    assert!(compiled
        .analysis
        .catalog
        .object(&TargetRef::new("entity", "alpha"))
        .is_some());
    let source = project.document(&project.entry).unwrap();
    assert!(source.contains("entity alpha kind place"));
    assert!(source.contains("from entity alpha to entity b"));
    assert!(source.contains("mark entity alpha with t"));
    assert!(source.contains("alias entity alpha as"));
    assert!(source.contains("anchor_link clue entity alpha"));
    assert!(source.contains("[[entity:alpha|甲地]]"));
    let maps = map_index_with_content(&project, &compiled);
    assert!(maps.diagnostics.is_empty(), "{:?}", maps.diagnostics);
    assert_eq!(
        maps.maps["m"].placements["a"].target_ref,
        Some(TargetRef::new("entity", "alpha"))
    );
    let views = graph_views::build_graph_view_index(&project, &compiled);
    assert!(views.diagnostics.is_empty(), "{:?}", views.diagnostics);
    assert_eq!(
        views.views["v"].draft.focus,
        TargetRef::new("entity", "alpha")
    );
    assert!(views.views["v"]
        .draft
        .positions
        .contains_key("entity:alpha"));
    assert!(!views.views["v"].draft.positions.contains_key("entity:a"));
}

#[test]
fn relation_id_rename_updates_content_and_shared_layout_references() {
    let mut project = project();
    let fingerprint = project.compile().analysis.fingerprint;
    let plan = project
        .plan_rename_target(&TargetRef::new("relation", "r"), "r_new")
        .unwrap();
    project.apply_rename_plan(&plan).unwrap();

    let compiled = project.compile();
    assert_eq!(compiled.analysis.fingerprint, fingerprint);
    assert!(compiled.analysis.catalog.relations.contains_key("r_new"));
    assert!(!compiled.analysis.catalog.relations.contains_key("r"));
    let source = project.document(&project.entry).unwrap();
    assert!(source.contains("relation_def r_new type knows"));
    assert!(source.contains("mark relation r_new with t"));
    assert!(source.contains("[[relation:r_new|知道关系]]"));
    let maps = map_index_with_content(&project, &compiled);
    assert_eq!(
        maps.maps["m"].placements["a"].scope_refs,
        vec![TargetRef::new("relation", "r_new")]
    );
    let views = graph_views::build_graph_view_index(&project, &compiled);
    let draft = &views.views["v"].draft;
    assert_eq!(draft.hidden_relation_ids, vec!["r_new"]);
    assert!(draft.positions.contains_key("relation:r_new"));
}

#[test]
fn stale_or_colliding_rename_plan_is_zero_write() {
    let mut project = project();
    assert!(project
        .plan_rename_target(&TargetRef::new("entity", "a"), "b")
        .is_err());

    let plan = project
        .plan_rename_target(&TargetRef::new("entity", "a"), "alpha")
        .unwrap();
    let entry = project.entry.clone();
    project
        .set_text(
            &entry,
            format!("{}\n# 外部修改\n", project.document(&entry).unwrap()),
        )
        .unwrap();
    let before = project.content_baseline();
    assert!(project.apply_rename_plan(&plan).is_err());
    assert_eq!(project.content_baseline(), before);
    assert!(project
        .compile()
        .analysis
        .catalog
        .entities
        .contains_key("a"));
    assert!(!project
        .compile()
        .analysis
        .catalog
        .entities
        .contains_key("alpha"));
}
#[test]
fn broken_registered_view_blocks_rename_instead_of_hiding_references() {
    let mut project = project();
    let path = project.root.join(".world/graph-views/v.json");
    project
        .set_authoring_document(&path, br#"{"schema_version":1,"id":"v","focus":"#.to_vec())
        .unwrap();
    let before = project.content_baseline();
    let error = project
        .plan_rename_target(&TargetRef::new("entity", "a"), "alpha")
        .unwrap_err();
    assert!(error.contains("引用检查不完整") || error.contains("JSON"));
    assert_eq!(project.content_baseline(), before);
}

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::project::Project;
use worldline_core::{RelationQueryDirection, RelationQueryOptions, TargetRef};

fn root(name: &str) -> std::path::PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "worldline-source-scope-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn write_project(root: &std::path::Path, manifest: Option<&str>) {
    let _ = fs::remove_dir_all(root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::create_dir_all(root.join("drafts")).unwrap();
    fs::write(root.join("world.wl"), "character live as \"活动\"\n").unwrap();
    fs::write(
        root.join("drafts/unused.wl"),
        "character hidden as \"归档\"\n",
    )
    .unwrap();
    if let Some(manifest) = manifest {
        fs::write(root.join(".world/project.json"), manifest).unwrap();
    }
}

#[test]
fn legacy_project_without_source_config_still_compiles_every_wl_file() {
    let root = root("legacy");
    write_project(&root, None);
    let mut project = Project::open(&root).unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.analysis.symbols.characters.contains_key("live"));
    assert!(result.analysis.symbols.characters.contains_key("hidden"));
    assert!(project.source_selection().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_source_config_excludes_archived_sources_and_blocks_includes() {
    let root = root("explicit");
    write_project(
        &root,
        Some(
            r#"{"schema_version":1,"language_version":"1.9","entry":"world.wl","required_features":["workspace.source_sets.v1"],"maps":{},"graph_views":{},"source_config":{"mode":"explicit","active":["world.wl"],"archived":["drafts/unused.wl"]}}"#,
        ),
    );
    let mut project = Project::open(&root).unwrap();
    let result = project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.analysis.symbols.characters.contains_key("live"));
    assert!(!result.analysis.symbols.characters.contains_key("hidden"));
    let selection = project.source_selection().unwrap();
    assert_eq!(selection.active.len(), 1);
    assert_eq!(selection.archived.len(), 1);

    let entry = project.entry.clone();
    project
        .set_text(
            &entry,
            "include \"drafts/unused.wl\"\ncharacter live as \"活动\"\n".into(),
        )
        .unwrap();
    let result = project.compile();
    assert!(result
        .diagnostics
        .iter()
        .any(|item| { item.code == "A105" && item.message.contains("非活动源码") }));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_source_config_is_a_workspace_error_and_never_silently_falls_back() {
    let root = root("invalid");
    write_project(
        &root,
        Some(
            r#"{"schema_version":1,"language_version":"1.9","entry":"world.wl","required_features":["workspace.source_sets.v1"],"maps":{},"graph_views":{},"source_config":{"mode":"explicit","active":["../escape.wl"],"archived":[]}}"#,
        ),
    );
    let project = Project::open(&root).unwrap();
    assert!(project
        .authoring_diagnostics()
        .iter()
        .any(|item| item.code == "WS005"));
    assert!(project.source_selection().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn relation_scope_filter_is_or_within_dimension_and_and_across_dimensions() {
    let source = r#"
period old as "旧纪元"
period late as "旧纪元末" within old
entity version_a kind version as "版本A"
entity version_b kind version as "版本B"
entity a kind place
entity b kind place
relation_type links as "连接"
relation_def old_a type links from entity a to entity b
  scope period old
  scope entity version_a
relation_def late_a type links from entity a to entity b
  scope period late
  scope entity version_a
relation_def old_b type links from entity a to entity b
  scope period old
  scope entity version_b
relation_def global type links from entity a to entity b
"#;
    let result = worldline_core::compile_source_with_options(
        "world.wl",
        source,
        worldline_core::CompileOptions::v1_10(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let catalog = &result.analysis.catalog;
    let target = TargetRef::new("entity", "a");
    let options = RelationQueryOptions {
        scope_refs: vec![
            TargetRef::new("period", "old"),
            TargetRef::new("entity", "version_a"),
        ],
        direction: RelationQueryDirection::Both,
        ..Default::default()
    };
    let filtered = catalog.query_relations(&target, options.clone());
    assert_eq!(
        filtered
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        vec!["old_a"]
    );
    let expanded = worldline_core::relations::expand_period_scope_refs(
        &result.analysis.timeline,
        &options.scope_refs,
        true,
    );
    let with_children = catalog.query_relations(
        &target,
        RelationQueryOptions {
            scope_refs: expanded,
            ..options
        },
    );
    assert_eq!(
        with_children
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        vec!["late_a", "old_a"]
    );
}

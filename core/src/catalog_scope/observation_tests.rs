use super::tests::{fixture, query};
use super::*;
use crate::project::Project;

#[test]
fn immutable_queries_never_read_a_new_untracked_include_from_disk() {
    let mut project = fixture("untracked-include", 3);
    std::fs::create_dir_all(&project.root).unwrap();
    let disk_only = project.root.join("disk_only.wl");
    std::fs::write(&disk_only, "entity disk_secret kind place\n").unwrap();
    let source = format!(
        "include \"disk_only.wl\"\n{}",
        project.document(&project.entry).unwrap()
    );
    project.set_text(&project.entry.clone(), source).unwrap();
    assert!(!project.documents.contains_key(&disk_only));
    let baseline = project.content_baseline();
    let scope = project
        .catalog_scope_snapshot(&CatalogQuery::default(), 10_000)
        .unwrap();
    assert!(scope.query().incomplete);
    assert!(scope
        .query()
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "A105"));
    assert!(!scope
        .query()
        .matches()
        .iter()
        .any(|item| item.target.id == "disk_secret"));
    let query_snapshot = project
        .catalog_query_snapshot(&CatalogQuery::default(), 10_000)
        .unwrap();
    assert!(query_snapshot.incomplete);
    assert!(!query_snapshot
        .matches()
        .iter()
        .any(|item| item.target.id == "disk_secret"));
    let frozen = serde_json::to_vec(&scope.query().page(0, 50).unwrap()).unwrap();
    std::fs::remove_dir_all(&project.root).unwrap();
    assert_eq!(
        serde_json::to_vec(&scope.query().page(0, 50).unwrap()).unwrap(),
        frozen
    );
    assert_eq!(project.content_baseline(), baseline);
    assert!(!project.documents.contains_key(&disk_only));
}

#[test]
fn asset_only_refresh_changes_memory_observation_without_changing_author_content() {
    let mut project = on_disk();
    let baseline = project.content_baseline();
    let observed = project.catalog_scope_observation_key();
    let scope = project.catalog_scope_snapshot(&query(), 10_000).unwrap();
    let frozen = serde_json::to_vec(&scope).unwrap();
    let candidate = project.clone();
    std::fs::remove_file(project.root.join("art.png")).unwrap();
    assert_eq!(
        project.catalog_scope_observation_key(),
        observed,
        "getter must not probe the disk"
    );
    assert!(project.refresh().unwrap().is_empty());
    assert_eq!(project.content_baseline(), baseline);
    let after = project.catalog_scope_observation_key();
    assert_ne!(after, observed);
    assert_eq!(
        candidate.catalog_scope_observation_key(),
        after,
        "old candidate cannot roll observation back"
    );
    assert_eq!(serde_json::to_vec(&scope).unwrap(), frozen);
    project.refresh().unwrap();
    assert_eq!(
        project.catalog_scope_observation_key(),
        after,
        "no-op refresh must retain cache identity"
    );
    std::fs::remove_dir_all(&project.root).unwrap();
    assert_eq!(
        project.catalog_scope_observation_key(),
        after,
        "no IO even after root removal"
    );
    assert!(project.refresh().is_err());
    assert_ne!(
        project.catalog_scope_observation_key(),
        after,
        "failed observation invalidates prior trust"
    );
}

fn on_disk() -> Project {
    let mut project = fixture("asset-observation", 3);
    let source = format!(
        "asset art image \"art.png\"\n{}",
        project.document(&project.entry).unwrap()
    );
    project.set_text(&project.entry.clone(), source).unwrap();
    for (path, document) in &project.documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &document.text).unwrap();
    }
    for (path, document) in &project.authoring_documents {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, document.bytes()).unwrap();
    }
    std::fs::write(project.root.join("art.png"), b"scope asset bytes").unwrap();
    project.mark_saved();
    project.refresh().unwrap();
    project
}

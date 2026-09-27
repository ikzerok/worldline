use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::queries::{
    CatalogQuery, CatalogQueryFilter, CatalogQueryOptions, MissingCondition, PropertyScalar,
    QueryError, SavedQueryDraft, TodoKind,
};

#[path = "catalog_queries/filters.rs"]
mod filters;
#[path = "catalog_queries/saved.rs"]
mod saved;
#[path = "catalog_queries/todos.rs"]
mod todos;

fn project(name: &str, source: &str) -> Project {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-catalog-query-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world")).unwrap();
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"query_test","language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","content.relations.v1"],"maps":{},"graph_views":{}}"#,
    )
    .unwrap();
    Project::open(&root).unwrap()
}

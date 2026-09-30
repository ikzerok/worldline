//! 资料库组合查询、共享已保存查询与待办只读投影。

use std::path::Path;

pub const CATALOG_QUERY_SCHEMA_VERSION: u32 = 1;
pub const SORTED_CATALOG_QUERY_SCHEMA_VERSION: u32 = 2;
pub const CATALOG_QUERY_SORT_REQUIRED_FEATURE: &str = "catalog.query_sort.v1";
pub const MAX_CATALOG_QUERY_PAGE_SIZE: usize = 100;
pub const MAX_CATALOG_QUERY_CANDIDATES: usize = 100_000;
pub const MAX_CATALOG_QUERY_VALUES: usize = 100;
pub const DEFAULT_CATALOG_QUERY_CANDIDATES: usize = 10_000;
pub const SAVED_QUERY_SCHEMA_VERSION: u64 = 1;
pub const SAVED_QUERY_REQUIRED_FEATURE: &str = "catalog.saved_queries.v1";

mod filters;
mod pagination;
mod saved;
mod sorting;
mod todos;

pub use filters::{
    CatalogQuery, CatalogQueryFilter, MissingCondition, PropertyCondition, PropertyScalar,
    QueryError, RelationCondition, RelationDirection,
};
pub use pagination::{
    CatalogQueryCursor, CatalogQueryMatch, CatalogQueryOptions, CatalogQueryPage, QuerySource,
};
pub use saved::{SavedQueryDocument, SavedQueryDraft, SavedQueryIndex};
pub use sorting::{CatalogQuerySort, CatalogSortDirection, CatalogSortField};
pub use todos::{TodoItem, TodoKind, TodoProjection};

pub(super) fn source_relative(root: &Path, source: &str) -> String {
    let source = crate::compiler::source_path(Path::new(source));
    source
        .strip_prefix(crate::compiler::source_path(root))
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| source.to_string_lossy().replace('\\', "/"))
}

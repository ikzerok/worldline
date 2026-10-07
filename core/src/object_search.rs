//! 已编译目录的只读完整身份候选页，以及不刷新磁盘的 Project 快照入口。
use crate::catalog::{Catalog, CatalogObject};
use serde::{Deserialize, Serialize};
mod filter;
pub use filter::ObjectSearchFilter;

#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod tests;

pub const MAX_OBJECT_SEARCH_LIMIT: usize = 100;
pub const DEFAULT_OBJECT_SEARCH_CANDIDATES: usize =
    crate::queries::DEFAULT_CATALOG_QUERY_CANDIDATES;
pub const MAX_OBJECT_SEARCH_CANDIDATES: usize = crate::queries::MAX_CATALOG_QUERY_CANDIDATES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObjectSearchOptions {
    pub offset: usize,
    pub limit: usize,
    pub max_candidates: usize,
}

impl Default for ObjectSearchOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 20,
            max_candidates: DEFAULT_OBJECT_SEARCH_CANDIDATES,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectSearchPage {
    pub items: Vec<CatalogObject>,
    /// 所给目录的精确匹配数；不证明该目录的源码全部可解析。
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    /// 本页未包含全部匹配项；也包括前页已显示的项。
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectSearchError {
    InvalidLimit { limit: usize },
    InvalidCandidateBudget { budget: usize },
    CandidateBudgetExceeded { candidates: usize, budget: usize },
    InvalidOffset { offset: usize, total: usize },
}

impl std::fmt::Display for ObjectSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLimit { limit } => write!(
                f,
                "候选页数量 {limit} 无效，必须在 1–{MAX_OBJECT_SEARCH_LIMIT} 之间"
            ),
            Self::InvalidCandidateBudget { budget } => write!(
                f,
                "候选预算 {budget} 无效，必须在 1–{MAX_OBJECT_SEARCH_CANDIDATES} 之间"
            ),
            Self::CandidateBudgetExceeded { candidates, budget } => write!(
                f,
                "目录包含 {candidates} 个对象，超过候选预算 {budget}；未返回部分结果"
            ),
            Self::InvalidOffset { offset, total } => {
                write!(f, "候选起点 {offset} 超过匹配总数 {total}")
            }
        }
    }
}
impl std::error::Error for ObjectSearchError {}

impl Catalog {
    /// 对名称、ID、kind 和别名进行 Unicode 小写子串匹配，返回稳定完整身份页。
    pub fn search_objects_page(
        &self,
        query: &str,
        options: ObjectSearchOptions,
    ) -> Result<ObjectSearchPage, ObjectSearchError> {
        self.search_objects_filtered_page(query, &ObjectSearchFilter::default(), options)
    }

    /// 显式过滤已编译目录；预算始终以完整目录为准，来源路径仅在开启时参与文字匹配。
    pub fn search_objects_filtered_page(
        &self,
        query: &str,
        filter: &ObjectSearchFilter,
        options: ObjectSearchOptions,
    ) -> Result<ObjectSearchPage, ObjectSearchError> {
        self.validate_object_search_options(options)?;
        let mut matches = filter.matches(self, query);
        matches.sort_by(|left, right| left.target.cmp(&right.target));
        let total = matches.len();
        if options.offset > total {
            return Err(ObjectSearchError::InvalidOffset {
                offset: options.offset,
                total,
            });
        }
        let end = options.offset.saturating_add(options.limit).min(total);
        let items = matches[options.offset..end]
            .iter()
            .map(|object| (*object).clone())
            .collect::<Vec<_>>();
        Ok(ObjectSearchPage {
            truncated: items.len() < total,
            items,
            total,
            offset: options.offset,
            limit: options.limit,
            next_offset: (end < total).then_some(end),
        })
    }

    fn validate_object_search_options(
        &self,
        options: ObjectSearchOptions,
    ) -> Result<(), ObjectSearchError> {
        if !(1..=MAX_OBJECT_SEARCH_LIMIT).contains(&options.limit) {
            return Err(ObjectSearchError::InvalidLimit {
                limit: options.limit,
            });
        }
        if !(1..=MAX_OBJECT_SEARCH_CANDIDATES).contains(&options.max_candidates) {
            return Err(ObjectSearchError::InvalidCandidateBudget {
                budget: options.max_candidates,
            });
        }
        if self.objects.len() > options.max_candidates {
            return Err(ObjectSearchError::CandidateBudgetExceeded {
                candidates: self.objects.len(),
                budget: options.max_candidates,
            });
        }
        Ok(())
    }
}

impl crate::project::Project {
    /// 已加载当前缓冲的只读目录快照；不刷新、不写入、不补载磁盘来源。
    pub fn compile_object_search_snapshot(&self) -> crate::CompileResult {
        self.compile_problems_snapshot()
    }
}

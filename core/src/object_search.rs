//! 已编译目录的只读、完整身份候选页；不编译 Project 或解释查询语言。
use crate::catalog::{Catalog, CatalogObject};
use serde::Serialize;
use std::collections::BTreeSet;

pub const MAX_OBJECT_SEARCH_LIMIT: usize = 100;
pub const DEFAULT_OBJECT_SEARCH_CANDIDATES: usize =
    crate::queries::DEFAULT_CATALOG_QUERY_CANDIDATES;
pub const MAX_OBJECT_SEARCH_CANDIDATES: usize = crate::queries::MAX_CATALOG_QUERY_CANDIDATES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        let needle = query.trim().to_lowercase();
        let alias_targets: BTreeSet<_> = self
            .aliases
            .iter()
            .filter(|alias| alias.name.to_lowercase().contains(&needle))
            .map(|alias| &alias.target)
            .collect();
        let mut matches: Vec<_> = self
            .objects
            .iter()
            .filter(|object| {
                object.display.to_lowercase().contains(&needle)
                    || object.target.id.to_lowercase().contains(&needle)
                    || object.target.kind.to_lowercase().contains(&needle)
                    || alias_targets.contains(&object.target)
            })
            .collect();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::TargetRef;
    use crate::navigation::AliasInfo;

    fn catalog(count: usize) -> Catalog {
        Catalog {
            objects: (0..count)
                .rev()
                .map(|index| CatalogObject {
                    target: TargetRef::new("entity", &format!("pier_{index:03}")),
                    display: "第七码头 Ä".into(),
                    file: "lore/places.wl".into(),
                    line: index as u32 + 1,
                })
                .collect(),
            aliases: vec![AliasInfo {
                target: TargetRef::new("entity", "pier_028"),
                name: "OldQuay29 旧港 ÄLIAS".into(),
                file: "aliases.wl".into(),
                line: 90,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn aliases_unicode_ids_and_kinds_keep_identity_and_definition_source() {
        let catalog = catalog(30);
        for query in [" oldquay29 ", "旧港", "älias", "PIER_028"] {
            let page = catalog
                .search_objects_page(query, Default::default())
                .unwrap();
            assert_eq!(page.total, 1, "{query}");
            assert_eq!(page.items[0].target, TargetRef::new("entity", "pier_028"));
            assert_eq!(page.items[0].file, "lore/places.wl");
            assert_eq!(page.items[0].line, 29);
        }
        for query in ["ENTITY", "第七码头", "ä", ""] {
            assert_eq!(
                catalog
                    .search_objects_page(query, Default::default())
                    .unwrap()
                    .total,
                30,
                "{query}"
            );
        }
        assert_eq!(
            catalog
                .search_objects_page("aliases.wl", Default::default())
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn full_pages_reach_every_same_name_object_in_stable_order() {
        let mut catalog = catalog(45);
        catalog.objects.push(CatalogObject {
            target: TargetRef::new("character", "pier_028"),
            display: "第七码头 Ä".into(),
            file: "people.wl".into(),
            line: 4,
        });
        let mut options = ObjectSearchOptions {
            limit: 8,
            ..Default::default()
        };
        let mut targets = Vec::new();
        loop {
            let page = catalog.search_objects_page("第七码头", options).unwrap();
            assert_eq!(page.total, 46);
            assert_eq!(page.offset, options.offset);
            assert_eq!(page.limit, 8);
            assert!(page.truncated);
            targets.extend(page.items.into_iter().map(|object| object.target));
            let Some(next) = page.next_offset else { break };
            options.offset = next;
        }
        assert_eq!(targets.len(), 46);
        assert!(targets.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(targets.contains(&TargetRef::new("entity", "pier_044")));
        assert!(targets.contains(&TargetRef::new("character", "pier_028")));
        catalog.objects.reverse();
        let first = catalog.search_objects_page("", Default::default()).unwrap();
        assert_eq!(first.items[0].target, targets[0]);
    }

    #[test]
    fn invalid_bounds_are_explicit_and_totals_are_never_partial() {
        let catalog = catalog(30);
        for limit in [0, MAX_OBJECT_SEARCH_LIMIT + 1, usize::MAX] {
            assert!(matches!(
                catalog.search_objects_page(
                    "",
                    ObjectSearchOptions {
                        limit,
                        ..Default::default()
                    }
                ),
                Err(ObjectSearchError::InvalidLimit { .. })
            ));
        }
        for max_candidates in [0, MAX_OBJECT_SEARCH_CANDIDATES + 1, usize::MAX] {
            assert!(matches!(
                catalog.search_objects_page(
                    "",
                    ObjectSearchOptions {
                        max_candidates,
                        ..Default::default()
                    }
                ),
                Err(ObjectSearchError::InvalidCandidateBudget { .. })
            ));
        }
        assert_eq!(
            catalog
                .search_objects_page(
                    "no match",
                    ObjectSearchOptions {
                        max_candidates: 29,
                        ..Default::default()
                    }
                )
                .unwrap_err(),
            ObjectSearchError::CandidateBudgetExceeded {
                candidates: 30,
                budget: 29
            }
        );
        for offset in [31, usize::MAX] {
            assert_eq!(
                catalog
                    .search_objects_page(
                        "",
                        ObjectSearchOptions {
                            offset,
                            ..Default::default()
                        }
                    )
                    .unwrap_err(),
                ObjectSearchError::InvalidOffset { offset, total: 30 }
            );
        }
        let end = catalog
            .search_objects_page(
                "",
                ObjectSearchOptions {
                    offset: 30,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(end.items.is_empty());
        assert_eq!(end.next_offset, None);
        let empty = catalog
            .search_objects_page("absent", Default::default())
            .unwrap();
        assert_eq!(empty.total, 0);
        assert!(!empty.truncated);
        assert_eq!(empty.next_offset, None);
        let whole = catalog
            .search_objects_page(
                "",
                ObjectSearchOptions {
                    limit: 100,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!whole.truncated);
        assert_eq!(whole.items.len(), whole.total);
    }
}

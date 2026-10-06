use super::filters::{missing_list, property_list, relation_list, value_list};
use super::{
    source_relative, CatalogQuery, CatalogQueryFilter, MissingCondition, PropertyScalar,
    QueryError, RelationDirection, CATALOG_QUERY_SCHEMA_VERSION, DEFAULT_CATALOG_QUERY_CANDIDATES,
    MAX_CATALOG_QUERY_CANDIDATES, MAX_CATALOG_QUERY_PAGE_SIZE,
};
use crate::analysis::Analysis;
use crate::ast::PropertyValue;
use crate::catalog::{Catalog, CatalogObject, TargetRef};
use crate::project::Project;
use crate::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogQueryOptions {
    pub offset: usize,
    pub page_size: usize,
    pub max_candidates: usize,
}

impl Default for CatalogQueryOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            page_size: 50,
            max_candidates: DEFAULT_CATALOG_QUERY_CANDIDATES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuerySource {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogQueryMatch {
    pub target: TargetRef,
    pub display: String,
    pub source: QuerySource,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogQueryCursor {
    pub schema_version: u32,
    pub offset: usize,
    pub page_size: usize,
    #[serde(default = "default_cursor_candidate_budget")]
    pub max_candidates: usize,
    pub query_fingerprint: String,
    pub snapshot: String,
}

fn default_cursor_candidate_budget() -> usize {
    DEFAULT_CATALOG_QUERY_CANDIDATES
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogQueryPage {
    pub schema_version: u32,
    pub summary: String,
    pub snapshot: String,
    pub offset: usize,
    pub total: usize,
    pub items: Vec<CatalogQueryMatch>,
    pub next: Option<CatalogQueryCursor>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Project {
    /// 在当前缓冲上查询；不刷新、保存或以查询结果修改 Project。
    pub fn query_catalog(
        &self,
        query: &CatalogQuery,
        options: CatalogQueryOptions,
    ) -> Result<CatalogQueryPage, QueryError> {
        self.query_catalog_cancellable(query, options, || false)
    }

    /// 只接受同一查询与当前内容基线的游标；陈旧游标明确失效，不按旧 offset 猜测续页。
    pub fn continue_catalog_query(
        &self,
        query: &CatalogQuery,
        cursor: &CatalogQueryCursor,
    ) -> Result<CatalogQueryPage, QueryError> {
        if cursor.schema_version != CATALOG_QUERY_SCHEMA_VERSION
            || cursor.snapshot != self.content_baseline()
            || cursor.query_fingerprint != query_fingerprint(query)?
        {
            return Err(QueryError::StaleCursor);
        }
        self.query_catalog(
            query,
            CatalogQueryOptions {
                offset: cursor.offset,
                page_size: cursor.page_size,
                max_candidates: cursor.max_candidates,
            },
        )
    }

    /// 过滤候选时每 64 个对象检查一次取消回调；取消不返回部分页。
    pub fn query_catalog_cancellable<F>(
        &self,
        query: &CatalogQuery,
        options: CatalogQueryOptions,
        mut cancelled: F,
    ) -> Result<CatalogQueryPage, QueryError>
    where
        F: FnMut() -> bool,
    {
        query.validate(&self.root)?;
        validate_options(options)?;
        let snapshot = self.content_baseline();
        let compiled = self.compile_current();
        let candidates = compiled.analysis.catalog.objects.len();
        if candidates > options.max_candidates {
            return Err(QueryError::CandidateBudgetExceeded {
                candidates,
                budget: options.max_candidates,
            });
        }
        let evaluator = QueryEvaluator::new(query, &compiled.analysis.catalog);
        let mut matches = Vec::new();
        for (index, object) in compiled.analysis.catalog.objects.iter().enumerate() {
            if index % 64 == 0 && cancelled() {
                return Err(QueryError::Cancelled);
            }
            let reasons =
                matching_reasons(query, object, &compiled.analysis, &evaluator, &self.root);
            if let Some(reasons) = reasons {
                matches.push(CatalogQueryMatch {
                    target: object.target.clone(),
                    display: object.display.clone(),
                    source: QuerySource {
                        file: object.file.clone(),
                        line: object.line,
                    },
                    reasons,
                });
            }
        }
        if cancelled() {
            return Err(QueryError::Cancelled);
        }
        super::sorting::sort_matches(&mut matches, query.sort);
        if cancelled() {
            return Err(QueryError::Cancelled);
        }
        let total = matches.len();
        let page_end = options.offset.saturating_add(options.page_size).min(total);
        let items = if options.offset >= total {
            Vec::new()
        } else {
            matches[options.offset..page_end].to_vec()
        };
        let fingerprint = query_fingerprint(query)?;
        let next = (page_end < total).then(|| CatalogQueryCursor {
            schema_version: CATALOG_QUERY_SCHEMA_VERSION,
            offset: page_end,
            page_size: options.page_size,
            max_candidates: options.max_candidates,
            query_fingerprint: fingerprint,
            snapshot: snapshot.clone(),
        });
        Ok(CatalogQueryPage {
            schema_version: CATALOG_QUERY_SCHEMA_VERSION,
            summary: query.summary(),
            snapshot,
            offset: options.offset,
            total,
            items,
            next,
            diagnostics: compiled.diagnostics,
        })
    }
}
fn validate_options(options: CatalogQueryOptions) -> Result<(), QueryError> {
    if !(1..=MAX_CATALOG_QUERY_PAGE_SIZE).contains(&options.page_size) {
        return Err(QueryError::InvalidOptions(format!(
            "page_size 必须在 1–{MAX_CATALOG_QUERY_PAGE_SIZE} 之间"
        )));
    }
    if !(1..=MAX_CATALOG_QUERY_CANDIDATES).contains(&options.max_candidates) {
        return Err(QueryError::InvalidOptions(format!(
            "max_candidates 必须在 1–{MAX_CATALOG_QUERY_CANDIDATES} 之间"
        )));
    }
    Ok(())
}

fn query_fingerprint(query: &CatalogQuery) -> Result<String, QueryError> {
    let bytes = serde_json::to_vec(query)
        .map_err(|error| QueryError::InvalidQuery(format!("无法编码查询：{error}")))?;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{hash:016x}"))
}

fn matching_reasons(
    query: &CatalogQuery,
    object: &CatalogObject,
    analysis: &Analysis,
    evaluator: &QueryEvaluator,
    root: &Path,
) -> Option<Vec<String>> {
    let mut reasons = Vec::new();
    for filter in &query.filters {
        let (matched, reason) = match filter {
            CatalogQueryFilter::Kind { values, negate } => {
                let matched = values.iter().any(|kind| kind == &object.target.kind);
                (*negate != matched, format!("类型 {}", value_list(values)))
            }
            CatalogQueryFilter::Name { values, negate } => {
                let aliases = analysis
                    .catalog
                    .aliases
                    .iter()
                    .filter(|alias| alias.target == object.target)
                    .map(|alias| alias.name.as_str())
                    .collect::<Vec<_>>();
                let matched = values.iter().any(|value| {
                    let needle = value.to_ascii_lowercase();
                    object.display.to_ascii_lowercase().contains(&needle)
                        || object.target.id.to_ascii_lowercase().contains(&needle)
                        || aliases
                            .iter()
                            .any(|alias| alias.to_ascii_lowercase().contains(&needle))
                });
                (
                    *negate != matched,
                    format!("名称/别名 {}", value_list(values)),
                )
            }
            CatalogQueryFilter::Tag {
                values,
                recursive: _,
                negate,
            } => {
                let matched = !values.is_empty() && evaluator.tag_targets.contains(&object.target);
                (*negate != matched, format!("标签 {}", value_list(values)))
            }
            CatalogQueryFilter::Property { values, negate } => {
                let matched = values.iter().any(|condition| {
                    property_value(analysis, &object.target, &condition.key)
                        .is_some_and(|value| property_equals(value, &condition.equals))
                });
                (
                    *negate != matched,
                    format!("属性 {}", property_list(values)),
                )
            }
            CatalogQueryFilter::Relation { values, negate } => {
                let matched =
                    !values.is_empty() && evaluator.relation_targets.contains(&object.target);
                (
                    *negate != matched,
                    format!("明确关系 {}", relation_list(values)),
                )
            }
            CatalogQueryFilter::AuthorScope {
                source_files,
                negate,
            } => {
                let source = source_relative(root, &object.file);
                let matched = source_files
                    .iter()
                    .any(|file| file.replace('\\', "/") == source);
                (
                    *negate != matched,
                    format!("作者范围 {}", value_list(source_files)),
                )
            }
            CatalogQueryFilter::Missing { values, negate } => {
                let matched = values
                    .iter()
                    .any(|condition| is_missing(analysis, evaluator, &object.target, condition));
                (*negate != matched, format!("缺值 {}", missing_list(values)))
            }
        };
        if !matched {
            return None;
        }
        reasons.push(if filter.negated() {
            format!("未命中否定条件：{reason}")
        } else {
            format!("命中：{reason}")
        });
    }
    if reasons.is_empty() {
        reasons.push("无筛选条件，匹配全部资料".into());
    }
    Some(reasons)
}

fn property_value<'a>(
    analysis: &'a Analysis,
    target: &TargetRef,
    key: &str,
) -> Option<&'a PropertyValue> {
    match target.kind.as_str() {
        "character" => analysis
            .symbols
            .characters
            .get(&target.id)
            .and_then(|info| info.properties.get(key)),
        "entity" => analysis
            .catalog
            .entities
            .get(&target.id)
            .and_then(|info| info.properties.get(key)),
        "tag" => analysis
            .catalog
            .tags
            .get(&target.id)
            .and_then(|info| info.properties.get(key)),
        "relation" => analysis
            .catalog
            .relations
            .get(&target.id)
            .and_then(|info| info.properties.get(key)),
        "world" => analysis
            .world
            .as_ref()
            .filter(|world| world.id == target.id)
            .and_then(|world| world.properties.get(key)),
        _ => None,
    }
}

fn property_equals(actual: &PropertyValue, expected: &PropertyScalar) -> bool {
    match (actual, expected) {
        (PropertyValue::Str(actual), PropertyScalar::String(expected)) => actual == expected,
        (PropertyValue::Num(actual), PropertyScalar::Number(expected)) => actual == expected,
        (PropertyValue::Bool(actual), PropertyScalar::Boolean(expected)) => actual == expected,
        (PropertyValue::Ref(actual), PropertyScalar::Reference(expected)) => actual == expected,
        _ => false,
    }
}

struct QueryEvaluator {
    tag_targets: BTreeSet<TargetRef>,
    relation_targets: BTreeSet<TargetRef>,
    relation_types_by_target: BTreeMap<TargetRef, BTreeSet<String>>,
}

impl QueryEvaluator {
    fn new(query: &CatalogQuery, catalog: &Catalog) -> Self {
        let mut evaluator = Self {
            tag_targets: BTreeSet::new(),
            relation_targets: BTreeSet::new(),
            relation_types_by_target: BTreeMap::new(),
        };
        for filter in &query.filters {
            match filter {
                CatalogQueryFilter::Tag {
                    values, recursive, ..
                } => {
                    for tag in values {
                        evaluator.tag_targets.extend(
                            catalog
                                .query(tag, *recursive)
                                .into_iter()
                                .map(|object| object.target),
                        );
                    }
                }
                CatalogQueryFilter::Relation { values, .. } => {
                    for relation in catalog.relations.values() {
                        for condition in values {
                            if condition
                                .relation_type
                                .as_ref()
                                .is_some_and(|kind| kind != &relation.relation_type)
                            {
                                continue;
                            }
                            let outgoing = condition.direction != RelationDirection::Incoming
                                && condition
                                    .related
                                    .as_ref()
                                    .is_none_or(|related| related == &relation.to_ref);
                            let incoming = condition.direction != RelationDirection::Outgoing
                                && condition
                                    .related
                                    .as_ref()
                                    .is_none_or(|related| related == &relation.from_ref);
                            if outgoing {
                                evaluator.relation_targets.insert(relation.from_ref.clone());
                            }
                            if incoming {
                                evaluator.relation_targets.insert(relation.to_ref.clone());
                            }
                        }
                        evaluator
                            .relation_types_by_target
                            .entry(relation.from_ref.clone())
                            .or_default()
                            .insert(relation.relation_type.clone());
                        evaluator
                            .relation_types_by_target
                            .entry(relation.to_ref.clone())
                            .or_default()
                            .insert(relation.relation_type.clone());
                    }
                }
                CatalogQueryFilter::Missing { values, .. }
                    if values.iter().any(|condition| {
                        matches!(condition, MissingCondition::Relation { .. })
                    }) =>
                {
                    for relation in catalog.relations.values() {
                        evaluator
                            .relation_types_by_target
                            .entry(relation.from_ref.clone())
                            .or_default()
                            .insert(relation.relation_type.clone());
                        evaluator
                            .relation_types_by_target
                            .entry(relation.to_ref.clone())
                            .or_default()
                            .insert(relation.relation_type.clone());
                    }
                }
                _ => {}
            }
        }
        evaluator
    }
}

fn is_missing(
    analysis: &Analysis,
    evaluator: &QueryEvaluator,
    object: &TargetRef,
    condition: &MissingCondition,
) -> bool {
    match condition {
        MissingCondition::Property { key } => property_value(analysis, object, key).is_none(),
        MissingCondition::Relation { relation_type } => evaluator
            .relation_types_by_target
            .get(object)
            .is_none_or(|types| match relation_type {
                Some(relation_type) => !types.contains(relation_type),
                None => types.is_empty(),
            }),
    }
}

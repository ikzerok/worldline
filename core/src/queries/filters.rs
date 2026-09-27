use super::{CATALOG_QUERY_SCHEMA_VERSION, MAX_CATALOG_QUERY_VALUES};
use crate::catalog::{TargetRef, TARGET_KINDS};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PropertyScalar {
    String(String),
    Number(f64),
    Boolean(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyCondition {
    pub key: String,
    pub equals: PropertyScalar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MissingCondition {
    Property { key: String },
    Relation { relation_type: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "dimension", rename_all = "snake_case")]
pub enum CatalogQueryFilter {
    Kind {
        values: Vec<String>,
        #[serde(default)]
        negate: bool,
    },
    Name {
        values: Vec<String>,
        #[serde(default)]
        negate: bool,
    },
    Tag {
        values: Vec<String>,
        #[serde(default)]
        recursive: bool,
        #[serde(default)]
        negate: bool,
    },
    Property {
        values: Vec<PropertyCondition>,
        #[serde(default)]
        negate: bool,
    },
    Relation {
        values: Vec<RelationCondition>,
        #[serde(default)]
        negate: bool,
    },
    AuthorScope {
        source_files: Vec<String>,
        #[serde(default)]
        negate: bool,
    },
    Missing {
        values: Vec<MissingCondition>,
        #[serde(default)]
        negate: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationCondition {
    #[serde(default)]
    pub relation_type: Option<String>,
    #[serde(default)]
    pub direction: RelationDirection,
    #[serde(default)]
    pub related: Option<TargetRef>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationDirection {
    Outgoing,
    Incoming,
    #[default]
    Either,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogQuery {
    pub schema_version: u32,
    #[serde(default)]
    pub filters: Vec<CatalogQueryFilter>,
}

impl Default for CatalogQuery {
    fn default() -> Self {
        Self {
            schema_version: CATALOG_QUERY_SCHEMA_VERSION,
            filters: Vec::new(),
        }
    }
}

impl CatalogQuery {
    pub fn validate(&self, root: &Path) -> Result<(), QueryError> {
        if self.schema_version != CATALOG_QUERY_SCHEMA_VERSION {
            return Err(QueryError::InvalidQuery(format!(
                "不支持的查询 schema_version:{}",
                self.schema_version
            )));
        }
        let mut dimensions = HashSet::new();
        for filter in &self.filters {
            let dimension = filter.dimension();
            if !dimensions.insert(dimension) {
                return Err(QueryError::InvalidQuery(format!(
                    "查询维度 {dimension} 重复；同维度候选应放在同一个 OR 集合"
                )));
            }
            if filter.value_count() > MAX_CATALOG_QUERY_VALUES {
                return Err(QueryError::InvalidQuery(format!(
                    "查询维度 {dimension} 的 OR 候选不能超过 {MAX_CATALOG_QUERY_VALUES} 项"
                )));
            }
            match filter {
                CatalogQueryFilter::Kind { values, .. } => {
                    if let Some(kind) = values
                        .iter()
                        .find(|kind| !TARGET_KINDS.contains(&kind.as_str()))
                    {
                        return Err(QueryError::InvalidQuery(format!("未知对象类型:{kind}")));
                    }
                }
                CatalogQueryFilter::Name { values, .. }
                | CatalogQueryFilter::Tag { values, .. } => {
                    if values.iter().any(|value| value.trim().is_empty()) {
                        return Err(QueryError::InvalidQuery("name/tag 候选值不能为空白".into()));
                    }
                }
                CatalogQueryFilter::Property { values, .. } => {
                    if values
                        .iter()
                        .any(|condition| condition.key.trim().is_empty())
                    {
                        return Err(QueryError::InvalidQuery("property key 不能为空".into()));
                    }
                    if values.iter().any(|condition| {
                        matches!(condition.equals, PropertyScalar::Number(value) if !value.is_finite())
                    }) {
                        return Err(QueryError::InvalidQuery(
                            "property 数值必须是有限数值".into(),
                        ));
                    }
                }
                CatalogQueryFilter::Relation { values, .. } => {
                    if values.iter().any(|condition| {
                        condition
                            .relation_type
                            .as_ref()
                            .is_some_and(|relation_type| relation_type.trim().is_empty())
                            || condition.related.as_ref().is_some_and(|target| {
                                target.kind.trim().is_empty() || target.id.trim().is_empty()
                            })
                    }) {
                        return Err(QueryError::InvalidQuery(
                            "关系条件的类型与目标身份不能为空".into(),
                        ));
                    }
                    if values.iter().any(|condition| {
                        condition
                            .related
                            .as_ref()
                            .is_some_and(|target| !TARGET_KINDS.contains(&target.kind.as_str()))
                    }) {
                        return Err(QueryError::InvalidQuery("关系条件含有未知目标类型".into()));
                    }
                }
                CatalogQueryFilter::AuthorScope { source_files, .. } => {
                    for file in source_files {
                        validate_source_scope(root, file)?;
                    }
                }
                CatalogQueryFilter::Missing { values, .. } => {
                    if values.iter().any(|condition| match condition {
                        MissingCondition::Property { key } => key.trim().is_empty(),
                        MissingCondition::Relation { relation_type } => relation_type
                            .as_ref()
                            .is_some_and(|kind| kind.trim().is_empty()),
                    }) {
                        return Err(QueryError::InvalidQuery(
                            "缺值条件的属性或关系类型不能为空".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut filters: Vec<_> = self.filters.iter().collect();
        filters.sort_by_key(|filter| filter.order());
        if filters.is_empty() {
            return "全部资料".into();
        }
        filters
            .into_iter()
            .map(CatalogQueryFilter::summary)
            .collect::<Vec<_>>()
            .join(" 且 ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    InvalidQuery(String),
    InvalidOptions(String),
    CandidateBudgetExceeded { candidates: usize, budget: usize },
    Cancelled,
    StaleCursor,
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidQuery(message) | Self::InvalidOptions(message) => {
                formatter.write_str(message)
            }
            Self::CandidateBudgetExceeded { candidates, budget } => {
                write!(formatter, "查询候选对象 {candidates} 超出预算 {budget}")
            }
            Self::Cancelled => formatter.write_str("查询已取消"),
            Self::StaleCursor => {
                formatter.write_str("StaleCursor：查询或 Project 快照已变化，请从第一页重查")
            }
        }
    }
}

impl QueryError {
    /// 稳定机器错误码，供 CLI、agent 与其他 core 调用方共用。
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidQuery(_) => "INVALID_QUERY",
            Self::InvalidOptions(_) => "INVALID_OPTIONS",
            Self::CandidateBudgetExceeded { .. } => "CANDIDATE_BUDGET_EXCEEDED",
            Self::Cancelled => "CANCELLED",
            Self::StaleCursor => "STALE_CURSOR",
        }
    }
}

impl std::error::Error for QueryError {}
fn validate_source_scope(root: &Path, file: &str) -> Result<(), QueryError> {
    let path = Path::new(file);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || path.extension().and_then(|extension| extension.to_str()) != Some("wl")
    {
        return Err(QueryError::InvalidQuery(format!(
            "作者范围必须是工作区内相对 .wl 路径:{file}"
        )));
    }
    let normalized_root = crate::compiler::source_path(root);
    let resolved = crate::compiler::source_path(&root.join(path));
    if !resolved.starts_with(normalized_root) {
        return Err(QueryError::InvalidQuery(format!(
            "作者范围路径越过工作区边界:{file}"
        )));
    }
    Ok(())
}
impl CatalogQueryFilter {
    fn dimension(&self) -> &'static str {
        match self {
            Self::Kind { .. } => "kind",
            Self::Name { .. } => "name",
            Self::Tag { .. } => "tag",
            Self::Property { .. } => "property",
            Self::Relation { .. } => "relation",
            Self::AuthorScope { .. } => "author_scope",
            Self::Missing { .. } => "missing",
        }
    }

    fn order(&self) -> u8 {
        match self {
            Self::Kind { .. } => 0,
            Self::Name { .. } => 1,
            Self::Tag { .. } => 2,
            Self::Property { .. } => 3,
            Self::Relation { .. } => 4,
            Self::AuthorScope { .. } => 5,
            Self::Missing { .. } => 6,
        }
    }

    pub(super) fn negated(&self) -> bool {
        match self {
            Self::Kind { negate, .. }
            | Self::Name { negate, .. }
            | Self::Tag { negate, .. }
            | Self::Property { negate, .. }
            | Self::Relation { negate, .. }
            | Self::AuthorScope { negate, .. }
            | Self::Missing { negate, .. } => *negate,
        }
    }

    fn value_count(&self) -> usize {
        match self {
            Self::Kind { values, .. } | Self::Name { values, .. } | Self::Tag { values, .. } => {
                values.len()
            }
            Self::Property { values, .. } => values.len(),
            Self::Relation { values, .. } => values.len(),
            Self::Missing { values, .. } => values.len(),
            Self::AuthorScope { source_files, .. } => source_files.len(),
        }
    }

    fn summary(&self) -> String {
        let (name, values) = match self {
            Self::Kind { values, .. } => ("类型", value_list(values)),
            Self::Name { values, .. } => ("名称/别名", value_list(values)),
            Self::Tag {
                values, recursive, ..
            } => (
                if *recursive { "递归标签" } else { "标签" },
                value_list(values),
            ),
            Self::Property { values, .. } => ("属性", property_list(values)),
            Self::Relation { values, .. } => ("明确关系", relation_list(values)),
            Self::AuthorScope { source_files, .. } => ("作者范围", value_list(source_files)),
            Self::Missing { values, .. } => ("缺值", missing_list(values)),
        };
        format!(
            "{}{}：{values}",
            if self.negated() { "非" } else { "" },
            name
        )
    }
}

pub(super) fn value_list(values: &[String]) -> String {
    if values.is_empty() {
        return "(空集合)".into();
    }
    values
        .iter()
        .map(|value| format!("「{value}」"))
        .collect::<Vec<_>>()
        .join(" 或 ")
}

pub(super) fn property_list(values: &[PropertyCondition]) -> String {
    if values.is_empty() {
        return "(空集合)".into();
    }
    values
        .iter()
        .map(|condition| {
            let value = match &condition.equals {
                PropertyScalar::String(value) => format!("「{value}」"),
                PropertyScalar::Number(value) => value.to_string(),
                PropertyScalar::Boolean(value) if *value => "是".into(),
                PropertyScalar::Boolean(_) => "否".into(),
            };
            format!("{} = {value}", condition.key)
        })
        .collect::<Vec<_>>()
        .join(" 或 ")
}

pub(super) fn relation_list(values: &[RelationCondition]) -> String {
    if values.is_empty() {
        return "(空集合)".into();
    }
    values
        .iter()
        .map(|condition| {
            let direction = match condition.direction {
                RelationDirection::Outgoing => "出边",
                RelationDirection::Incoming => "入边",
                RelationDirection::Either => "任一方向",
            };
            let related = condition.related.as_ref().map_or_else(
                || "任意目标".into(),
                |target| format!("{}:{}", target.kind, target.id),
            );
            format!(
                "{} {direction} {related}",
                condition.relation_type.as_deref().unwrap_or("任意类型"),
            )
        })
        .collect::<Vec<_>>()
        .join(" 或 ")
}

pub(super) fn missing_list(values: &[MissingCondition]) -> String {
    if values.is_empty() {
        return "(空集合)".into();
    }
    values
        .iter()
        .map(|condition| match condition {
            MissingCondition::Property { key } => format!("属性 {key}"),
            MissingCondition::Relation { relation_type } => {
                format!("关系 {}", relation_type.as_deref().unwrap_or("任意类型"))
            }
        })
        .collect::<Vec<_>>()
        .join(" 或 ")
}

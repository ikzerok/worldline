//! 资料库组合查询、共享已保存查询与待办只读投影。

use crate::analysis::Analysis;
use crate::ast::PropertyValue;
use crate::catalog::{Catalog, CatalogObject, TargetRef, TARGET_KINDS};
use crate::collaboration::{AnchorStatus, CommentAnchor, ProposalStatus};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json};
use crate::Diagnostic;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

pub const CATALOG_QUERY_SCHEMA_VERSION: u32 = 1;
pub const MAX_CATALOG_QUERY_PAGE_SIZE: usize = 100;
pub const MAX_CATALOG_QUERY_CANDIDATES: usize = 100_000;
pub const MAX_CATALOG_QUERY_VALUES: usize = 100;
pub const DEFAULT_CATALOG_QUERY_CANDIDATES: usize = 10_000;
pub const SAVED_QUERY_SCHEMA_VERSION: u64 = 1;
pub const SAVED_QUERY_REQUIRED_FEATURE: &str = "catalog.saved_queries.v1";

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedQueryDraft {
    pub id: String,
    pub name: String,
    pub query: CatalogQuery,
}

#[derive(Debug, Clone)]
pub struct SavedQueryDocument {
    pub draft: SavedQueryDraft,
    pub path: PathBuf,
    /// 原始对象用于更新已知字段时保留扩展字段。
    pub source: Value,
    pub read_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SavedQueryIndex {
    pub queries: BTreeMap<String, SavedQueryDocument>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoKind {
    BrokenLink,
    EntryToCreate,
    DetachedComment,
    OpenProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TodoItem {
    pub id: String,
    pub kind: TodoKind,
    /// 被检查的缺失对象，或产生待办的批注/提案身份。
    pub target: TargetRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub related_target: Option<TargetRef>,
    pub source: QuerySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TodoProjection {
    pub schema_version: u32,
    pub snapshot: String,
    pub items: Vec<TodoItem>,
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
                    source: QuerySource {
                        file: object.file.clone(),
                        line: object.line,
                    },
                    reasons,
                });
            }
        }
        matches.sort_by(|a, b| {
            (&a.target, &a.source.file, a.source.line).cmp(&(
                &b.target,
                &b.source.file,
                b.source.line,
            ))
        });
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

impl Project {
    /// 读取清单明确注册的共享查询；无效文档仍保留在 Project 的原始字节缓冲中。
    pub fn saved_query_index(&self) -> SavedQueryIndex {
        let mut index = SavedQueryIndex::default();
        let manifest = manifest_path(&self.root);
        let Ok(document) = self.authoring_document(&manifest) else {
            return index;
        };
        if document.is_deleted() {
            return index;
        }
        let registry = parse_registry(&self.root, document.bytes());
        index.diagnostics.extend(registry.diagnostics);
        for (registered_id, path) in registry.saved_queries {
            let parsed = (|| -> Result<(SavedQueryDraft, Value, bool), String> {
                let document = self.authoring_document(&path)?;
                if document.is_deleted() {
                    return Err("注册的已保存查询文档已删除".into());
                }
                let source = parse_unique_json(document.bytes())
                    .map_err(|error| format!("查询 JSON 无法解析:{error}"))?;
                if source.get("schema_version").and_then(Value::as_u64)
                    != Some(SAVED_QUERY_SCHEMA_VERSION)
                {
                    return Err("已保存查询 schema_version 不受支持，只读保留原文".into());
                }
                let draft: SavedQueryDraft = serde_json::from_value(source.clone())
                    .map_err(|error| format!("已保存查询结构无效:{error}"))?;
                validate_saved_query(&self.root, &draft)?;
                if draft.id != registered_id {
                    return Err("已保存查询 ID 与清单注册 ID 不一致".into());
                }
                Ok((draft, source, document.is_read_only()))
            })();
            match parsed {
                Ok((draft, source, read_only)) => {
                    index.queries.insert(
                        registered_id,
                        SavedQueryDocument {
                            draft,
                            path,
                            source,
                            read_only,
                        },
                    );
                }
                Err(error) => index.diagnostics.push(Diagnostic::error(
                    "QRY001",
                    &path.to_string_lossy(),
                    crate::Span::new(1, 1, 1),
                    error,
                )),
            }
        }
        crate::sort_diagnostics(&mut index.diagnostics);
        index
    }

    /// 创建或更新共享查询定义；个人收藏与待办状态不写入查询文档。
    pub fn save_saved_query(
        &mut self,
        draft: SavedQueryDraft,
        expected_baseline: &str,
    ) -> Result<Vec<PathBuf>, String> {
        if expected_baseline != self.content_baseline() {
            return Err("StaleBaseline：已保存查询写入基线已过期".into());
        }
        self.ensure_workspace_writable()?;
        validate_saved_query(&self.root, &draft)?;

        let mut candidate = self.clone();
        let manifest = manifest_path(&self.root);
        if !candidate.authoring_documents.contains_key(&manifest) {
            candidate.create_authoring_document(&manifest, minimal_manifest(&candidate)?)?;
        }
        let manifest_document = candidate.authoring_document(&manifest)?;
        if manifest_document.is_deleted() || manifest_document.is_read_only() {
            return Err("已保存查询需要可写的工作区清单".into());
        }
        let mut manifest_value = parse_unique_json(manifest_document.bytes())
            .map_err(|error| format!("工作区清单无法解析：{error}"))?;
        let registry = parse_registry(&self.root, manifest_document.bytes());
        let path = registry
            .saved_queries
            .get(&draft.id)
            .cloned()
            .unwrap_or_else(|| self.root.join(format!(".world/queries/{}.json", draft.id)));
        if registry
            .saved_queries
            .iter()
            .any(|(registered_id, registered_path)| {
                registered_id != &draft.id && registered_path == &path
            })
        {
            return Err("已保存查询路径已由其他 ID 注册，保留原文档".into());
        }
        let existing = candidate
            .authoring_documents
            .get(&path)
            .filter(|document| !document.deleted)
            .cloned();
        if let Some(document) = &existing {
            if document.read_only {
                return Err("已保存查询版本或必需能力未知，只能只读查看".into());
            }
            let source = parse_unique_json(&document.bytes)
                .map_err(|error| format!("已保存查询 JSON 无法解析：{error}"))?;
            if source.get("schema_version").and_then(Value::as_u64)
                != Some(SAVED_QUERY_SCHEMA_VERSION)
            {
                return Err("已保存查询 schema_version 未知，只读保留原文".into());
            }
            let existing_draft: SavedQueryDraft = serde_json::from_value(source.clone())
                .map_err(|error| format!("现有查询文档结构无效，只读保留原文：{error}"))?;
            validate_saved_query(&self.root, &existing_draft)
                .map_err(|error| format!("现有查询无效，只读保留原文：{error}"))?;
            if existing_draft.id != draft.id {
                return Err("已保存查询 ID 与请求或清单注册 ID 不一致，保留原文档".into());
            }
        }

        let mut changed = Vec::new();
        if existing.is_none() {
            let manifest_object = manifest_value
                .as_object_mut()
                .ok_or("工作区清单顶层必须是对象")?;
            let features = manifest_object
                .entry("required_features")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("清单 required_features 必须是数组")?;
            if features.iter().any(|feature| !feature.is_string()) {
                return Err("清单 required_features 只能包含字符串".into());
            }
            if !features
                .iter()
                .any(|feature| feature.as_str() == Some(SAVED_QUERY_REQUIRED_FEATURE))
            {
                features.push(json!(SAVED_QUERY_REQUIRED_FEATURE));
            }
            let registrations = manifest_object
                .entry("saved_queries")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or("清单 saved_queries 必须是对象")?;
            if registrations.contains_key(&draft.id) {
                return Err("已保存查询 ID 已注册但路径无效，保留原注册".into());
            }
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| "已保存查询路径越出工作区")?
                .to_string_lossy()
                .replace('\\', "/");
            registrations.insert(draft.id.clone(), json!(relative));
            candidate.set_authoring_document(
                &manifest,
                serde_json::to_vec_pretty(&manifest_value).map_err(|error| error.to_string())?,
            )?;
            changed.push(manifest.clone());
            if !candidate.authoring_diagnostics().is_empty() {
                return Err("注册已保存查询后清单诊断无效，未应用更改".into());
            }
        }

        let fresh = serde_json::to_value(&draft).map_err(|error| error.to_string())?;
        let mut saved = existing
            .as_ref()
            .map(|document| parse_unique_json(&document.bytes))
            .transpose()
            .map_err(|error| format!("已保存查询 JSON 无法解析：{error}"))?
            .unwrap_or_else(|| json!({}));
        merge_json_preserving_unknown(&mut saved, &fresh);
        saved["schema_version"] = json!(SAVED_QUERY_SCHEMA_VERSION);
        let bytes = serde_json::to_vec_pretty(&saved).map_err(|error| error.to_string())?;
        if existing.is_some() {
            candidate.set_authoring_document(&path, bytes)?;
        } else {
            candidate.create_authoring_document(&path, bytes)?;
        }
        changed.push(path);
        *self = candidate;
        Ok(changed)
    }

    /// 汇总当前缓冲中的明确未完成事项；只读，不为投影创建持久身份或修改来源文档。
    pub fn todo_projection(&self) -> TodoProjection {
        let content = self.compile_current();
        let maps = crate::presentation_commands::map_index_with_content(self, &content);
        let comments = crate::collaboration::build_comment_index(self, &content, &maps);
        let proposals = crate::collaboration::build_proposal_index(self);
        let mut diagnostics = content.diagnostics.clone();
        diagnostics.extend(maps.diagnostics.iter().cloned());
        diagnostics.extend(comments.diagnostics.iter().cloned());
        diagnostics.extend(proposals.diagnostics.iter().cloned());

        let mut items = Vec::new();
        let mut missing_targets: BTreeMap<TargetRef, Vec<&crate::navigation::TextLinkInfo>> =
            BTreeMap::new();
        for link in &content.analysis.catalog.text_links {
            if content.analysis.catalog.object(&link.target).is_some() {
                continue;
            }
            let source = QuerySource {
                file: link.file.clone(),
                line: link.line,
            };
            let source_path = source_relative(&self.root, &link.file);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::BrokenLink,
                    &source_path,
                    link.line,
                    Some(link.column),
                    &link.target,
                    "",
                ),
                kind: TodoKind::BrokenLink,
                target: link.target.clone(),
                related_target: Some(link.source.clone()),
                source,
                column: Some(link.column),
                reason: format!(
                    "正文链接指向不存在的 {} `{}`",
                    link.target.kind, link.target.id
                ),
            });
            missing_targets
                .entry(link.target.clone())
                .or_default()
                .push(link);
        }
        for (target, mut links) in missing_targets {
            links.sort_by(|left, right| {
                (&left.file, left.line, left.column).cmp(&(&right.file, right.line, right.column))
            });
            let first = links[0];
            let source_path = source_relative(&self.root, &first.file);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::EntryToCreate,
                    &source_path,
                    first.line,
                    Some(first.column),
                    &target,
                    "",
                ),
                kind: TodoKind::EntryToCreate,
                target: target.clone(),
                related_target: None,
                source: QuerySource {
                    file: first.file.clone(),
                    line: first.line,
                },
                column: Some(first.column),
                reason: format!(
                    "缺失条目 {} `{}` 被 {} 处正文链接引用",
                    target.kind,
                    target.id,
                    links.len()
                ),
            });
        }

        for comment in comments.comments.values().filter(|comment| {
            !comment.draft.resolved && comment.anchor_status == AnchorStatus::Detached
        }) {
            let related_target = match &comment.draft.anchor {
                CommentAnchor::Object { target } => Some(target.clone()),
                CommentAnchor::MapPlacement { map_id, .. } => Some(TargetRef::new("map", map_id)),
                CommentAnchor::TextRange { path, .. } => Some(TargetRef::new("file", path)),
            };
            let source_file = comment.path.to_string_lossy().into_owned();
            let line = authoring_document_line(self, &comment.path, &comment.draft.id);
            let source_path = source_relative(&self.root, &source_file);
            let target = TargetRef::new("comment", &comment.draft.id);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::DetachedComment,
                    &source_path,
                    line,
                    None,
                    &target,
                    &comment.draft.id,
                ),
                kind: TodoKind::DetachedComment,
                target,
                related_target,
                source: QuerySource {
                    file: source_file,
                    line,
                },
                column: None,
                reason: format!("未解决批注 `{}` 的锚点已失效", comment.draft.id),
            });
        }

        for proposal in proposals
            .proposals
            .values()
            .filter(|proposal| proposal.draft.status == ProposalStatus::Open)
        {
            let source_file = proposal.path.to_string_lossy().into_owned();
            let line = authoring_document_line(self, &proposal.path, &proposal.draft.id);
            let source_path = source_relative(&self.root, &source_file);
            let target = TargetRef::new("proposal", &proposal.draft.id);
            items.push(TodoItem {
                id: todo_id(
                    TodoKind::OpenProposal,
                    &source_path,
                    line,
                    None,
                    &target,
                    &proposal.draft.id,
                ),
                kind: TodoKind::OpenProposal,
                target,
                related_target: None,
                source: QuerySource {
                    file: source_file,
                    line,
                },
                column: None,
                reason: format!("提案 `{}` 等待审阅", proposal.draft.id),
            });
        }
        items.sort_by(|left, right| {
            (
                left.kind,
                &left.target,
                &left.source.file,
                left.source.line,
                left.column,
                &left.id,
            )
                .cmp(&(
                    right.kind,
                    &right.target,
                    &right.source.file,
                    right.source.line,
                    right.column,
                    &right.id,
                ))
        });
        crate::sort_diagnostics(&mut diagnostics);
        TodoProjection {
            schema_version: 1,
            snapshot: self.content_baseline(),
            items,
            diagnostics,
        }
    }
}

fn validate_saved_query(root: &Path, draft: &SavedQueryDraft) -> Result<(), String> {
    if !crate::workspace_documents::valid_id(&draft.id) {
        return Err("已保存查询 ID 无效".into());
    }
    if draft.name.trim().is_empty() || draft.name.contains(['\n', '\r']) {
        return Err("已保存查询名称不能为空或跨行".into());
    }
    draft
        .query
        .validate(root)
        .map_err(|error| error.to_string())
}

fn minimal_manifest(project: &Project) -> Result<Vec<u8>, String> {
    let entry = project
        .entry
        .strip_prefix(&project.root)
        .map_err(|_| "工程入口越出工作区")?
        .to_string_lossy()
        .replace('\\', "/");
    let raw_id = project
        .root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let mut id = raw_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if !id
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
    {
        id.insert(0, '_');
    }
    let value = json!({
        "schema_version": 1,
        "project_id": id,
        "language_version": project.language_version(),
        "entry": entry,
        "required_features": [SAVED_QUERY_REQUIRED_FEATURE],
        "maps": {},
        "graph_views": {},
        "saved_queries": {}
    });
    serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())
}

fn merge_json_preserving_unknown(original: &mut Value, fresh: &Value) {
    match (original, fresh) {
        (Value::Object(original), Value::Object(fresh)) => {
            for (key, value) in fresh {
                match original.get_mut(key) {
                    Some(existing) => merge_json_preserving_unknown(existing, value),
                    None => {
                        original.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (Value::Array(original), Value::Array(fresh))
            if fresh
                .iter()
                .all(|value| value.get("dimension").and_then(Value::as_str).is_some()) =>
        {
            let mut merged = Vec::with_capacity(fresh.len());
            for filter in fresh {
                let prior = original
                    .iter()
                    .find(|candidate| candidate.get("dimension") == filter.get("dimension"));
                if let Some(prior) = prior {
                    let mut retained = prior.clone();
                    merge_json_preserving_unknown(&mut retained, filter);
                    merged.push(retained);
                } else {
                    merged.push(filter.clone());
                }
            }
            *original = merged;
        }
        (Value::Array(original), Value::Array(fresh)) => {
            for (index, value) in fresh.iter().enumerate() {
                if let Some(existing) = original.get_mut(index) {
                    merge_json_preserving_unknown(existing, value);
                } else {
                    original.push(value.clone());
                }
            }
            original.truncate(fresh.len());
        }
        (original, fresh) => *original = fresh.clone(),
    }
}

fn todo_id(
    kind: TodoKind,
    source_path: &str,
    line: u32,
    column: Option<u32>,
    target: &TargetRef,
    extra: &str,
) -> String {
    let identity = format!(
        "{kind:?}\0{source_path}\0{line}\0{column:?}\0{}\0{}\0{extra}",
        target.kind, target.id
    );
    let mut hash = 0xcbf29ce484222325u64;
    for byte in identity.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("todo_{hash:016x}")
}

fn authoring_document_line(project: &Project, path: &Path, id: &str) -> u32 {
    let Ok(document) = project.authoring_document(path) else {
        return 1;
    };
    let marker = format!("\"id\": \"{id}\"");
    String::from_utf8_lossy(document.bytes())
        .lines()
        .position(|line| line.contains(&marker))
        .map(|line| line as u32 + 1)
        .unwrap_or(1)
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

fn source_relative(root: &Path, source: &str) -> String {
    let source = crate::compiler::source_path(Path::new(source));
    source
        .strip_prefix(crate::compiler::source_path(root))
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| source.to_string_lossy().replace('\\', "/"))
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

    fn negated(&self) -> bool {
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

fn value_list(values: &[String]) -> String {
    if values.is_empty() {
        return "(空集合)".into();
    }
    values
        .iter()
        .map(|value| format!("「{value}」"))
        .collect::<Vec<_>>()
        .join(" 或 ")
}

fn property_list(values: &[PropertyCondition]) -> String {
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

fn relation_list(values: &[RelationCondition]) -> String {
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

fn missing_list(values: &[MissingCondition]) -> String {
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

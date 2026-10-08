//! 可传输的完整查询物化；分页不编译，不访问文件系统。
use super::pagination::{matching_reasons, query_fingerprint, validate_options, QueryEvaluator};
use super::{CatalogQuery, CatalogQueryCursor, CatalogQueryMatch, CatalogQueryOptions, QueryError};
use crate::{CompileResult, Diagnostic, Severity, Span};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::Path;

pub const MAX_CATALOG_SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_CATALOG_SNAPSHOT_DIAGNOSTICS: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogScopeDiagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub file: String,
    pub span: Span,
    pub note: Option<String>,
    pub suggestion: Option<String>,
    pub related: Vec<(String, Span)>,
}
impl From<&Diagnostic> for CatalogScopeDiagnostic {
    fn from(value: &Diagnostic) -> Self {
        Self {
            severity: value.severity,
            code: value.code.into(),
            message: value.message.clone(),
            file: value.file.clone(),
            span: value.span,
            note: value.note.clone(),
            suggestion: value.suggestion.clone(),
            related: value.related.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogQuerySnapshot {
    pub schema_version: u32,
    pub summary: String,
    pub snapshot: String,
    pub query_fingerprint: String,
    pub max_candidates: usize,
    pub candidates: usize,
    pub incomplete: bool,
    pub diagnostics: Vec<CatalogScopeDiagnostic>,
    matches: Vec<CatalogQueryMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSnapshotPage {
    pub schema_version: u32,
    pub summary: String,
    pub snapshot: String,
    pub offset: usize,
    pub total: usize,
    pub incomplete: bool,
    pub items: Vec<CatalogQueryMatch>,
    pub next: Option<CatalogQueryCursor>,
    pub diagnostics: Vec<CatalogScopeDiagnostic>,
}

impl CatalogQuerySnapshot {
    pub(crate) fn from_content(
        root: &Path,
        baseline: String,
        content: &CompileResult,
        query: &CatalogQuery,
        max_candidates: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Self, QueryError> {
        query.validate(root)?;
        validate_options(CatalogQueryOptions {
            max_candidates,
            ..Default::default()
        })?;
        let candidates = content.analysis.catalog.objects.len();
        if candidates > max_candidates {
            return Err(QueryError::CandidateBudgetExceeded {
                candidates,
                budget: max_candidates,
            });
        }
        if content.diagnostics.len() > MAX_CATALOG_SNAPSHOT_DIAGNOSTICS {
            return Err(budget_error("诊断数量超过 10,000"));
        }
        let evaluator = QueryEvaluator::new(query, &content.analysis.catalog);
        let mut matches = Vec::new();
        let mut bytes = 0;
        for (index, object) in content.analysis.catalog.objects.iter().enumerate() {
            if index % 64 == 0 && cancelled() {
                return Err(QueryError::Cancelled);
            }
            if let Some(reasons) =
                matching_reasons(query, object, &content.analysis, &evaluator, root)
            {
                encoded_size(object, MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes))?;
                let item = CatalogQueryMatch {
                    target: object.target.clone(),
                    display: object.display.clone(),
                    source: super::QuerySource {
                        file: object.file.clone(),
                        line: object.line,
                    },
                    reasons,
                };
                bytes += encoded_size(&item, MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes))?;
                matches.push(item);
            }
        }
        super::sorting::sort_matches(&mut matches, query.sort);
        let mut diagnostics = Vec::new();
        for diagnostic in &content.diagnostics {
            encoded_size(diagnostic, MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes))?;
            let item = CatalogScopeDiagnostic::from(diagnostic);
            bytes += encoded_size(&item, MAX_CATALOG_SNAPSHOT_BYTES.saturating_sub(bytes))?;
            diagnostics.push(item);
        }
        if cancelled() {
            return Err(QueryError::Cancelled);
        }
        let snapshot = Self {
            schema_version: 1,
            summary: query.summary(),
            snapshot: baseline,
            query_fingerprint: query_fingerprint(query)?,
            max_candidates,
            candidates,
            incomplete: diagnostics.iter().any(|d| d.severity == Severity::Error),
            diagnostics,
            matches,
        };
        encoded_size(&snapshot, MAX_CATALOG_SNAPSHOT_BYTES)?;
        Ok(snapshot)
    }

    pub fn matches(&self) -> &[CatalogQueryMatch] {
        &self.matches
    }
    pub fn total(&self) -> usize {
        self.matches.len()
    }
    pub fn matches_query(&self, query: &CatalogQuery) -> bool {
        query_fingerprint(query).is_ok_and(|fingerprint| fingerprint == self.query_fingerprint)
    }
    pub fn page(&self, offset: usize, page_size: usize) -> Result<CatalogSnapshotPage, QueryError> {
        validate_options(CatalogQueryOptions {
            offset,
            page_size,
            max_candidates: self.max_candidates,
        })?;
        let total = self.total();
        let end = offset.saturating_add(page_size).min(total);
        Ok(CatalogSnapshotPage {
            schema_version: 1,
            summary: self.summary.clone(),
            snapshot: self.snapshot.clone(),
            offset,
            total,
            incomplete: self.incomplete,
            items: self.matches.get(offset..end).unwrap_or_default().to_vec(),
            next: (end < total).then(|| CatalogQueryCursor {
                schema_version: super::CATALOG_QUERY_SCHEMA_VERSION,
                offset: end,
                page_size,
                max_candidates: self.max_candidates,
                query_fingerprint: self.query_fingerprint.clone(),
                snapshot: self.snapshot.clone(),
            }),
            diagnostics: self.diagnostics.clone(),
        })
    }
    pub fn continue_page(
        &self,
        cursor: &CatalogQueryCursor,
    ) -> Result<CatalogSnapshotPage, QueryError> {
        if cursor.schema_version != super::CATALOG_QUERY_SCHEMA_VERSION
            || cursor.snapshot != self.snapshot
            || cursor.query_fingerprint != self.query_fingerprint
            || cursor.max_candidates != self.max_candidates
        {
            return Err(QueryError::StaleCursor);
        }
        self.page(cursor.offset, cursor.page_size)
    }
    pub(crate) fn validate_wire(&self) -> Result<(), QueryError> {
        if self.schema_version != 1
            || self.matches.len() > self.candidates
            || self.candidates > self.max_candidates
            || self.max_candidates == 0
            || self.max_candidates > super::MAX_CATALOG_QUERY_CANDIDATES
            || self.diagnostics.len() > MAX_CATALOG_SNAPSHOT_DIAGNOSTICS
        {
            return Err(budget_error("查询快照身份或数量无效"));
        }
        encoded_size(self, MAX_CATALOG_SNAPSHOT_BYTES).map(|_| ())
    }
}

impl crate::project::Project {
    pub fn catalog_query_snapshot(
        &self,
        query: &CatalogQuery,
        max_candidates: usize,
    ) -> Result<CatalogQuerySnapshot, QueryError> {
        self.catalog_query_snapshot_cancellable(query, max_candidates, || false)
    }
    pub fn catalog_query_snapshot_cancellable(
        &self,
        query: &CatalogQuery,
        max_candidates: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<CatalogQuerySnapshot, QueryError> {
        query.validate(&self.root)?;
        validate_options(CatalogQueryOptions {
            max_candidates,
            ..Default::default()
        })?;
        if cancelled() {
            return Err(QueryError::Cancelled);
        }
        let baseline = self.content_baseline();
        let content = self.compile_problems_snapshot();
        CatalogQuerySnapshot::from_content(
            &self.root,
            baseline,
            &content,
            query,
            max_candidates,
            &mut cancelled,
        )
    }
}

pub(crate) fn budget_error(message: &str) -> QueryError {
    QueryError::InvalidOptions(format!("查询范围预算：{message}"))
}
pub(crate) fn encoded_size(value: &impl Serialize, limit: usize) -> Result<usize, QueryError> {
    struct Budget {
        count: usize,
        limit: usize,
    }
    impl Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.count) {
                return Err(io::Error::other("查询范围 JSON 超过 32 MiB 预算"));
            }
            self.count += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Budget { count: 0, limit };
    serde_json::to_writer(&mut writer, value).map_err(|_| budget_error("完整 JSON 超过 32 MiB"))?;
    Ok(writer.count)
}

pub(crate) fn check_clone_bytes<'a>(
    parts: impl IntoIterator<Item = &'a str>,
    remaining: usize,
) -> Result<(), QueryError> {
    let mut used = 0usize;
    for part in parts {
        used = used
            .checked_add(part.len())
            .ok_or_else(|| budget_error("元数据字节溢出"))?;
        if used > remaining {
            return Err(budget_error("单项元数据超过剩余字节预算"));
        }
    }
    Ok(())
}

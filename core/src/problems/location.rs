use super::*;
use crate::{project::Project, Span};
use std::path::{Component, Path};

pub(crate) fn relative(project: &Project, path: &Path) -> Option<String> {
    let path = path.strip_prefix(&project.root).ok()?;
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || path.as_os_str().is_empty()
    {
        return None;
    }
    let path = path.to_str()?;
    #[cfg(windows)]
    {
        Some(path.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        Some(path.to_owned())
    }
}

pub(crate) fn project_location(
    project: &Project,
    file: &str,
    span: crate::Span,
    role: Option<ProblemSourceRole>,
    excerpt_limit: usize,
) -> ProblemLocation {
    let role = role.unwrap_or(ProblemSourceRole::Document);
    let path = Path::new(file);
    let relative = relative(project, path);
    let mut location = ProblemLocation {
        path: relative,
        precision: ProblemPrecision::Unavailable,
        span: None,
        byte_range: None,
        char_range: None,
        excerpt: None,
        excerpt_truncated: false,
        reason: None,
        context: Some(ProblemSourceContext::empty(role)),
    };
    if location.path.is_none() {
        location.reason = Some("outside_workspace".into());
        return location;
    }
    if role == ProblemSourceRole::Unavailable {
        location.reason = Some("source_unavailable".into());
        return location;
    }
    if project.documents.contains_key(path)
        && project.source_selection().is_some_and(|selection| !selection.is_active(path))
    {
        location.reason = Some("inactive_source".into());
        return location;
    }
    let text = if let Some(document) = project.documents.get(path).filter(|d| !d.is_deleted()) {
        Some(document.text.as_str())
    } else {
        project
            .authoring_documents
            .get(path)
            .filter(|d| !d.is_deleted())
            .and_then(|d| std::str::from_utf8(d.bytes()).ok())
    };
    let Some(text) = text else {
        location.reason = Some("source_unavailable".into());
        return location;
    };
    let exact = matches!(
        role,
        ProblemSourceRole::Target | ProblemSourceRole::Expression
            | ProblemSourceRole::Statement | ProblemSourceRole::Declaration
    );
    let context = if exact {
        let Some(line) = ranges(text, span) else {
            location.reason = Some("invalid_span".into());
            return location;
        };
        location.precision = ProblemPrecision::Span;
        location.span = Some(span);
        location.byte_range = Some(ProblemRange {
            start: line.byte_base + line.hit.start,
            end: line.byte_base + line.hit.end,
        });
        location.char_range = Some(ProblemRange {
            start: line.char_base + span.column as usize - 1,
            end: line.char_base + span.column as usize - 1 + span.length as usize,
        });
        super::context::project(
            line.text, line.byte_base, line.char_base, Some(line.hit), role, excerpt_limit,
        )
    } else {
        location.precision = ProblemPrecision::Document;
        location.reason = Some("document_only".into());
        super::context::project(text, 0, 0, None, role, excerpt_limit)
    };
    location.excerpt = context.text.clone();
    location.excerpt_truncated = context.prefix_clipped || context.suffix_clipped
        || context.visibility == ProblemContextVisibility::Partial;
    location.context = Some(context);
    location
}

struct SourceLine<'a> {
    text: &'a str,
    byte_base: usize,
    char_base: usize,
    hit: ProblemRange,
}
fn ranges(text: &str, span: Span) -> Option<SourceLine<'_>> {
    let line_index = usize::try_from(span.line.checked_sub(1)?).ok()?;
    let column = usize::try_from(span.column.checked_sub(1)?).ok()?;
    let length = usize::try_from(span.length).ok()?;
    let mut byte_base = 0;
    let mut char_base = 0;
    // Preserve the empty final line and remove only the CR belonging to CRLF.
    let mut lines = text.split('\n');
    for _ in 0..line_index {
        let line = lines.next()?;
        byte_base += line.len() + 1;
        char_base += line.chars().count() + 1;
    }
    let raw = lines.next()?;
    let line = if byte_base + raw.len() < text.len() {
        raw.strip_suffix('\r').unwrap_or(raw)
    } else {
        raw
    };
    let boundaries: Vec<usize> = line
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(line.len()))
        .collect();
    let end_column = column.checked_add(length)?;
    Some(SourceLine {
        text: line,
        byte_base,
        char_base,
        hit: ProblemRange {
            start: *boundaries.get(column)?,
            end: *boundaries.get(end_column)?,
        },
    })
}

impl Project {
    pub fn problem_location(
        &self,
        report: &ProblemsReport,
        problem_id: &str,
        related_index: Option<usize>,
    ) -> Result<ProblemLocation, ProblemsError> {
        if self.content_baseline() != report.content_baseline
            || self.problems_observation_key().ok().as_deref()
                != Some(report.source_observation.as_str())
        {
            return Err(ProblemsError::new(
                "STALE_REPORT",
                "问题报告不属于当前已应用缓冲，请刷新",
            ));
        }
        report.check_problem_id(problem_id)?;
        let entry = report
            .entries
            .iter()
            .find(|entry| entry.id == problem_id)
            .ok_or_else(|| ProblemsError::new("UNKNOWN_PROBLEM", "当前报告没有此问题"))?;
        let location = match related_index {
            Some(index) => report
                .related
                .get(problem_id)
                .and_then(|list| list.get(index))
                .ok_or_else(|| ProblemsError::new("UNKNOWN_PROBLEM", "关联来源不存在或已截断"))?,
            None => &entry.primary,
        };
        if location.path.as_deref().is_some_and(|path| !super::query::valid_path(path)) {
            return Err(ProblemsError::new(
                "INVALID_QUERY",
                "问题来源路径不在工作区内",
            ));
        }
        if report.schema_version != 1
            || location.context.as_ref().is_none_or(|context| context.version != 1)
            || report.report_version != super::version::of(report)
            || report.reasons.iter().any(|reason| matches!(
                reason.as_str(), "source_conflict" | "external_observation_changed"
            ))
        {
            return Err(ProblemsError::new(
                "STALE_REPORT",
                "问题来源证据不属于当前格式或稿件，请刷新",
            ));
        }
        let Some(path) = location.path.as_deref() else {
            return Ok(location.clone());
        };
        // Re-project from the current loaded buffers; never trust transported byte offsets.
        if location.precision == ProblemPrecision::Unavailable {
            return Ok(location.clone());
        }
        let current = project_location(
            self,
            &self.root.join(path).to_string_lossy(),
            location.span.unwrap_or_default(),
            location.context.as_ref().map(|context| context.role),
            report.limits.max_excerpt_bytes,
        );
        if &current != location {
            return Err(ProblemsError::new(
                "STALE_REPORT",
                "问题来源或位置证据已经改变",
            ));
        }
        Ok(current)
    }
}

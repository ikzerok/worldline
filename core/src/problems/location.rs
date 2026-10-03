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
    exact: bool,
    excerpt_limit: usize,
) -> ProblemLocation {
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
    };
    if location.path.is_none() {
        location.reason = Some("outside_workspace".into());
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
    let excerpt = if exact {
        let Some((bytes, chars, line)) = ranges(text, span) else {
            location.reason = Some("invalid_span".into());
            return location;
        };
        location.precision = ProblemPrecision::Span;
        location.span = Some(span);
        location.byte_range = Some(bytes);
        location.char_range = Some(chars);
        line
    } else {
        location.precision = ProblemPrecision::Document;
        location.reason = Some("document_only".into());
        text
    };
    let (text, truncated) = clipped(excerpt, excerpt_limit);
    location.excerpt = Some(text);
    location.excerpt_truncated = truncated;
    location
}

fn ranges(text: &str, span: Span) -> Option<(ProblemRange, ProblemRange, &str)> {
    let line_index = usize::try_from(span.line.checked_sub(1)?).ok()?;
    let column = usize::try_from(span.column.checked_sub(1)?).ok()?;
    let length = usize::try_from(span.length).ok()?;
    let mut byte_base = 0;
    let mut char_base = 0;
    // split preserves an empty last physical line and removes only CRLF's CR.
    let mut lines = text.split('\n');
    for _ in 0..line_index {
        let line = lines.next()?;
        byte_base += line.len() + 1;
        char_base += line.chars().count() + 1;
    }
    let line = lines.next()?.trim_end_matches('\r');
    let boundaries: Vec<usize> = line
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(line.len()))
        .collect();
    let end_column = column.checked_add(length)?;
    let start = *boundaries.get(column)?;
    let end = *boundaries.get(end_column)?;
    Some((
        ProblemRange {
            start: byte_base + start,
            end: byte_base + end,
        },
        ProblemRange {
            start: char_base + column,
            end: char_base + end_column,
        },
        line,
    ))
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
        let Some(path) = location.path.as_deref() else {
            return Ok(location.clone());
        };
        if !super::query::valid_path(path) {
            return Err(ProblemsError::new(
                "INVALID_QUERY",
                "问题来源路径不在工作区内",
            ));
        }
        // Re-project from the current loaded buffers; never trust transported byte offsets.
        if location.precision == ProblemPrecision::Unavailable {
            return Ok(location.clone());
        }
        let current = project_location(
            self,
            &self.root.join(path).to_string_lossy(),
            location.span.unwrap_or_default(),
            location.precision == ProblemPrecision::Span,
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

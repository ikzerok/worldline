use super::*;
use serde::Serialize;

pub(crate) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn normalized(query: &ProblemQuery) -> Result<ProblemQuery, ProblemsError> {
    if query.path.as_deref().is_some_and(|p| !valid_path(p)) || query.text.len() > 16_384 {
        return Err(ProblemsError::new("INVALID_QUERY", "路径或搜索文本无效"));
    }
    let mut query = query.clone();
    query.severities.sort();
    query.severities.dedup();
    query.domains.sort();
    query.domains.dedup();
    query.text = query.text.to_lowercase();
    Ok(query)
}
fn size<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |v| v.len())
}
fn limit(value: usize) -> Result<usize, ProblemsError> {
    match value {
        0 => Ok(50),
        1..=200 => Ok(value),
        _ => Err(ProblemsError::new("INVALID_QUERY", "每页条目上限为 200")),
    }
}
impl ProblemsReport {
    fn offset(
        &self,
        cursor: Option<&ProblemCursor>,
        key: &str,
        total: usize,
    ) -> Result<usize, ProblemsError> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };
        if cursor.report_version != self.report_version {
            return Err(ProblemsError::new(
                "STALE_REPORT",
                "分页游标对应的报告已改变",
            ));
        }
        if cursor.query_key != key || cursor.offset > total {
            return Err(ProblemsError::new(
                "INVALID_CURSOR",
                "分页游标与筛选条件或分页种类不匹配",
            ));
        }
        Ok(cursor.offset)
    }
    fn cursor(&self, key: String, next: usize, total: usize) -> Option<ProblemCursor> {
        (next < total).then(|| ProblemCursor {
            report_version: self.report_version.clone(),
            query_key: key,
            offset: next,
        })
    }
    pub fn query(
        &self,
        query: &ProblemQuery,
        cursor: Option<&ProblemCursor>,
        requested_limit: usize,
    ) -> Result<ProblemPage, ProblemsError> {
        let limit = limit(requested_limit)?;
        let query = normalized(query)?;
        let key = digest(&serde_json::to_vec(&("problems", &query)).expect("可序列化筛选"));
        let matches: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| {
                (query.severities.is_empty() || query.severities.contains(&entry.severity))
                    && (query.domains.is_empty() || query.domains.contains(&entry.domain))
                    && (query.path.is_none() || query.path == entry.primary.path)
                    && (query.text.is_empty()
                        || [
                            Some(entry.message.as_str()),
                            Some(entry.code.as_str()),
                            entry.note.as_deref(),
                            entry.suggestion.as_deref(),
                            entry.primary.path.as_deref(),
                        ]
                        .into_iter()
                        .flatten()
                        .any(|text| text.to_lowercase().contains(&query.text)))
            })
            .collect();
        let offset = self.offset(cursor, &key, matches.len())?;
        let mut entries = Vec::new();
        // Reserve enough room for metadata and a continuation cursor.
        let mut bytes = 4096;
        for entry in matches.iter().skip(offset).take(limit) {
            let cost = size(entry);
            if bytes + cost > 1024 * 1024 {
                break;
            }
            bytes += cost;
            entries.push((*entry).clone());
        }
        if entries.is_empty() && offset < matches.len() {
            return Err(ProblemsError::new(
                "BUDGET_EXCEEDED",
                "单条问题超过响应字节预算",
            ));
        }
        Ok(ProblemPage {
            report_version: self.report_version.clone(),
            content_baseline: self.content_baseline.clone(),
            total: self.entries.len(),
            matched: matches.len(),
            next_cursor: self.cursor(key, offset + entries.len(), matches.len()),
            entries,
            complete: self.complete,
            truncated: self.truncated,
        })
    }
    pub fn related_page(
        &self,
        problem_id: &str,
        cursor: Option<&ProblemCursor>,
        requested_limit: usize,
    ) -> Result<ProblemRelatedPage, ProblemsError> {
        let limit = limit(requested_limit)?;
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == problem_id)
            .ok_or_else(|| ProblemsError::new("UNKNOWN_PROBLEM", "当前报告没有此问题"))?;
        let locations = self
            .related
            .get(problem_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let key = digest(&serde_json::to_vec(&("related", problem_id)).expect("可序列化身份"));
        let offset = self.offset(cursor, &key, locations.len())?;
        let mut page = Vec::new();
        let mut bytes = 4096;
        for location in locations.iter().skip(offset).take(limit) {
            let cost = size(location);
            if bytes + cost > 1024 * 1024 {
                break;
            }
            bytes += cost;
            page.push(location.clone());
        }
        if page.is_empty() && offset < locations.len() {
            return Err(ProblemsError::new(
                "BUDGET_EXCEEDED",
                "单个关联来源超过响应字节预算",
            ));
        }
        Ok(ProblemRelatedPage {
            report_version: self.report_version.clone(),
            problem_id: problem_id.into(),
            total: entry.related_count,
            next_cursor: self.cursor(key, offset + page.len(), locations.len()),
            locations: page,
            truncated: locations.len() < entry.related_count,
        })
    }
}

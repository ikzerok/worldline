use super::*;
use crate::{project::Project, CompileResult, ValueKind};

impl Project {
    pub fn condition_candidates_from_compile(
        &self,
        compiled: &CompileResult,
        query: &ConditionCandidateQuery,
    ) -> Result<ConditionCandidates, String> {
        super::protocol::snapshot(self, compiled)?;
        super::protocol::version(query.schema_version)?;
        if compiled.has_errors() {
            return Err("候选声明含正式错误或歧义；请修复后重新查询".into());
        }
        if query.limit == 0 || query.limit > 100 || query.text.len() > 512 {
            return Err("条件候选页长须为 1–100，检索文本至多 512 字节".into());
        }
        let text = query.text.to_lowercase();
        let matches = |id: &str, display: &str| {
            text.is_empty()
                || id.to_lowercase().contains(&text)
                || display.to_lowercase().contains(&text)
        };
        let catalog = &compiled.analysis.catalog;
        let mut candidates = Vec::new();
        match query.kind {
            ConditionCandidateKind::Variable => {
                for (id, info) in &compiled.analysis.symbols.vars {
                    let value_type = match info.kind {
                        Some(ValueKind::Num) => ConditionValueType::Num,
                        Some(ValueKind::Str) => ConditionValueType::Str,
                        Some(ValueKind::Bool) => ConditionValueType::Bool,
                        _ => continue,
                    };
                    if matches(id, id) {
                        candidates.push((
                            id.as_str(),
                            id.as_str(),
                            Some(value_type),
                            None,
                            info.decl_file.as_str(),
                            info.decl_span.line,
                        ));
                    }
                }
            }
            ConditionCandidateKind::State => {
                for (id, info) in &catalog.states {
                    if matches(id, &info.display) && catalog.object(&info.target).is_some() {
                        candidates.push((
                            id.as_str(),
                            info.display.as_str(),
                            None,
                            Some(&info.target),
                            info.file.as_str(),
                            info.line,
                        ));
                    }
                }
            }
            ConditionCandidateKind::Tag => {
                for (id, info) in &catalog.tags {
                    if info.declared && matches(id, &info.display) {
                        candidates.push((
                            id.as_str(),
                            info.display.as_str(),
                            None,
                            None,
                            info.file.as_str(),
                            info.line,
                        ));
                    }
                }
            }
        }
        candidates.sort_by_key(|c| c.0);
        let total = candidates.len();
        let kind = match query.kind {
            ConditionCandidateKind::Variable => "variable",
            ConditionCandidateKind::State => "state",
            ConditionCandidateKind::Tag => "tag",
        };
        let selected = candidates
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect::<Vec<_>>();
        let mut remaining = MAX_CONDITION_OUTPUT_BYTES - 4096;
        for (id, display, _, owner, file, _) in &selected {
            let size = id
                .len()
                .saturating_add(display.len())
                .saturating_add(file.len())
                .saturating_add(owner.map_or(0, |owner| owner.kind.len() + owner.id.len()))
                .saturating_mul(6)
                .saturating_add(512);
            remaining = remaining
                .checked_sub(size)
                .ok_or("条件候选超过 8 MiB 输出预算")?;
        }
        let items = selected
            .into_iter()
            .map(
                |(id, display, value_type, owner, file, line)| ConditionCandidate {
                    target: crate::TargetRef::new(kind, id),
                    display: display.to_owned(),
                    value_type,
                    owner: owner.cloned(),
                    file: file.to_owned(),
                    line,
                },
            )
            .collect::<Vec<_>>();
        let result = ConditionCandidates {
            schema_version: 1,
            baseline: self.content_baseline(),
            generation: self.search_refresh_generation(),
            total,
            offset: query.offset,
            has_more: query.offset.saturating_add(items.len()) < total,
            items,
        };
        super::protocol::measure(&result, MAX_CONDITION_OUTPUT_BYTES)?;
        Ok(result)
    }
}

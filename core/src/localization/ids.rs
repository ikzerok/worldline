use super::*;
use crate::project::Project;
use std::collections::{BTreeMap, BTreeSet};

impl Project {
    pub fn preview_localization_ids(
        &self,
        draft: &LocalizationIdDraft,
    ) -> Result<LocalizationIdPlan, LocalizationError> {
        prepare(self, draft)
            .map(|(plan, _)| plan)
            .map_err(LocalizationError::from)
    }

    pub fn apply_localization_ids(
        &mut self,
        draft: &LocalizationIdDraft,
        expected_plan_digest: &str,
    ) -> Result<LocalizationIdResult, LocalizationError> {
        let (plan, candidate) = prepare(self, draft).map_err(LocalizationError::from)?;
        if plan.plan_digest != expected_plan_digest {
            return Err(LocalizationError::new(
                "STALE_PLAN",
                "稳定 ID 预览已过期，请重新预览",
            ));
        }
        if !plan.can_apply {
            let first = plan.diagnostics.first();
            return Err(LocalizationError::new(
                first.map_or("VALIDATION_FAILED", |d| d.code.as_str()),
                first.map_or("稳定 ID 计划未通过校验", |d| d.message.as_str()),
            ));
        }
        editing::guard(self).map_err(LocalizationError::from)?;
        let baseline = self.content_baseline();
        let new_baseline = candidate.content_baseline();
        let changed_files = plan
            .changes
            .iter()
            .map(|change| self.root.join(&change.file))
            .collect();
        *self = candidate;
        Ok(LocalizationIdResult {
            plan,
            changed_files,
            baseline,
            new_baseline,
        })
    }
}

fn prepare(
    project: &Project,
    draft: &LocalizationIdDraft,
) -> Result<(LocalizationIdPlan, Project), String> {
    if draft.schema_version != 1 {
        return Err("UNSUPPORTED_VERSION：不支持的稳定 ID 草稿版本".into());
    }
    limits::budget(
        draft.assignments.len() <= MAX_LOCALIZATION_BATCH,
        "ID 分配数量",
    )?;
    if draft.assignments.is_empty() {
        return Err("VALIDATION_FAILED：至少明确选择一个稳定 ID 分配".into());
    }
    limits::serialized(draft, MAX_LOCALIZATION_JSON_BYTES, "ID 草稿输入")?;
    for assignment in &draft.assignments {
        limits::id(&assignment.id)?;
        if let Some(id) = &assignment.expected_id {
            limits::id(id)?;
        }
    }
    editing::guard(project)?;
    if !project.compile_options().localization_ids {
        return Err("FEATURE_REQUIRED：请先显式启用 content.localization.v1".into());
    }
    let content_baseline = project.content_baseline();
    let source_baseline = export::source_baseline(project)?;
    let (compiled, records) = source::current(project)?;
    let mut diagnostics = Vec::new();
    if source_baseline != draft.source_baseline {
        issue(
            &mut diagnostics,
            "STALE_SOURCE",
            None,
            "稳定 ID 草稿的源码基线已过期",
        )?;
    }
    let mut selected = BTreeSet::new();
    let mut replacements = BTreeMap::<String, BTreeMap<u32, (&Option<String>, &String)>>::new();
    for assignment in &draft.assignments {
        let matching: Vec<_> = records
            .iter()
            .filter(|r| r.unit.source == assignment.source)
            .collect();
        if matching.len() != 1
            || !selected.insert((assignment.source.file.clone(), assignment.source.line))
        {
            issue(
                &mut diagnostics,
                "INVALID_SOURCE",
                Some(assignment),
                "来源必须精确选中一个未重复的当前 AST 单元",
            )?;
            continue;
        }
        let record = matching[0];
        if record.id != assignment.expected_id
            || record.unit.source_revision != assignment.source_revision
        {
            issue(
                &mut diagnostics,
                "STALE_SOURCE",
                Some(assignment),
                "当前来源、源修订或原稳定 ID 与草稿不匹配",
            )?;
            continue;
        }
        replacements
            .entry(assignment.source.file.clone())
            .or_default()
            .insert(
                assignment.source.line,
                (&assignment.expected_id, &assignment.id),
            );
    }
    let mut candidate = project.clone();
    let mut changes = Vec::new();
    if diagnostics.is_empty() {
        for (file, lines) in &replacements {
            let path = project.root.join(file);
            crate::source_lifecycle::safety::writable_path(&path)?;
            let before = project.document(&path)?;
            let after = replace_lines(before, lines)?;
            candidate.set_text(&path, after.clone())?;
            if before != after {
                changes.push(LocalizationSourceChange {
                    file: file.clone(),
                    before: before.into(),
                    after,
                });
            }
        }
        let (after, new_records) = source::current(&candidate)?;
        let mut counts = BTreeMap::new();
        for record in &new_records {
            if let Some(id) = &record.id {
                *counts.entry(id).or_insert(0usize) += 1;
            }
        }
        for assignment in &draft.assignments {
            if counts.get(&assignment.id) != Some(&1) {
                issue(
                    &mut diagnostics,
                    "DUPLICATE_ID",
                    Some(assignment),
                    "新稳定 ID 在最终活动源码中不唯一",
                )?;
            }
        }
        let before_content: Vec<_> = records
            .iter()
            .map(|r| (&r.unit.source, &r.unit.source_revision, &r.unit.parts))
            .collect();
        let after_content: Vec<_> = new_records
            .iter()
            .map(|r| (&r.unit.source, &r.unit.source_revision, &r.unit.parts))
            .collect();
        if compiled.analysis.fingerprint != after.analysis.fingerprint
            || before_content != after_content
        {
            issue(
                &mut diagnostics,
                "SOURCE_MISMATCH",
                None,
                "稳定 ID 注记改变了源内容或运行语义，计划已拒绝",
            )?;
        }
    }
    if diagnostics.is_empty() && changes.is_empty() {
        issue(
            &mut diagnostics,
            "NO_CHANGE",
            None,
            "稳定 ID 与当前源码一致，无需重复应用",
        )?;
    }
    let can_apply = diagnostics.is_empty();
    let digest_bytes = serde_json::to_vec(&(
        draft,
        &content_baseline,
        &source_baseline,
        project.search_refresh_generation(),
        &changes,
        &diagnostics,
    ))
    .map_err(|e| e.to_string())?;
    let plan = LocalizationIdPlan {
        schema_version: 1,
        plan_digest: export::digest("worldline-localization-id-plan-v1", &digest_bytes),
        content_baseline,
        source_baseline,
        changes,
        diagnostics,
        can_apply,
    };
    limits::serialized(&plan, limits::MAX_OUTPUT_BYTES, "ID 计划输出")?;
    Ok((plan, candidate))
}

fn issue(
    out: &mut Vec<LocalizationDiagnostic>,
    code: &str,
    assignment: Option<&LocalizationIdAssignment>,
    message: &str,
) -> Result<(), String> {
    limits::budget(out.len() < limits::MAX_DIAGNOSTICS, "诊断数")?;
    out.push(LocalizationDiagnostic {
        code: code.into(),
        id: assignment.map(|a| a.id.clone()),
        source: assignment.map(|a| a.source.clone()),
        message: message.into(),
    });
    Ok(())
}

fn replace_lines(
    source: &str,
    replacements: &BTreeMap<u32, (&Option<String>, &String)>,
) -> Result<String, String> {
    let comments = crate::lexer::comment_source_spans(source);
    let mut output = String::with_capacity(source.len());
    let mut start = 0usize;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        let Some((old, new)) = replacements.get(&(index as u32 + 1)) else {
            output.push_str(line);
            start += line.len();
            continue;
        };
        // Use the language's exact comment ranges; preserve Unicode, trailing comments and CRLF bytes.
        let code_end = super::source::statement_code_range(line, start, &comments)
            .ok_or("稳定 ID 来源没有正文")?
            .end;
        let code = &line[..code_end];
        if let Some(old) = old {
            let annotation = format!("#wl-localization:{old}");
            let prefix = code
                .strip_suffix(&annotation)
                .ok_or("SOURCE_MISMATCH：旧稳定 ID 注记不在预期行尾")?;
            if !prefix.ends_with(char::is_whitespace) {
                return Err("SOURCE_MISMATCH：旧稳定 ID 注记边界无效".into());
            }
            output.push_str(prefix);
            output.push_str("#wl-localization:");
            output.push_str(new);
        } else {
            output.push_str(code);
            output.push_str(" #wl-localization:");
            output.push_str(new);
        }
        output.push_str(&line[code_end..]);
        start += line.len();
    }
    Ok(output)
}

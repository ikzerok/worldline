use super::*;
use crate::{capabilities::CapabilityEnableRequest, LanguageVersion};

pub(super) fn guard(project: &Project, buffer: &WritingBuffer) -> Result<()> {
    project
        .ensure_workspace_writable()
        .map_err(|e| DialogueError::new("READ_ONLY", e))?;
    if buffer.baseline() != project.content_baseline()
        || project.document(buffer.path()).ok() != Some(buffer.original())
    {
        return Err(DialogueError::new(
            "STALE_BASELINE",
            "正文完整基线已过期，请保留输入并重新预览",
        ));
    }
    crate::authoring_intents::require_active_source(project, buffer.path())
        .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?;
    crate::file_access::within(&project.root, buffer.path())
        .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?;
    crate::source_lifecycle::safety::writable_path(buffer.path())
        .map_err(|e| DialogueError::new("READ_ONLY", e))?;
    if !project.recovery_conflicts().is_empty() {
        return Err(DialogueError::new(
            "EXTERNAL_CONFLICT",
            "工程有未解决的保存恢复冲突",
        ));
    }
    project
        .verify_review_navigation()
        .map_err(|e| DialogueError::new("EXTERNAL_CONFLICT", e))?;
    super::super::commands::ensure_disk_matches_saved_baselines(project)
        .map_err(|e| DialogueError::new("EXTERNAL_CONFLICT", e))
}

pub(super) fn prepare(
    project: &Project,
    buffer: &WritingBuffer,
    request: &DialogueEditRequest,
) -> Result<(Project, DialogueEditPlan)> {
    if request.schema_version != 1 {
        return Err(DialogueError::new(
            "INVALID_REQUEST",
            "不支持的台词请求版本",
        ));
    }
    if serde_json::to_vec(request)
        .map_err(|e| DialogueError::new("INVALID_REQUEST", e.to_string()))?
        .len()
        > MAX_DIALOGUE_REQUEST_BYTES
        || buffer
            .source()
            .len()
            .saturating_add(buffer.original().len())
            > MAX_DIALOGUE_CHANGE_BYTES
    {
        return Err(DialogueError::new(
            "BUDGET_EXCEEDED",
            "台词请求或完整草稿预览超过预算",
        ));
    }
    guard(project, buffer)?;
    if request.expected_baseline != project.content_baseline() {
        return Err(DialogueError::new(
            "STALE_BASELINE",
            "台词请求的工程基线已过期",
        ));
    }
    if request.generation != buffer.generation() {
        return Err(DialogueError::new(
            "STALE_DRAFT",
            "台词输入代次已过期，输入已保留",
        ));
    }
    let workspace_guard = workspace(project)?;
    let mut candidate = project.clone();
    candidate
        .set_text(buffer.path(), buffer.source().into())
        .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?;
    let before = candidate.compile_current();
    let projection = projection::build(project, buffer, &request.target, &before)?;
    let edit = writer::prepare(buffer, &projection, &request.operation)?;
    let mut plan = DialogueEditPlan {
        schema_version: 1,
        request: request.clone(),
        baseline: project.content_baseline(),
        generation: buffer.generation(),
        snapshot: projection.snapshot,
        source_path: relative(project, buffer.path())?,
        range: edit.range.clone(),
        before: edit.before.clone(),
        after: edit.after.clone(),
        old_speaker: edit.original.as_ref().and_then(|s| s.draft.speaker.clone()),
        new_speaker: edit.expected.as_ref().and_then(|s| s.speaker.clone()),
        metadata_losses: edit.losses.clone(),
        migration: None,
        changes: Vec::new(),
        includes_unapplied_draft: buffer.is_changed(),
        runtime_fingerprint_before: before.analysis.fingerprint,
        runtime_fingerprint_after: before.analysis.fingerprint,
        fingerprint_comparison_reliable: true,
        can_apply: edit.confirmed,
        no_change: edit.no_change,
        plan_digest: String::new(),
        root: project.root.clone(),
        refresh_generation: project.search_refresh_generation(),
        workspace_guard,
        continuation: None,
    };
    if edit.no_change {
        plan.includes_unapplied_draft = false;
        plan.continuation = continuation::witness(project, buffer, &edit, project, &plan)?;
        digest(&mut plan)?;
        return Ok((project.clone(), plan));
    }
    let requires_say = edit
        .expected
        .as_ref()
        .is_some_and(|draft| draft.kind == DialogueKind::Say);
    if requires_say && !candidate.language_version_kind().supports_language_111() {
        if !request.enable_language_1_11 {
            return Err(DialogueError::new(
                "MIGRATION_REQUIRED",
                "正式台词需要明确预览启用语言 1.11；工程尚未升级",
            ));
        }
        let migration = candidate
            .plan_capability_enable(&CapabilityEnableRequest {
                target_language: LanguageVersion::V1_11,
                enable_features: Vec::new(),
                expected_baseline: candidate.content_baseline(),
            })
            .map_err(|e| DialogueError::new("INVALID_DRAFT", e))?;
        plan.can_apply &= migration.can_apply;
        plan.fingerprint_comparison_reliable &= migration.can_apply;
        if migration.can_apply {
            candidate
                .apply_capability_enable(&migration)
                .map_err(|e| DialogueError::new("INVALID_DRAFT", e))?;
        }
        plan.migration = Some(migration);
    }
    if buffer.source().get(edit.range.clone()) != Some(edit.before.as_str()) {
        return Err(DialogueError::new(
            "STALE_DRAFT",
            "正式语句的完整来源片段已变化",
        ));
    }
    let mut source = buffer.source().to_owned();
    source.replace_range(edit.range.clone(), &edit.after);
    candidate
        .set_text(buffer.path(), source.clone())
        .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?;
    if plan
        .migration
        .as_ref()
        .is_none_or(|migration| migration.can_apply)
    {
        let after = candidate.compile_current();
        if after.has_errors() {
            let detail = after
                .diagnostics
                .iter()
                .find(|d| d.severity == crate::Severity::Error)
                .map(|d| d.message.as_str())
                .unwrap_or("候选编译失败");
            return Err(DialogueError::new(
                "INVALID_DRAFT",
                format!("台词候选无效，输入未应用：{detail}"),
            ));
        }
        if let Some(expected) = &edit.expected {
            // 重新打开候选缓冲仅作正式 AST 校验，不推进原缓冲、原 Project 或保存基线。
            let candidate_buffer = candidate
                .open_source_writing_buffer(buffer.path())
                .map_err(|e| DialogueError::new("SOURCE_UNAVAILABLE", e))?;
            let verified =
                projection::build(&candidate, &candidate_buffer, &request.target, &after)?;
            let actual = verified
                .statements
                .iter()
                .find(|s| s.source.line == edit.line)
                .ok_or_else(|| {
                    DialogueError::new("INVALID_DRAFT", "候选不能无损表示为一条正式语句")
                })?;
            if !parts::equivalent(expected, &actual.draft) || !metadata_matches(&edit, actual) {
                return Err(DialogueError::new(
                    "INVALID_DRAFT",
                    "现有语言无法无损表示此正文或元数据；请保留输入并使用源码入口",
                ));
            }
        }
        plan.runtime_fingerprint_after = after.analysis.fingerprint;
    }
    changes(project, &candidate, buffer, &mut plan)?;
    // 阻断迁移仍显示真实清单候选字节，绝不伪称已经应用。
    if let Some(migration) = plan.migration.as_ref().filter(|m| !m.can_apply) {
        let path = crate::workspace_documents::manifest_path(&project.root);
        if !plan.changes.iter().any(|change| change.path == path) {
            plan.changes.push(WritingAuthoringChange {
                path,
                before: migration.manifest_bytes_before().map(utf8).transpose()?,
                after: utf8(migration.manifest_bytes_after())?,
                includes_unapplied_draft: false,
            });
        }
    }
    plan.changes.sort_by(|a, b| a.path.cmp(&b.path));
    check_change_budget(&plan)?;
    guard(project, buffer)?;
    if workspace(project)? != plan.workspace_guard {
        return Err(DialogueError::new(
            "EXTERNAL_CONFLICT",
            "工作区库存在预览期间变化，全部输入未应用",
        ));
    }
    plan.continuation = continuation::witness(project, buffer, &edit, &candidate, &plan)?;
    digest(&mut plan)?;
    Ok((candidate, plan))
}

fn metadata_matches(edit: &writer::Edit, actual: &DialogueStatement) -> bool {
    match &edit.original {
        Some(original) => {
            original.localization_id == actual.localization_id
                && if actual.kind == DialogueKind::Text {
                    original.glue == actual.glue && original.tags == actual.tags
                } else {
                    !actual.glue && actual.tags.is_empty()
                }
        }
        None => actual.localization_id.is_none() && !actual.glue && actual.tags.is_empty(),
    }
}
fn changes(
    project: &Project,
    candidate: &Project,
    buffer: &WritingBuffer,
    plan: &mut DialogueEditPlan,
) -> Result<()> {
    for (path, document) in &candidate.documents {
        let before = project.document(path).ok();
        if before != Some(document.text.as_str()) {
            plan.changes.push(WritingAuthoringChange {
                path: path.clone(),
                before: before.map(str::to_owned),
                after: document.text.clone(),
                includes_unapplied_draft: path == buffer.path() && buffer.is_changed(),
            });
        }
    }
    for (path, document) in &candidate.authoring_documents {
        let before = project.authoring_document(path).ok().map(|d| d.bytes());
        if before != Some(document.bytes()) {
            plan.changes.push(WritingAuthoringChange {
                path: path.clone(),
                before: before.map(utf8).transpose()?,
                after: utf8(document.bytes())?,
                includes_unapplied_draft: false,
            });
        }
    }
    Ok(())
}
fn check_change_budget(plan: &DialogueEditPlan) -> Result<()> {
    let bytes = plan.changes.iter().fold(0usize, |n, c| {
        n.saturating_add(c.before.as_ref().map_or(0, String::len))
            .saturating_add(c.after.len())
    });
    if bytes > MAX_DIALOGUE_CHANGE_BYTES {
        Err(DialogueError::new(
            "BUDGET_EXCEEDED",
            "台词完整变更预览超过 8 MiB，未应用",
        ))
    } else {
        Ok(())
    }
}
fn digest(plan: &mut DialogueEditPlan) -> Result<()> {
    plan.plan_digest = crate::presentation_commands::document_hash(
        &serde_json::to_vec(plan)
            .map_err(|e| DialogueError::new("INVALID_REQUEST", e.to_string()))?,
    );
    Ok(())
}
fn utf8(bytes: &[u8]) -> Result<String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| DialogueError::new("SOURCE_UNAVAILABLE", "作者文档不是有效 UTF-8"))
}
fn relative(project: &Project, path: &std::path::Path) -> Result<PathBuf> {
    path.strip_prefix(&project.root)
        .map(std::path::Path::to_owned)
        .map_err(|_| DialogueError::new("SOURCE_UNAVAILABLE", "台词来源越出工作区"))
}

pub(super) fn workspace(project: &Project) -> Result<String> {
    let inventory = crate::source_lifecycle::safety::inventory(project).map_err(|e| {
        DialogueError::new(
            if e.message.contains("预算") {
                "BUDGET_EXCEEDED"
            } else {
                "EXTERNAL_CONFLICT"
            },
            e.message,
        )
    })?;
    project.source_lifecycle_guard(&inventory).map_err(|e| {
        DialogueError::new(
            if e.message.contains("预算") {
                "BUDGET_EXCEEDED"
            } else {
                "EXTERNAL_CONFLICT"
            },
            e.message,
        )
    })
}

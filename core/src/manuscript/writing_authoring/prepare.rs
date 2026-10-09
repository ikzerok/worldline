use super::*;
use crate::authoring_intents::{require_active_source, AuthoringIntent};
use crate::capabilities::CapabilityEnableRequest;
use crate::presentation_commands::document_hash;
use crate::LanguageVersion;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn prepare(
    project: &Project,
    buffers: &[WritingBuffer],
    request: &WritingAuthoringRequest,
) -> Result<(Project, WritingAuthoringPlan), String> {
    guard(project, buffers, request)?;
    let selection = &request.selection;
    require_active_source(project, &selection.path)?;
    let mut paths = BTreeSet::from([selection.path.clone()]);
    let target = match &request.target {
        IntentTarget::Existing(target) => target.clone(),
        IntentTarget::CreateCharacter { path, draft } => {
            paths.insert(require_active_source(project, path)?);
            TargetRef::new("character", &draft.id)
        }
        IntentTarget::CreateEntity { path, draft } => {
            paths.insert(require_active_source(project, path)?);
            TargetRef::new("entity", &draft.id)
        }
    };
    let selected = selected_buffers(project, buffers, &paths)?;
    let source = selected
        .get(&selection.path)
        .ok_or("正文缓冲不存在，请重新打开章节")?;
    if request.generation != source.generation()
        || selection.expected_text.is_empty()
        || source.source().get(selection.start..selection.end) != Some(&selection.expected_text)
    {
        return Err("正文选区或代次已过期，或选区不在 UTF-8 字符边界；输入已保留".into());
    }
    let link = crate::navigation::link_source(
        &target,
        &selection.expected_text,
        &selection.path.to_string_lossy(),
    )?;
    let preview_bytes = paths.iter().try_fold(0usize, |sum, path| {
        let before = project.document(path)?.len();
        let after = selected
            .get(path)
            .map_or(before, |buffer| buffer.source().len());
        Ok::<_, String>(sum.saturating_add(before).saturating_add(after))
    })?;
    if preview_bytes > MAX_WRITING_AUTHORING_CHANGE_BYTES {
        return Err("关联完整预览超过 8 MiB 预算，未编译或应用超量草稿".into());
    }
    let mut candidate = project.clone();
    for (path, buffer) in &selected {
        candidate.set_text(path, buffer.source().into())?;
    }
    let runtime_fingerprint_before = candidate.compile_current().analysis.fingerprint;
    let migration = if matches!(&request.target, IntentTarget::CreateEntity { .. })
        && !project.language_version_kind().supports_entities()
    {
        if !request.enable_entities {
            return Err("新建实体需要明确预览启用语言 1.10；工程尚未升级，输入已保留".into());
        }
        let migration = candidate.plan_capability_enable(&CapabilityEnableRequest {
            target_language: LanguageVersion::V1_10,
            enable_features: Vec::new(),
            expected_baseline: candidate.content_baseline(),
        })?;
        if migration.can_apply {
            candidate.apply_capability_enable(&migration)?;
        }
        Some(migration)
    } else {
        None
    };
    let mut plan = WritingAuthoringPlan {
        target,
        source: request.source.clone(),
        source_path: selection.path.clone(),
        cursor_utf8: selection.end,
        link_start_utf8: selection.start,
        changes: Vec::new(),
        included_buffers: selected
            .values()
            .map(|buffer| WritingAuthoringBuffer {
                path: buffer.path().to_owned(),
                generation: buffer.generation(),
                changed: buffer.is_changed(),
                source_hash: document_hash(buffer.source().as_bytes()),
                original_hash: document_hash(buffer.original().as_bytes()),
            })
            .collect(),
        migration,
        runtime_fingerprint_before,
        runtime_fingerprint_after: runtime_fingerprint_before,
        can_apply: false,
        plan_digest: String::new(),
        request: request.clone(),
        root: project.root.clone(),
        refresh_generation: project.search_refresh_generation(),
    };
    if plan
        .migration
        .as_ref()
        .is_some_and(|migration| !migration.can_apply)
    {
        finish_plan(project, &candidate, &selected, &mut plan)?;
        return Ok((candidate, plan));
    }
    let projection = candidate.project_writing_buffer(source, &request.source)?;
    if selection.start < projection.range.start || selection.end > projection.range.end {
        return Err("选区不属于当前章节正文来源，请重新选择正文文字".into());
    }
    let mut linked_source = source.source().to_owned();
    linked_source.replace_range(selection.start..selection.end, &link);
    let intent = AuthoringIntent {
        expected_baseline: candidate.content_baseline(),
        target: request.target.clone(),
        selection: Some(selection.clone()),
        placement: None,
    };
    candidate.apply_authoring_intent(&intent)?;
    let after = candidate.document(&selection.path)?;
    // 现有正式 metadata writer 只在同文件前置新声明；保持选区准确身份。
    if !after.ends_with(&linked_source) {
        return Err("新资料改变了无法证明的正文范围，关联草稿未应用".into());
    }
    let prefix = after.len() - linked_source.len();
    plan.link_start_utf8 = prefix + selection.start;
    plan.cursor_utf8 = plan.link_start_utf8 + link.len();
    plan.runtime_fingerprint_after = candidate.compile_current().analysis.fingerprint;
    plan.can_apply = true;
    finish_plan(project, &candidate, &selected, &mut plan)?;
    project.verify_review_navigation()?;
    Ok((candidate, plan))
}

fn guard(
    project: &Project,
    buffers: &[WritingBuffer],
    request: &WritingAuthoringRequest,
) -> Result<(), String> {
    if buffers.len() > 4096
        || serde_json::to_vec(request)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_WRITING_AUTHORING_REQUEST_BYTES
    {
        return Err("正文关联请求超过预算，输入未应用".into());
    }
    project.ensure_workspace_writable()?;
    if project.content_baseline() != request.expected_baseline {
        return Err("正文关联基线已过期，请保留草稿并重新预览".into());
    }
    if !project.recovery_conflicts().is_empty() {
        return Err("工程有未解决的保存事务冲突".into());
    }
    project.verify_review_navigation()?;
    super::super::commands::ensure_disk_matches_saved_baselines(project)
}

fn selected_buffers<'a>(
    project: &Project,
    buffers: &'a [WritingBuffer],
    paths: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, &'a WritingBuffer>, String> {
    let mut selected = BTreeMap::new();
    for path in paths {
        crate::file_access::within(&project.root, path)?;
        crate::source_lifecycle::safety::writable_path(path)?;
        for buffer in buffers.iter().filter(|buffer| buffer.path() == path) {
            if selected.insert(path.clone(), buffer).is_some() {
                return Err("同一文件出现多个正文缓冲，不能确认关联草稿".into());
            }
            if buffer.baseline() != project.content_baseline()
                || project.document(path)? != buffer.original()
            {
                return Err("关联文件的正文草稿基线已过期，输入已保留".into());
            }
        }
    }
    Ok(selected)
}

fn finish_plan(
    project: &Project,
    candidate: &Project,
    selected: &BTreeMap<PathBuf, &WritingBuffer>,
    plan: &mut WritingAuthoringPlan,
) -> Result<(), String> {
    for (path, document) in &candidate.documents {
        let before = project.document(path).ok();
        if before != Some(document.text.as_str()) {
            plan.changes.push(WritingAuthoringChange {
                path: path.clone(),
                before: before.map(str::to_owned),
                after: document.text.clone(),
                includes_unapplied_draft: selected
                    .get(path)
                    .is_some_and(|buffer| buffer.is_changed()),
            });
        }
    }
    for (path, document) in &candidate.authoring_documents {
        let before = project
            .authoring_document(path)
            .ok()
            .map(|document| document.bytes());
        if before != Some(document.bytes()) {
            plan.changes.push(WritingAuthoringChange {
                path: path.clone(),
                before: before.map(utf8).transpose()?,
                after: utf8(document.bytes())?,
                includes_unapplied_draft: false,
            });
        }
    }
    plan.changes
        .sort_by(|left, right| left.path.cmp(&right.path));
    let bytes = plan.changes.iter().fold(0usize, |sum, change| {
        sum.saturating_add(change.before.as_ref().map_or(0, String::len))
            .saturating_add(change.after.len())
    });
    if bytes > MAX_WRITING_AUTHORING_CHANGE_BYTES {
        return Err("关联完整预览超过 8 MiB 预算，工程未修改".into());
    }
    plan.plan_digest = document_hash(&serde_json::to_vec(plan).map_err(|error| error.to_string())?);
    Ok(())
}
fn utf8(bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| "关联文档不是有效 UTF-8".into())
}

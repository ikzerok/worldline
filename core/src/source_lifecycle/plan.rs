use super::*;
use crate::refactor::preview::{apply, Edit};
use std::path::Path;

pub(super) fn prepare(
    project: &Project,
    request: &SourceLifecycleRequest,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(Project, SourceLifecyclePlan), String> {
    check_cancelled(cancelled)?;
    project.ensure_workspace_writable()?;
    project.source_lifecycle_disk_baselines_match()?;
    safety::inventory(project)?;
    let before = project.compile_current();
    let mut candidate = project.clone();
    let (source_path, destination_path, membership, changes, resources) = match request {
        SourceLifecycleRequest::Create { path } => {
            safety::relative(path)?;
            let destination = candidate.add_file(path)?;
            (
                None,
                Some(destination.clone()),
                member(&candidate, &destination),
                diff(project, &candidate)?,
                Vec::new(),
            )
        }
        SourceLifecycleRequest::Include { path } => {
            safety::relative(path)?;
            let source = project.root.join(path);
            candidate.include_file(&source)?;
            (
                Some(source.clone()),
                None,
                member(project, &source),
                diff(project, &candidate)?,
                Vec::new(),
            )
        }
        SourceLifecycleRequest::Move { from, to } => {
            safety::relative(from)?;
            let old = crate::file_access::within(&project.root, &project.root.join(from))?;
            if old == project.entry {
                return Err("本版不支持移动工程入口；请保留入口路径".into());
            }
            project.document(&old)?;
            let new = safety::destination(project, to)?;
            if old == new {
                return Err("源码新旧路径相同".into());
            }
            if before.has_errors() {
                return Err("安全移动需要当前活动源码编译通过；新建/引用不受此限制".into());
            }
            let (mut changes, resources) = source::rewrite(project, &old, &new, cancelled)?;
            changes.extend(registered::rewrite(
                project,
                &old,
                &new,
                16384usize.saturating_sub(resources.len()),
            )?);
            for change in &changes {
                let bytes = change.after.as_ref().ok_or("移动候选缺少正文")?;
                if change.kind == "source" {
                    candidate.set_text(
                        &change.path,
                        String::from_utf8(bytes.clone()).map_err(|_| "候选源码不是 UTF-8")?,
                    )?;
                } else {
                    candidate.set_authoring_document(&change.path, bytes.clone())?;
                }
            }
            candidate.relocate_source_buffer(&old, &new)?;
            for resource in &resources {
                if resource.field == "asset.path"
                    && digest(&resource_bytes(&candidate, &resource.resolved_after)?)
                        != resource.content_digest
                {
                    return Err("移动会改变被当作原始附件的源码/展示文档字节，无法证明素材保持不变，工程未修改".into());
                }
            }
            // 清单新路径与缓冲移动同时在私有候选中完成，再检查正式语义。
            proof::equivalent(project, &candidate, &before, &old, &new)?;
            registered::validate_candidate(project, &candidate)?;
            let membership = member(project, &old);
            if membership != member(&candidate, &new) {
                return Err("移动改变了 active/archive 成员身份，工程未修改".into());
            }
            (Some(old), Some(new), membership, changes, resources)
        }
    };
    check_cancelled(cancelled)?;
    safety::buffer_budget(&candidate)?;
    safety::writable_paths(&changes)?;
    let after = candidate.compile_current();
    let mut plan = SourceLifecyclePlan {
        request: request.clone(),
        content_baseline: project.content_baseline(),
        plan_digest: String::new(),
        changes,
        source_path,
        destination_path,
        membership,
        runtime_fingerprint_before: before.analysis.fingerprint,
        runtime_fingerprint_after: after.analysis.fingerprint,
        entry_before: before.program.entry,
        entry_after: after.program.entry,
        load_order_before: before.program.files,
        load_order_after: after.program.files,
        resources,
    };
    let payload = serde_json::to_vec(&(&plan, candidate.content_baseline()))
        .map_err(|error| error.to_string())?;
    plan.plan_digest = digest(&payload);
    Ok((candidate, plan))
}

fn member(project: &Project, path: &Path) -> String {
    match project.source_selection() {
        None => "recursive",
        Some(selection) if selection.is_active(path) => "active",
        Some(selection) if selection.is_archived(path) => "archived",
        Some(_) => "inactive",
    }
    .into()
}

fn diff(before: &Project, after: &Project) -> Result<Vec<SourceLifecycleChange>, String> {
    let mut changes = Vec::new();
    for (path, document) in &after.documents {
        if document.is_deleted() {
            continue;
        }
        let old = before
            .documents
            .get(path)
            .filter(|document| !document.is_deleted())
            .map(|document| document.text.as_bytes());
        let new = document.text.as_bytes();
        if old == Some(new) {
            continue;
        }
        changes.push(change(path, "source", old, new)?);
    }
    for (path, document) in &after.authoring_documents {
        if document.is_deleted() {
            continue;
        }
        let old = before
            .authoring_documents
            .get(path)
            .filter(|document| !document.is_deleted())
            .map(|document| document.bytes());
        if old == Some(document.bytes()) {
            continue;
        }
        changes.push(change(path, "authoring", old, document.bytes())?);
    }
    Ok(changes)
}

fn change(
    path: &Path,
    kind: &str,
    before: Option<&[u8]>,
    after: &[u8],
) -> Result<SourceLifecycleChange, String> {
    let source = std::str::from_utf8(before.unwrap_or_default()).map_err(|_| "原文不是 UTF-8")?;
    let replacement = std::str::from_utf8(after).map_err(|_| "候选不是 UTF-8")?;
    let (_, occurrences) = apply(
        source,
        vec![Edit {
            range: 0..source.len(),
            replacement: replacement.into(),
            field: format!("{kind}.lifecycle"),
        }],
    )?;
    Ok(SourceLifecycleChange {
        path: path.into(),
        after_path: path.into(),
        kind: kind.into(),
        occurrences,
        before: before.map(<[u8]>::to_vec),
        after: Some(after.to_vec()),
    })
}

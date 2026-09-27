use super::proposal_merge::{current_text, merge_change, ProposalMerge};
use super::{
    absolute_path, build_proposal_index, preview_proposal, update_known_fields,
    ApplyProposalCommand, CollaborationResult, ProposalConflict, ProposalFileChange,
    ProposalResolution, ProposalStatus,
};
use crate::presentation_commands::Revision;
use crate::project::Project;
use crate::workspace_documents::parse_unique_json;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn set_json_pointer(root: &mut Value, pointer: &str, value: Option<Value>) -> Result<(), String> {
    if pointer.is_empty() {
        *root = value.ok_or("JSON 根冲突不能删除整个值")?;
        return Ok(());
    }
    let tokens = pointer
        .strip_prefix('/')
        .ok_or("提案冲突位置不是 JSON Pointer")?
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>();
    let (last, parents) = tokens
        .split_last()
        .ok_or("提案冲突位置不是有效 JSON Pointer")?;
    let mut parent = root;
    for token in parents {
        parent = parent
            .as_object_mut()
            .and_then(|object| object.get_mut(token))
            .ok_or("提案冲突的父字段不存在或路径经过数组")?;
    }
    let object = parent
        .as_object_mut()
        .ok_or("数组冲突必须作为完整 JSON 值解决")?;
    if let Some(value) = value {
        object.insert(last.clone(), value);
    } else {
        object.remove(last);
    }
    Ok(())
}

fn collect_proposal_resolutions(
    conflicts: &[ProposalConflict],
    resolutions: &[ProposalResolution],
) -> Result<BTreeMap<(String, String), Option<String>>, String> {
    let mut expected = BTreeSet::new();
    for conflict in conflicts {
        if !expected.insert((conflict.path.clone(), conflict.location.clone())) {
            return Err("提案预览包含重复的冲突位置".into());
        }
    }
    let mut values = BTreeMap::new();
    for resolution in resolutions {
        let key = (resolution.path.clone(), resolution.location.clone());
        if !expected.contains(&key) {
            return Err("解决方案不对应当前提案冲突".into());
        }
        if values.insert(key, resolution.value.clone()).is_some() {
            return Err("同一提案冲突只能提交一项解决方案".into());
        }
    }
    let unresolved = conflicts
        .iter()
        .filter(|conflict| {
            !values.contains_key(&(conflict.path.clone(), conflict.location.clone()))
        })
        .map(|conflict| {
            format!(
                "{}{}：{}",
                conflict.path, conflict.location, conflict.message
            )
        })
        .collect::<Vec<_>>()
        .join("；");
    if !unresolved.is_empty() {
        return Err(format!("提案存在未解决的三方冲突：{unresolved}"));
    }
    Ok(values)
}

fn resolve_change_conflicts(
    change: &ProposalFileChange,
    mut merged: ProposalMerge,
    conflicts: &[ProposalConflict],
    resolutions: &mut BTreeMap<(String, String), Option<String>>,
) -> Result<Option<String>, String> {
    for conflict in conflicts {
        let key = (conflict.path.clone(), conflict.location.clone());
        let resolution = resolutions.remove(&key).ok_or("提案存在未解决的三方冲突")?;
        if !conflict.location.is_empty() {
            if !merged.structured {
                return Err("只有结构化展示冲突可以按 JSON Pointer 解决".into());
            }
            let text = merged.text.as_deref().ok_or("JSON 冲突所在文件已被删除")?;
            let mut document = parse_unique_json(text.as_bytes())
                .map_err(|error| format!("当前 JSON 合并结果无效：{error}"))?;
            let value = resolution
                .as_deref()
                .map(|text| parse_unique_json(text.as_bytes()))
                .transpose()
                .map_err(|error| format!("冲突解决值不是有效 JSON：{error}"))?;
            set_json_pointer(&mut document, &conflict.location, value)?;
            merged.text =
                Some(serde_json::to_string_pretty(&document).map_err(|error| error.to_string())?);
        } else if merged.structured {
            let text = resolution.ok_or("JSON 根冲突必须提供完整 JSON 值")?;
            let document = parse_unique_json(text.as_bytes())
                .map_err(|error| format!("冲突解决值不是有效 JSON：{error}"))?;
            merged.text =
                Some(serde_json::to_string_pretty(&document).map_err(|error| error.to_string())?);
        } else {
            if change.domain == "presentation" {
                if let Some(text) = &resolution {
                    parse_unique_json(text.as_bytes())
                        .map_err(|error| format!("冲突解决文档不是有效 JSON：{error}"))?;
                }
            }
            merged.text = resolution;
        }
    }
    Ok(merged.text)
}

/// Applies a proposal only when its fresh preview has no unresolved conflicts.
pub fn apply_proposal(
    project: &mut Project,
    revision: &mut Revision,
    command: ApplyProposalCommand,
) -> Result<CollaborationResult, String> {
    apply_proposal_with_resolutions(project, revision, command, &[])
}
/// Rechecks and atomically applies a complete set of explicit conflict decisions.
pub fn apply_proposal_with_resolutions(
    project: &mut Project,
    revision: &mut Revision,
    command: ApplyProposalCommand,
    resolutions: &[ProposalResolution],
) -> Result<CollaborationResult, String> {
    if command.expected_revision != *revision {
        return Err("StaleRevision：审阅开始后工程修订已变化，请重新预览".into());
    }
    if command.expected_baseline != project.content_baseline() {
        return Err("StaleBaseline：审阅预览后内容已变化，请重新预览".into());
    }
    let index = build_proposal_index(project);
    if index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
    {
        return Err("提案索引含错误，请先修复后再采纳".into());
    }
    let proposal = index
        .proposals
        .get(&command.proposal_id)
        .ok_or("提案不存在")?;
    if proposal.read_only {
        return Err("提案文档为只读，不能采纳".into());
    }
    if proposal.draft.status != ProposalStatus::Open {
        return Err("提案已经结束，不能重复采纳".into());
    }
    let preview = preview_proposal(project, &proposal.draft)?;
    let mut resolution_values = collect_proposal_resolutions(&preview.conflicts, resolutions)?;
    if preview.files.len() != proposal.draft.changes.len() {
        return Err("提案预览文件与原提案不一致，请重新比较".into());
    }

    let mut candidate = project.clone();
    let mut changed_files = Vec::new();
    let mut touched_content = false;
    for (change, file_preview) in proposal.draft.changes.iter().zip(&preview.files) {
        if file_preview.path != change.path {
            return Err("提案预览文件与原提案不一致，请重新比较".into());
        }
        let (current, tracked_kind) = current_text(project, &change.path)?;
        let Some(authoring) = tracked_kind else {
            return Err(format!("提案目标未被当前 Project 跟踪：{}", change.path));
        };
        let merge = merge_change(change, current.as_deref())?;
        if merge.conflicts != file_preview.conflicts {
            return Err("提案预览已过期，请重新比较".into());
        }
        let merged = resolve_change_conflicts(
            change,
            merge,
            &file_preview.conflicts,
            &mut resolution_values,
        )?;
        let path = absolute_path(project, &change.path)?;
        match merged {
            Some(text) if authoring => {
                candidate.set_authoring_document(&path, text.into_bytes())?;
            }
            Some(text) => {
                candidate.set_text(&path, text)?;
                touched_content = true;
            }
            None => {
                candidate.delete_document(&path)?;
                touched_content |= !authoring;
            }
        }
        changed_files.push(path);
    }

    if !resolution_values.is_empty() {
        return Err("存在未应用的提案解决方案".into());
    }

    if touched_content && candidate.compile().has_errors() {
        return Err("提案采纳后的内容未通过编译检查，未写入任何文件".into());
    }

    let mut accepted = proposal.draft.clone();
    accepted.status = ProposalStatus::Accepted;
    let mut proposal_source = proposal.source.clone();
    update_known_fields(
        &mut proposal_source,
        &serde_json::to_value(&accepted).map_err(|error| error.to_string())?,
    )?;
    candidate.set_authoring_document(
        &proposal.path,
        serde_json::to_vec_pretty(&proposal_source).map_err(|error| error.to_string())?,
    )?;
    changed_files.push(proposal.path.clone());

    *project = candidate;
    *revision = revision.next_presentation();
    Ok(CollaborationResult {
        changed_files,
        new_revision: *revision,
    })
}

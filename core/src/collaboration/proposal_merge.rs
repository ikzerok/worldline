use super::proposal::validate_proposal;
use super::review::{proposal_reference_impacts, review_differences};
use super::{
    absolute_path, pointer_child, ProposalConflict, ProposalDraft, ProposalFileChange,
    ProposalFilePreview, ProposalPreview,
};
use crate::project::Project;
use crate::workspace_documents::parse_unique_json;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

fn conflict(
    path: &str,
    location: impl Into<String>,
    message: impl Into<String>,
) -> ProposalConflict {
    ProposalConflict {
        path: path.into(),
        location: location.into(),
        message: message.into(),
    }
}

fn merge_json_option(
    path: &str,
    pointer: &str,
    base: Option<&Value>,
    current: Option<&Value>,
    proposed: Option<&Value>,
) -> (Option<Value>, Vec<ProposalConflict>) {
    if current == base {
        return (proposed.cloned(), Vec::new());
    }
    if proposed == base || current == proposed {
        return (current.cloned(), Vec::new());
    }
    match (base, current, proposed) {
        (
            Some(Value::Object(base)),
            Some(Value::Object(current)),
            Some(Value::Object(proposed)),
        ) => {
            let keys = base
                .keys()
                .chain(current.keys())
                .chain(proposed.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            let mut merged = Map::new();
            let mut conflicts = Vec::new();
            for key in keys {
                let (value, mut nested) = merge_json_option(
                    path,
                    &pointer_child(pointer, &key),
                    base.get(&key),
                    current.get(&key),
                    proposed.get(&key),
                );
                if let Some(value) = value {
                    merged.insert(key, value);
                }
                conflicts.append(&mut nested);
            }
            (Some(Value::Object(merged)), conflicts)
        }
        (Some(Value::Array(_)), Some(Value::Array(_)), Some(Value::Array(_))) => (
            current.cloned(),
            vec![conflict(
                path,
                pointer,
                "数组被双方并行修改；顺序和删除语义不能自动合并",
            )],
        ),
        (Some(_), None, Some(_)) | (Some(_), Some(_), None) => (
            current.cloned(),
            vec![conflict(path, pointer, "删除与修改并发冲突")],
        ),
        (None, Some(_), Some(_)) => (
            current.cloned(),
            vec![conflict(path, pointer, "双方新增了不同值")],
        ),
        _ => (
            current.cloned(),
            vec![conflict(path, pointer, "同一字段被双方修改为不同值")],
        ),
    }
}

pub(super) fn current_text(
    project: &Project,
    relative: &str,
) -> Result<(Option<String>, Option<bool>), String> {
    let path = absolute_path(project, relative)?;
    let Some(state) = project.tracked_file_state(&path) else {
        return Ok((None, None));
    };
    let current = state
        .current
        .map(String::from_utf8)
        .transpose()
        .map_err(|_| format!("当前文件不是 UTF-8：{relative}"))?;
    Ok((current, Some(state.authoring)))
}

pub(super) struct ProposalMerge {
    pub(super) text: Option<String>,
    pub(super) conflicts: Vec<ProposalConflict>,
    pub(super) structured: bool,
}

pub(super) fn merge_change(
    change: &ProposalFileChange,
    current: Option<&str>,
) -> Result<ProposalMerge, String> {
    if current == change.base.as_deref() {
        return Ok(ProposalMerge {
            text: change.proposed.clone(),
            conflicts: Vec::new(),
            structured: false,
        });
    }
    if change.proposed.as_deref() == change.base.as_deref() || current == change.proposed.as_deref()
    {
        return Ok(ProposalMerge {
            text: current.map(str::to_owned),
            conflicts: Vec::new(),
            structured: false,
        });
    }
    if change.domain == "presentation" {
        if let (Some(base), Some(current), Some(proposed)) =
            (change.base.as_deref(), current, change.proposed.as_deref())
        {
            let base_json = parse_unique_json(base.as_bytes());
            let current_json = parse_unique_json(current.as_bytes());
            let proposed_json = parse_unique_json(proposed.as_bytes());
            if let (Ok(base_json), Ok(current_json), Ok(proposed_json)) =
                (base_json, current_json, proposed_json)
            {
                let (merged, conflicts) = merge_json_option(
                    &change.path,
                    "",
                    Some(&base_json),
                    Some(&current_json),
                    Some(&proposed_json),
                );
                return Ok(ProposalMerge {
                    text: merged
                        .map(|value| serde_json::to_string_pretty(&value))
                        .transpose()
                        .map_err(|error| error.to_string())?,
                    conflicts,
                    structured: true,
                });
            }
        }
    }
    let message = if change.domain == "content" {
        "正文文件基线与当前稿均已变化；为避免文学内容误合并，需要人工比对"
    } else if current.is_none() || change.proposed.is_none() {
        "展示文档发生删除/修改冲突，需要人工决定"
    } else {
        "展示文档无法进行安全结构化三方合并"
    };
    Ok(ProposalMerge {
        text: current.map(str::to_owned),
        conflicts: vec![conflict(&change.path, "", message)],
        structured: false,
    })
}

pub fn preview_proposal(
    project: &Project,
    proposal: &ProposalDraft,
) -> Result<ProposalPreview, String> {
    validate_proposal(project, proposal)?;
    let mut files = Vec::new();
    let mut all_conflicts = Vec::new();
    let reference_impacts_by_file = proposal_reference_impacts(project, proposal);
    for change in &proposal.changes {
        let (current, tracked_kind) = current_text(project, &change.path)?;
        let merge = merge_change(change, current.as_deref())?;
        let mut conflicts = merge.conflicts;
        if tracked_kind.is_none()
            && current.is_none()
            && change.base.is_none()
            && change.proposed.is_some()
        {
            conflicts.push(conflict(
                &change.path,
                "",
                "新文件尚未由当前工作区注册，不能自动接管",
            ));
        }
        let changed = merge.text != current;
        let (differences, truncated, alignment_uncertain, raw) =
            review_differences(change, current.as_deref());
        let semantic_changed = !differences.is_empty();
        let (reference_impacts, reference_impact_complete) = reference_impacts_by_file
            .get(&change.path)
            .cloned()
            .unwrap_or_else(|| (Vec::new(), change.domain != "content"));
        all_conflicts.extend(conflicts.iter().cloned());
        files.push(ProposalFilePreview {
            path: change.path.clone(),
            domain: change.domain.clone(),
            changed,
            semantic_changed,
            alignment_uncertain,
            conflicts,
            differences,
            truncated,
            raw,
            reference_impacts,
            reference_impact_complete,
        });
    }
    Ok(ProposalPreview {
        proposal_id: proposal.id.clone(),
        expected_baseline: project.content_baseline(),
        files,
        conflicts: all_conflicts,
    })
}

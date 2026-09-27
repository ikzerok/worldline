use super::{
    absolute_path, collaboration_bookkeeping_path, register_new_document, relative_path, report,
    update_known_fields, CollaborationResult, ProposalCommand, ProposalDocument, ProposalDraft,
    ProposalFileChange, ProposalIndex, ProposalStatus,
};
use crate::presentation_commands::Revision;
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, valid_id};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn capture_dirty_proposal(
    project: &Project,
    id: impl Into<String>,
    author: impl Into<String>,
    reason: impl Into<String>,
) -> Result<ProposalDraft, String> {
    let mut changes = Vec::new();
    for state in project.dirty_tracked_files() {
        let relative = relative_path(project, &state.path)?;
        if collaboration_bookkeeping_path(&relative) {
            continue;
        }
        let base = state
            .baseline
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| format!("提案基线不是 UTF-8：{relative}"))?;
        let proposed = state
            .current
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| format!("提案内容不是 UTF-8：{relative}"))?;
        if base == proposed {
            continue;
        }
        changes.push(ProposalFileChange {
            path: relative,
            domain: if state.authoring {
                "presentation".into()
            } else {
                "content".into()
            },
            base,
            proposed,
        });
    }
    if changes.is_empty() {
        return Err("当前没有可纳入提案的未保存内容或展示修改".into());
    }
    Ok(ProposalDraft {
        id: id.into(),
        author: author.into(),
        reason: reason.into(),
        status: ProposalStatus::Open,
        changes,
    })
}

pub(super) fn validate_proposal(project: &Project, draft: &ProposalDraft) -> Result<(), String> {
    if !valid_id(&draft.id)
        || draft.author.trim().is_empty()
        || draft.reason.trim().is_empty()
        || draft.changes.is_empty()
    {
        return Err("提案需要有效 ID、作者、理由和至少一个文件修改".into());
    }
    let mut paths = BTreeSet::new();
    for change in &draft.changes {
        absolute_path(project, &change.path)?;
        if collaboration_bookkeeping_path(&change.path) {
            return Err("提案不能修改协作注册清单或协作文档自身".into());
        }
        if !matches!(change.domain.as_str(), "content" | "presentation") {
            return Err("提案文件 domain 只能是 content 或 presentation".into());
        }
        if !paths.insert(change.path.clone()) {
            return Err(format!("提案包含重复文件：{}", change.path));
        }
        if change.base == change.proposed {
            return Err(format!("提案文件没有实际变化：{}", change.path));
        }
    }
    Ok(())
}

pub fn build_proposal_index(project: &Project) -> ProposalIndex {
    let mut index = ProposalIndex::default();
    let manifest = manifest_path(&project.root);
    let Ok(document) = project.authoring_document(&manifest) else {
        return index;
    };
    if document.is_deleted() {
        return index;
    }
    let registry = parse_registry(&project.root, document.bytes());
    index.diagnostics.extend(registry.diagnostics);
    for (id, path) in registry.proposals {
        let parsed = (|| -> Result<(ProposalDraft, Value, bool), String> {
            let document = project.authoring_document(&path)?;
            if document.is_deleted() {
                return Err("注册的提案文档已删除".into());
            }
            let source = parse_unique_json(document.bytes())
                .map_err(|error| format!("提案 JSON 无法解析：{error}"))?;
            if source.get("schema_version").and_then(Value::as_u64) != Some(1) {
                return Err("提案 schema_version 不受支持".into());
            }
            let draft: ProposalDraft = serde_json::from_value(source.clone())
                .map_err(|error| format!("提案结构无效：{error}"))?;
            if draft.id != id {
                return Err("提案 ID 与清单注册 ID 不一致".into());
            }
            validate_proposal(project, &draft)?;
            Ok((draft, source, document.is_read_only()))
        })();
        match parsed {
            Ok((draft, source, read_only)) => {
                index.proposals.insert(
                    id,
                    ProposalDocument {
                        draft,
                        path,
                        source,
                        read_only,
                    },
                );
            }
            Err(error) => report(&mut index.diagnostics, &path, "COLLAB002", error),
        }
    }
    crate::sort_diagnostics(&mut index.diagnostics);
    index
}

pub fn write_proposal(
    project: &mut Project,
    revision: &mut Revision,
    command: ProposalCommand,
) -> Result<CollaborationResult, String> {
    if command.expected_revision != *revision
        || command.expected_baseline != project.content_baseline()
    {
        return Err("StaleRevision：提案创建基线已过期".into());
    }
    validate_proposal(project, &command.draft)?;
    if command.draft.status != ProposalStatus::Open {
        return Err("新建提案状态必须为 open".into());
    }
    let index = build_proposal_index(project);
    if index.proposals.contains_key(&command.draft.id) {
        return Err("提案 ID 已存在".into());
    }
    let path = project
        .root
        .join(format!(".world/proposals/{}.json", command.draft.id));
    let mut source = json!({"schema_version":1});
    update_known_fields(
        &mut source,
        &serde_json::to_value(&command.draft).map_err(|error| error.to_string())?,
    )?;
    let bytes = serde_json::to_vec_pretty(&source).map_err(|error| error.to_string())?;
    let mut candidate = project.clone();
    let changed_files = register_new_document(
        project,
        &mut candidate,
        "proposals",
        "collaboration.proposals.v1",
        &command.draft.id,
        &path,
        bytes,
    )?;
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(CollaborationResult {
        changed_files,
        new_revision: *revision,
    })
}

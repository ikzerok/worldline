//! M4 协作：持久批注、提案基线与保守三方合并。
//!
//! 本模块只操作 Project 已跟踪的源码/展示文档。自动合并仅在证据充分时发生：
//! JSON 对象按稳定键递归合并；数组和正文的并发改写一律显式冲突。
use crate::catalog::TargetRef;
use crate::presentation_commands::Revision;
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_unique_json};
use crate::{Diagnostic, Span};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod comments;
mod proposal;
mod proposal_apply;
mod proposal_merge;
mod review;
mod review_projection;

pub use comments::{
    anchor_status as comment_anchor_status, build_comment_index, capture_text_anchor, write_comment,
};
pub use proposal::{build_proposal_index, capture_dirty_proposal, write_proposal};
pub use proposal_apply::{apply_proposal, apply_proposal_with_resolutions};
pub use proposal_merge::preview_proposal;
pub(crate) use review::review_checkpoint_text;
pub use review_projection::{
    capture_text_selection, CommentAnchorFilter, CommentResolutionFilter, CommentReviewFilter,
    CommentReviewItem, CommentReviewProjection,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommentAnchor {
    Object {
        target: TargetRef,
    },
    MapPlacement {
        map_id: String,
        placement_id: String,
    },
    TextRange {
        path: String,
        start_line: u32,
        end_line: u32,
        baseline_hash: String,
        quote: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentDraft {
    pub id: String,
    pub author: String,
    pub body: String,
    pub anchor: CommentAnchor,
    #[serde(default)]
    pub resolved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStatus {
    Attached,
    Detached,
}

#[derive(Clone, Debug)]
pub struct CommentDocument {
    pub draft: CommentDraft,
    pub path: PathBuf,
    pub source: Value,
    pub read_only: bool,
    pub anchor_status: AnchorStatus,
}

#[derive(Clone, Debug, Default)]
pub struct CommentIndex {
    pub comments: BTreeMap<String, CommentDocument>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommentReference {
    pub comment_id: String,
    pub file: String,
    pub anchor: String,
}

#[derive(Clone, Debug)]
pub struct CommentCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub original: Option<String>,
    pub draft: CommentDraft,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    #[default]
    Open,
    Accepted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalFileChange {
    pub path: String,
    pub domain: String,
    pub base: Option<String>,
    pub proposed: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalDraft {
    pub id: String,
    pub author: String,
    pub reason: String,
    #[serde(default)]
    pub status: ProposalStatus,
    pub changes: Vec<ProposalFileChange>,
}

#[derive(Clone, Debug)]
pub struct ProposalDocument {
    pub draft: ProposalDraft,
    pub path: PathBuf,
    pub source: Value,
    pub read_only: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ProposalIndex {
    pub proposals: BTreeMap<String, ProposalDocument>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalConflict {
    pub path: String,
    pub location: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalFilePreview {
    pub path: String,
    pub domain: String,
    pub changed: bool,
    /// 仅表示作者语义值变化；JSON 空白和键次序变化不计入。
    pub semantic_changed: bool,
    /// 正文无法无歧义对齐时，differences 回退到整份三方原文。
    pub alignment_uncertain: bool,
    pub conflicts: Vec<ProposalConflict>,
    pub differences: Vec<ProposalDifference>,
    pub truncated: bool,
    pub raw: ProposalRawSources,
    pub reference_impacts: Vec<ProposalReferenceImpact>,
    pub reference_impact_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalReferenceImpact {
    pub target: TargetRef,
    pub current: Vec<ProposalReferenceLocation>,
    pub proposed: Vec<ProposalReferenceLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalReferenceLocation {
    pub source: TargetRef,
    pub kind: String,
    pub file: String,
    pub line: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalDifference {
    /// JSON Pointer；正文使用从 1 开始的段落路径 /paragraphs/N。
    pub path: String,
    pub base: Option<String>,
    pub current: Option<String>,
    pub proposed: Option<String>,
    /// 对正文段落给出精确 UTF-8 字节范围；JSON 字段回退到整份源文范围。
    pub base_range: Option<ProposalSourceRange>,
    pub current_range: Option<ProposalSourceRange>,
    pub proposed_range: Option<ProposalSourceRange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalSourceRange {
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalRawSources {
    pub base: Option<String>,
    pub current: Option<String>,
    pub proposed: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposalPreview {
    pub proposal_id: String,
    pub expected_baseline: String,
    pub files: Vec<ProposalFilePreview>,
    pub conflicts: Vec<ProposalConflict>,
}

impl ProposalPreview {
    pub fn can_apply(&self) -> bool {
        self.conflicts.is_empty()
    }

    pub fn content_files(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.domain == "content" && file.changed)
            .count()
    }

    pub fn presentation_files(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.domain == "presentation" && file.changed)
            .count()
    }
}

#[derive(Clone, Debug)]
pub struct ProposalCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub draft: ProposalDraft,
}

#[derive(Clone, Debug)]
pub struct ApplyProposalCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    pub proposal_id: String,
}

/// A resolver's explicit value for one core-reported proposal conflict.
/// `None` deletes a JSON object member or an entire file-level conflict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalResolution {
    pub path: String,
    pub location: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CollaborationResult {
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
}

fn report(
    diagnostics: &mut Vec<Diagnostic>,
    path: &Path,
    code: &'static str,
    message: impl Into<String>,
) {
    diagnostics.push(Diagnostic::error(
        code,
        &path.to_string_lossy(),
        Span::new(1, 1, 1),
        message,
    ));
}

fn relative_path(project: &Project, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(&project.root)
        .map_err(|_| format!("文件不在工作区内:{}", path.display()))?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn absolute_path(project: &Project, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains(':')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("提案文件路径必须是工作区内规范相对路径".into());
    }
    crate::file_access::within(&project.root, &project.root.join(relative))
}

fn update_known_fields(source: &mut Value, fresh: &Value) -> Result<(), String> {
    let target = source.as_object_mut().ok_or("协作文档顶层必须是对象")?;
    let fresh = fresh.as_object().ok_or("协作文档序列化无效")?;
    for (key, value) in fresh {
        target.insert(key.clone(), value.clone());
    }
    target.insert("schema_version".into(), json!(1));
    Ok(())
}

fn register_new_document(
    project: &Project,
    candidate: &mut Project,
    registry_key: &str,
    feature: &str,
    id: &str,
    path: &Path,
    bytes: Vec<u8>,
) -> Result<Vec<PathBuf>, String> {
    let manifest = manifest_path(&project.root);
    let manifest_document = project.authoring_document(&manifest)?;
    if manifest_document.is_deleted() || manifest_document.is_read_only() {
        return Err("协作文档需要可写的工作区清单".into());
    }
    let mut value = parse_unique_json(manifest_document.bytes())
        .map_err(|error| format!("工作区清单无法解析：{error}"))?;
    if value.get(registry_key).is_none() {
        value[registry_key] = json!({});
    }
    let registry_object = value[registry_key]
        .as_object_mut()
        .ok_or_else(|| format!("清单 {registry_key} 必须是对象"))?;
    if registry_object.contains_key(id) {
        return Err(format!("{registry_key} ID 已注册，不能覆盖"));
    }
    let relative = relative_path(project, path)?;
    registry_object.insert(id.into(), json!(relative));
    if value.get("required_features").is_none() {
        value["required_features"] = json!([]);
    }
    let features = value["required_features"]
        .as_array_mut()
        .ok_or("清单 required_features 必须是数组")?;
    if !features.iter().any(|item| item.as_str() == Some(feature)) {
        features.push(json!(feature));
    }
    candidate.set_authoring_document(
        &manifest,
        serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )?;
    candidate.create_authoring_document(path, bytes)?;
    Ok(vec![manifest, path.to_path_buf()])
}

fn collaboration_bookkeeping_path(relative: &str) -> bool {
    relative == ".world/project.json"
        || relative.starts_with(".world/comments/")
        || relative.starts_with(".world/proposals/")
}

fn pointer_child(pointer: &str, key: &str) -> String {
    let key = key.replace('~', "~0").replace('/', "~1");
    if pointer.is_empty() {
        format!("/{key}")
    } else {
        format!("{pointer}/{key}")
    }
}

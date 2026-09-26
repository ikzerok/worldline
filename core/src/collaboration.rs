//! M4 协作：持久批注、提案基线与保守三方合并。
//!
//! 本模块只操作 Project 已跟踪的源码/展示文档。自动合并仅在证据充分时发生：
//! JSON 对象按稳定键递归合并；数组和正文的并发改写一律显式冲突。
use crate::catalog::TargetRef;
use crate::presentation::MapIndex;
use crate::presentation_commands::{document_hash, Revision};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, valid_id};
use crate::{CompileResult, Diagnostic, Span};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

fn selected_lines(text: &str, start_line: u32, end_line: u32) -> Option<String> {
    if start_line == 0 || end_line < start_line {
        return None;
    }
    let lines = text.lines().collect::<Vec<_>>();
    let start = usize::try_from(start_line - 1).ok()?;
    let end = usize::try_from(end_line).ok()?;
    if start >= lines.len() || end > lines.len() {
        return None;
    }
    Some(lines[start..end].join("\n"))
}

pub fn capture_text_anchor(
    project: &Project,
    path: &Path,
    start_line: u32,
    end_line: u32,
) -> Result<CommentAnchor, String> {
    let text = project.document(path)?;
    let quote =
        selected_lines(text, start_line, end_line).ok_or("正文批注范围无效，请重新选择行号")?;
    Ok(CommentAnchor::TextRange {
        path: relative_path(project, path)?,
        start_line,
        end_line,
        baseline_hash: document_hash(quote.as_bytes()),
        quote,
    })
}

fn anchor_status(
    project: &Project,
    content: &CompileResult,
    maps: &MapIndex,
    anchor: &CommentAnchor,
) -> AnchorStatus {
    let attached = match anchor {
        CommentAnchor::Object { target } => content.analysis.catalog.object(target).is_some(),
        CommentAnchor::MapPlacement {
            map_id,
            placement_id,
        } => maps
            .maps
            .get(map_id)
            .is_some_and(|map| map.placements.contains_key(placement_id)),
        CommentAnchor::TextRange {
            path,
            start_line,
            end_line,
            baseline_hash,
            quote,
        } => absolute_path(project, path)
            .ok()
            .and_then(|path| project.document(&path).ok())
            .and_then(|text| selected_lines(text, *start_line, *end_line))
            .is_some_and(|current| {
                &current == quote && document_hash(current.as_bytes()) == *baseline_hash
            }),
    };
    if attached {
        AnchorStatus::Attached
    } else {
        AnchorStatus::Detached
    }
}

fn validate_new_anchor(
    project: &Project,
    content: &CompileResult,
    maps: &MapIndex,
    anchor: &CommentAnchor,
) -> Result<(), String> {
    if anchor_status(project, content, maps, anchor) == AnchorStatus::Attached {
        Ok(())
    } else {
        Err("批注锚点当前无法解析；请重新选择对象、标记或正文范围".into())
    }
}

pub fn build_comment_index(
    project: &Project,
    content: &CompileResult,
    maps: &MapIndex,
) -> CommentIndex {
    let mut index = CommentIndex::default();
    let manifest = manifest_path(&project.root);
    let Ok(document) = project.authoring_document(&manifest) else {
        return index;
    };
    if document.is_deleted() {
        return index;
    }
    let registry = parse_registry(&project.root, document.bytes());
    index.diagnostics.extend(registry.diagnostics);
    for (id, path) in registry.comments {
        let parsed = (|| -> Result<(CommentDraft, Value, bool), String> {
            let document = project.authoring_document(&path)?;
            if document.is_deleted() {
                return Err("注册的批注文档已删除".into());
            }
            let source = parse_unique_json(document.bytes())
                .map_err(|error| format!("批注 JSON 无法解析：{error}"))?;
            if source.get("schema_version").and_then(Value::as_u64) != Some(1) {
                return Err("批注 schema_version 不受支持".into());
            }
            let draft: CommentDraft = serde_json::from_value(source.clone())
                .map_err(|error| format!("批注结构无效：{error}"))?;
            if draft.id != id {
                return Err("批注 ID 与清单注册 ID 不一致".into());
            }
            Ok((draft, source, document.is_read_only()))
        })();
        match parsed {
            Ok((draft, source, read_only)) => {
                let anchor_status = anchor_status(project, content, maps, &draft.anchor);
                index.comments.insert(
                    id,
                    CommentDocument {
                        draft,
                        path,
                        source,
                        read_only,
                        anchor_status,
                    },
                );
            }
            Err(error) => report(&mut index.diagnostics, &path, "COLLAB001", error),
        }
    }
    crate::sort_diagnostics(&mut index.diagnostics);
    index
}

impl CommentIndex {
    pub fn references_to(&self, target: &TargetRef) -> Vec<CommentReference> {
        self.comments
            .values()
            .filter_map(|comment| match &comment.draft.anchor {
                CommentAnchor::Object { target: anchor } if anchor == target => {
                    Some(CommentReference {
                        comment_id: comment.draft.id.clone(),
                        file: comment.path.to_string_lossy().into_owned(),
                        anchor: "object".into(),
                    })
                }
                _ => None,
            })
            .collect()
    }
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

pub fn write_comment(
    project: &mut Project,
    revision: &mut Revision,
    command: CommentCommand,
) -> Result<CollaborationResult, String> {
    if command.expected_revision != *revision
        || command.expected_baseline != project.content_baseline()
    {
        return Err("StaleRevision：批注基线已过期，请重新检查后提交".into());
    }
    if !valid_id(&command.draft.id)
        || command.draft.author.trim().is_empty()
        || command.draft.body.trim().is_empty()
    {
        return Err("批注需要有效 ID、作者和正文".into());
    }
    if command
        .original
        .as_deref()
        .is_some_and(|id| id != command.draft.id)
    {
        return Err("批注 ID 是稳定身份，不能在编辑时改名".into());
    }
    let content = project.compile();
    let maps = crate::presentation_commands::map_index_with_content(project, &content);
    validate_new_anchor(project, &content, &maps, &command.draft.anchor)?;
    let index = build_comment_index(project, &content, &maps);
    if index
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
    {
        return Err("批注索引含错误，请先修复后再写入".into());
    }
    let old = command
        .original
        .as_deref()
        .and_then(|id| index.comments.get(id));
    if command.original.is_some() && old.is_none() {
        return Err("待编辑批注不存在".into());
    }
    if old.is_some_and(|comment| comment.read_only) {
        return Err("批注文档为只读，不能覆盖".into());
    }
    if command.original.is_none() && index.comments.contains_key(&command.draft.id) {
        return Err("批注 ID 已存在".into());
    }
    let path = old.map(|comment| comment.path.clone()).unwrap_or_else(|| {
        project
            .root
            .join(format!(".world/comments/{}.json", command.draft.id))
    });
    let mut source = old
        .map(|comment| comment.source.clone())
        .unwrap_or_else(|| json!({"schema_version":1}));
    update_known_fields(
        &mut source,
        &serde_json::to_value(&command.draft).map_err(|error| error.to_string())?,
    )?;
    let bytes = serde_json::to_vec_pretty(&source).map_err(|error| error.to_string())?;
    let mut candidate = project.clone();
    let changed_files = if old.is_some() {
        candidate.set_authoring_document(&path, bytes)?;
        vec![path]
    } else {
        register_new_document(
            project,
            &mut candidate,
            "comments",
            "collaboration.comments.v1",
            &command.draft.id,
            &path,
            bytes,
        )?
    };
    *project = candidate;
    *revision = revision.next_presentation();
    Ok(CollaborationResult {
        changed_files,
        new_revision: *revision,
    })
}

fn collaboration_bookkeeping_path(relative: &str) -> bool {
    relative == ".world/project.json"
        || relative.starts_with(".world/comments/")
        || relative.starts_with(".world/proposals/")
}

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

fn validate_proposal(project: &Project, draft: &ProposalDraft) -> Result<(), String> {
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

fn pointer_child(pointer: &str, key: &str) -> String {
    let key = key.replace('~', "~0").replace('/', "~1");
    if pointer.is_empty() {
        format!("/{key}")
    } else {
        format!("{pointer}/{key}")
    }
}

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

const MAX_REVIEW_DIFFERENCES: usize = 256;
const MAX_REVIEW_TEXT_BYTES: usize = 16 * 1024;

fn review_text(value: &str, truncated: &mut bool) -> String {
    if value.len() <= MAX_REVIEW_TEXT_BYTES {
        return value.to_owned();
    }
    *truncated = true;
    let mut end = MAX_REVIEW_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn review_value(value: Option<&Value>, truncated: &mut bool) -> Option<String> {
    let value = value?;
    let mut writer = ReviewValueWriter::default();
    if serde_json::to_writer(&mut writer, value).is_err() {
        *truncated = true;
    }
    let valid_len = std::str::from_utf8(&writer.bytes)
        .map_or_else(|error| error.valid_up_to(), |_| writer.bytes.len());
    Some(String::from_utf8(writer.bytes[..valid_len].to_vec()).unwrap())
}

#[derive(Default)]
struct ReviewValueWriter {
    bytes: Vec<u8>,
}

impl Write for ReviewValueWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = MAX_REVIEW_TEXT_BYTES - self.bytes.len();
        let written = bytes.len().min(remaining);
        self.bytes.extend_from_slice(&bytes[..written]);
        if written < bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "proposal review text limit reached",
            ));
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn paragraphs_with_ranges(source: &str) -> Vec<(&str, ProposalSourceRange)> {
    if source.is_empty() {
        return vec![(
            "",
            ProposalSourceRange {
                start_byte: 0,
                end_byte: 0,
            },
        )];
    }

    let bytes = source.as_bytes();
    let mut paragraphs = Vec::new();
    let mut paragraph_start = 0;
    let mut line_start = 0;
    let mut has_text = false;
    for (newline, byte) in bytes.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let content_end = if newline > line_start && bytes[newline - 1] == b'\r' {
            newline - 1
        } else {
            newline
        };
        let blank_line = source[line_start..content_end].trim().is_empty();
        let line_end = newline + 1;
        if blank_line {
            if has_text {
                paragraphs.push((
                    &source[paragraph_start..line_end],
                    ProposalSourceRange {
                        start_byte: paragraph_start,
                        end_byte: line_end,
                    },
                ));
                paragraph_start = line_end;
                has_text = false;
            } else if let Some((text, range)) = paragraphs.last_mut() {
                range.end_byte = line_end;
                *text = &source[range.start_byte..line_end];
                paragraph_start = line_end;
            } else {
                paragraph_start = line_end;
            }
        } else {
            has_text = true;
        }
        line_start = line_end;
    }

    if line_start < bytes.len() {
        has_text |= !source[line_start..].trim().is_empty();
    }
    if paragraph_start < bytes.len() {
        if has_text || paragraphs.is_empty() {
            paragraphs.push((
                &source[paragraph_start..],
                ProposalSourceRange {
                    start_byte: paragraph_start,
                    end_byte: bytes.len(),
                },
            ));
        } else if let Some((text, range)) = paragraphs.last_mut() {
            range.end_byte = bytes.len();
            *text = &source[range.start_byte..];
        }
    }
    if paragraphs.is_empty() {
        paragraphs.push((
            source,
            ProposalSourceRange {
                start_byte: 0,
                end_byte: bytes.len(),
            },
        ));
    }
    paragraphs
}

#[derive(Clone, Debug)]
struct ParagraphAlignment {
    /// 对应每个 base 段落的 current/proposed 段落索引；None 表示删除。
    matched: Vec<Option<usize>>,
    /// 以 base 段落边界为键的插入段落范围。
    inserted: Vec<Option<std::ops::Range<usize>>>,
}

fn paragraph_identity_alignment(count: usize) -> ParagraphAlignment {
    ParagraphAlignment {
        matched: (0..count).map(Some).collect(),
        inserted: vec![None; count + 1],
    }
}

fn paragraph_unique_positions<'a>(
    paragraphs: &[(&'a str, ProposalSourceRange)],
) -> BTreeMap<&'a str, Option<usize>> {
    let mut positions = BTreeMap::new();
    for (index, (text, _)) in paragraphs.iter().enumerate() {
        positions
            .entry(*text)
            .and_modify(|position| *position = None)
            .or_insert(Some(index));
    }
    positions
}

fn paragraph_gap_is_unambiguous(
    base: &[(&str, ProposalSourceRange)],
    changed: &[(&str, ProposalSourceRange)],
) -> bool {
    let mut seen = BTreeSet::new();
    base.iter()
        .chain(changed)
        .all(|(text, _)| seen.insert(*text))
}

fn align_paragraphs(
    base: &[(&str, ProposalSourceRange)],
    changed: &[(&str, ProposalSourceRange)],
) -> Option<ParagraphAlignment> {
    if base.len() == changed.len()
        && base
            .iter()
            .zip(changed)
            .all(|((left, _), (right, _))| left == right)
    {
        return Some(paragraph_identity_alignment(base.len()));
    }

    let base_positions = paragraph_unique_positions(base);
    let changed_positions = paragraph_unique_positions(changed);
    let mut anchors = Vec::new();
    let mut last_changed = None;
    for (base_index, (text, _)) in base.iter().enumerate() {
        let (Some(Some(unique_base)), Some(Some(unique_changed))) =
            (base_positions.get(text), changed_positions.get(text))
        else {
            continue;
        };
        if *unique_base != base_index
            || last_changed.is_some_and(|previous| *unique_changed <= previous)
        {
            return None;
        }
        anchors.push((base_index, *unique_changed));
        last_changed = Some(*unique_changed);
    }

    let mut alignment = ParagraphAlignment {
        matched: vec![None; base.len()],
        inserted: vec![None; base.len() + 1],
    };
    let mut base_start = 0;
    let mut changed_start = 0;
    for (base_end, changed_end) in anchors
        .into_iter()
        .chain(std::iter::once((base.len(), changed.len())))
    {
        let base_gap = &base[base_start..base_end];
        let changed_gap = &changed[changed_start..changed_end];
        match (base_gap.len(), changed_gap.len()) {
            (0, 0) => {}
            (0, _) => {
                alignment.inserted[base_start] = Some(changed_start..changed_end);
            }
            (_, 0) => {}
            (base_count, changed_count)
                if base_count == changed_count
                    && paragraph_gap_is_unambiguous(base_gap, changed_gap) =>
            {
                for offset in 0..base_count {
                    alignment.matched[base_start + offset] = Some(changed_start + offset);
                }
            }
            _ => return None,
        }

        if base_end < base.len() {
            alignment.matched[base_end] = Some(changed_end);
            base_start = base_end + 1;
            changed_start = changed_end + 1;
        }
    }
    Some(alignment)
}

fn paragraph_span<'a>(
    source: &'a str,
    paragraphs: &[(&str, ProposalSourceRange)],
    span: std::ops::Range<usize>,
) -> Option<(&'a str, ProposalSourceRange)> {
    if span.is_empty() {
        return None;
    }
    let start_byte = paragraphs.get(span.start)?.1.start_byte;
    let end_byte = paragraphs.get(span.end - 1)?.1.end_byte;
    Some((
        &source[start_byte..end_byte],
        ProposalSourceRange {
            start_byte,
            end_byte,
        },
    ))
}

fn push_review_difference(
    result: &mut Vec<ProposalDifference>,
    truncated: &mut bool,
    path: String,
    base: Option<String>,
    current: Option<String>,
    proposed: Option<String>,
    ranges: [Option<ProposalSourceRange>; 3],
) {
    if result.len() == MAX_REVIEW_DIFFERENCES {
        *truncated = true;
    } else if result.len() < MAX_REVIEW_DIFFERENCES {
        let [base_range, current_range, proposed_range] = ranges;
        result.push(ProposalDifference {
            path,
            base,
            current,
            proposed,
            base_range,
            current_range,
            proposed_range,
        });
    }
}

fn json_review_differences(
    path: &str,
    base: Option<&Value>,
    current: Option<&Value>,
    proposed: Option<&Value>,
    result: &mut Vec<ProposalDifference>,
    truncated: &mut bool,
    ranges: &[Option<ProposalSourceRange>; 3],
) {
    if base == current && current == proposed {
        return;
    }
    if result.len() >= MAX_REVIEW_DIFFERENCES {
        *truncated = true;
        return;
    }
    if let (
        Some(Value::Object(base)),
        Some(Value::Object(current)),
        Some(Value::Object(proposed)),
    ) = (base, current, proposed)
    {
        let mut base_keys = base.keys().peekable();
        let mut current_keys = current.keys().peekable();
        let mut proposed_keys = proposed.keys().peekable();
        while let Some(key) = [base_keys.peek(), current_keys.peek(), proposed_keys.peek()]
            .into_iter()
            .flatten()
            .min()
            .copied()
        {
            if base_keys.peek().copied() == Some(key) {
                base_keys.next();
            }
            if current_keys.peek().copied() == Some(key) {
                current_keys.next();
            }
            if proposed_keys.peek().copied() == Some(key) {
                proposed_keys.next();
            }
            let base_value = base.get(key);
            let current_value = current.get(key);
            let proposed_value = proposed.get(key);
            if base_value == current_value && current_value == proposed_value {
                continue;
            }
            json_review_differences(
                &pointer_child(path, key),
                base_value,
                current_value,
                proposed_value,
                result,
                truncated,
                ranges,
            );
            if result.len() == MAX_REVIEW_DIFFERENCES && *truncated {
                return;
            }
        }
    } else {
        let base = review_value(base, truncated);
        let current = review_value(current, truncated);
        let proposed = review_value(proposed, truncated);
        push_review_difference(
            result,
            truncated,
            path.to_owned(),
            base,
            current,
            proposed,
            ranges.clone(),
        );
    }
}

fn review_differences(
    change: &ProposalFileChange,
    current: Option<&str>,
) -> (Vec<ProposalDifference>, bool, bool, ProposalRawSources) {
    let mut result = Vec::new();
    let mut truncated = false;
    let raw = ProposalRawSources {
        base: change
            .base
            .as_deref()
            .map(|text| review_text(text, &mut truncated)),
        current: current.map(|text| review_text(text, &mut truncated)),
        proposed: change
            .proposed
            .as_deref()
            .map(|text| review_text(text, &mut truncated)),
    };
    let full_ranges = [change.base.as_deref(), current, change.proposed.as_deref()].map(|source| {
        source.map(|source| ProposalSourceRange {
            start_byte: 0,
            end_byte: source.len(),
        })
    });
    if change.domain == "presentation" {
        let parsed = [change.base.as_deref(), current, change.proposed.as_deref()]
            .map(|source| source.and_then(|source| parse_unique_json(source.as_bytes()).ok()));
        if let [Some(base), Some(current), Some(proposed)] = &parsed {
            json_review_differences(
                "",
                Some(base),
                Some(current),
                Some(proposed),
                &mut result,
                &mut truncated,
                &full_ranges,
            );
            return (result, truncated, false, raw);
        }
    } else if change.domain == "content" {
        if let (Some(base_text), Some(current_text), Some(proposed_text)) =
            (change.base.as_deref(), current, change.proposed.as_deref())
        {
            let base_paragraphs = paragraphs_with_ranges(base_text);
            let current_paragraphs = paragraphs_with_ranges(current_text);
            let proposed_paragraphs = paragraphs_with_ranges(proposed_text);
            let current_alignment = if base_text == current_text {
                Some(paragraph_identity_alignment(base_paragraphs.len()))
            } else {
                align_paragraphs(&base_paragraphs, &current_paragraphs)
            };
            let proposed_alignment = if base_text == proposed_text {
                Some(paragraph_identity_alignment(base_paragraphs.len()))
            } else {
                align_paragraphs(&base_paragraphs, &proposed_paragraphs)
            };
            if let (Some(current_alignment), Some(proposed_alignment)) =
                (current_alignment, proposed_alignment)
            {
                for boundary in 0..=base_paragraphs.len() {
                    let current_insertion = current_alignment.inserted[boundary]
                        .clone()
                        .and_then(|span| paragraph_span(current_text, &current_paragraphs, span));
                    let proposed_insertion = proposed_alignment.inserted[boundary]
                        .clone()
                        .and_then(|span| paragraph_span(proposed_text, &proposed_paragraphs, span));
                    let insertion_values = [None, current_insertion, proposed_insertion];
                    let insertion_texts = insertion_values
                        .each_ref()
                        .map(|item| item.as_ref().map(|(text, _)| *text));
                    if insertion_texts[0] != insertion_texts[1]
                        || insertion_texts[1] != insertion_texts[2]
                    {
                        let insertion_ranges = insertion_values
                            .each_ref()
                            .map(|item| item.as_ref().map(|(_, range)| range.clone()));
                        let insertion_texts = insertion_values
                            .map(|item| item.map(|(text, _)| review_text(text, &mut truncated)));
                        push_review_difference(
                            &mut result,
                            &mut truncated,
                            format!("/paragraphs/{}", boundary + 1),
                            insertion_texts[0].clone(),
                            insertion_texts[1].clone(),
                            insertion_texts[2].clone(),
                            insertion_ranges,
                        );
                    }

                    if boundary == base_paragraphs.len() {
                        continue;
                    }
                    let base_item = &base_paragraphs[boundary];
                    let current_item = current_alignment.matched[boundary]
                        .and_then(|index| current_paragraphs.get(index));
                    let proposed_item = proposed_alignment.matched[boundary]
                        .and_then(|index| proposed_paragraphs.get(index));
                    let paragraph_items = [Some(base_item), current_item, proposed_item];
                    let paragraph_texts = paragraph_items
                        .each_ref()
                        .map(|item| item.map(|(text, _)| *text));
                    if paragraph_texts[0] == paragraph_texts[1]
                        && paragraph_texts[1] == paragraph_texts[2]
                    {
                        continue;
                    }
                    let paragraph_ranges = paragraph_items
                        .each_ref()
                        .map(|item| item.map(|(_, range)| range.clone()));
                    let paragraph_texts = paragraph_items
                        .map(|item| item.map(|(text, _)| review_text(text, &mut truncated)));
                    push_review_difference(
                        &mut result,
                        &mut truncated,
                        format!("/paragraphs/{}", boundary + 1),
                        paragraph_texts[0].clone(),
                        paragraph_texts[1].clone(),
                        paragraph_texts[2].clone(),
                        paragraph_ranges,
                    );
                }
                return (result, truncated, false, raw);
            }
        }
        let has_difference =
            change.base.as_deref() != current || current != change.proposed.as_deref();
        if has_difference {
            push_review_difference(
                &mut result,
                &mut truncated,
                "".into(),
                raw.base.clone(),
                raw.current.clone(),
                raw.proposed.clone(),
                full_ranges,
            );
        }
        return (result, truncated, true, raw);
    }
    if change.base.as_deref() != current || current != change.proposed.as_deref() {
        push_review_difference(
            &mut result,
            &mut truncated,
            "".into(),
            raw.base.clone(),
            raw.current.clone(),
            raw.proposed.clone(),
            full_ranges,
        );
    }
    (result, truncated, false, raw)
}

pub(crate) fn review_checkpoint_text(
    base: Option<&str>,
    current: Option<&str>,
    checkpoint: Option<&str>,
) -> (Vec<ProposalDifference>, bool, bool, ProposalRawSources) {
    let change = ProposalFileChange {
        path: String::new(),
        domain: "content".into(),
        base: base.map(str::to_owned),
        proposed: checkpoint.map(str::to_owned),
    };
    review_differences(&change, current)
}

type LimitedReferenceLocations = (Vec<ProposalReferenceLocation>, bool);

fn index_reference_locations(
    result: &CompileResult,
    targets: &BTreeSet<TargetRef>,
) -> BTreeMap<TargetRef, LimitedReferenceLocations> {
    let mut index = targets
        .iter()
        .cloned()
        .map(|target| (target, (Vec::new(), true)))
        .collect::<BTreeMap<_, _>>();
    for reference in &result.analysis.catalog.references {
        let Some((locations, complete)) = index.get_mut(&reference.target) else {
            continue;
        };
        if locations.len() == MAX_REVIEW_DIFFERENCES {
            *complete = false;
        } else {
            locations.push(ProposalReferenceLocation {
                source: reference.source.clone(),
                kind: reference.kind.clone(),
                file: reference.file.clone(),
                line: reference.line,
            });
        }
    }
    index
}

fn changed_file_targets(
    result: Option<&CompileResult>,
    path: &Path,
) -> (BTreeSet<TargetRef>, bool) {
    let mut targets = BTreeSet::new();
    let Some(result) = result else {
        return (targets, false);
    };
    let mut complete = true;
    for object in &result.analysis.catalog.objects {
        let object_file = Path::new(&object.file);
        if object_file != path {
            continue;
        }
        if targets.contains(&object.target) {
            continue;
        }
        if targets.len() == MAX_REVIEW_DIFFERENCES {
            complete = false;
        } else {
            targets.insert(object.target.clone());
        }
    }
    (targets, complete)
}

fn proposal_reference_impacts(
    project: &Project,
    proposal: &ProposalDraft,
) -> BTreeMap<String, (Vec<ProposalReferenceImpact>, bool)> {
    let content_changes = proposal
        .changes
        .iter()
        .filter(|change| change.domain == "content")
        .collect::<Vec<_>>();
    if content_changes.is_empty() {
        return BTreeMap::new();
    }

    // Compile one complete candidate for the whole proposal. Per-file candidates
    // can hide cross-file errors and omit reference edits made by sibling files.
    let current = project.compile_current();
    let mut candidate = project.clone();
    let mut candidate_edit_complete = true;
    for change in &content_changes {
        let Ok(path) = absolute_path(project, &change.path) else {
            candidate_edit_complete = false;
            break;
        };
        let edit = match &change.proposed {
            Some(text) => candidate.set_text(&path, text.clone()),
            None => candidate.delete_document(&path),
        };
        if edit.is_err() {
            candidate_edit_complete = false;
            break;
        }
    }
    let proposed = candidate_edit_complete.then(|| candidate.compile_current());
    let compile_complete = candidate_edit_complete
        && !current.has_errors()
        && proposed.as_ref().is_some_and(|result| !result.has_errors());

    let mut targets_by_file = BTreeMap::new();
    let mut all_targets = BTreeSet::new();
    for change in &content_changes {
        let Ok(path) = absolute_path(project, &change.path) else {
            targets_by_file.insert(change.path.clone(), (BTreeSet::new(), false));
            continue;
        };
        let (mut targets, targets_complete) = changed_file_targets(Some(&current), &path);
        let (proposed_targets, proposed_targets_complete) =
            changed_file_targets(proposed.as_ref(), &path);
        let mut complete = compile_complete && targets_complete && proposed_targets_complete;
        for target in proposed_targets {
            if targets.contains(&target) {
                continue;
            }
            if targets.len() == MAX_REVIEW_DIFFERENCES {
                complete = false;
            } else {
                targets.insert(target);
            }
        }
        all_targets.extend(targets.iter().cloned());
        targets_by_file.insert(change.path.clone(), (targets, complete));
    }

    // Visit each catalog once and retain at most one sentinel beyond the public
    // limit, so a high-fanout target cannot allocate an unbounded DTO buffer.
    let current_references = index_reference_locations(&current, &all_targets);
    let proposed_references = proposed
        .as_ref()
        .map(|result| index_reference_locations(result, &all_targets))
        .unwrap_or_default();

    content_changes
        .into_iter()
        .map(|change| {
            let (targets, mut complete) = targets_by_file.remove(&change.path).unwrap_or_default();
            let mut impacts = Vec::with_capacity(targets.len());
            for target in targets {
                let (mut current, current_complete) =
                    current_references.get(&target).cloned().unwrap_or_default();
                let (mut proposed, proposed_complete) = proposed_references
                    .get(&target)
                    .cloned()
                    .unwrap_or_default();
                complete &= current_complete && proposed_complete;
                current.truncate(MAX_REVIEW_DIFFERENCES);
                proposed.truncate(MAX_REVIEW_DIFFERENCES);
                impacts.push(ProposalReferenceImpact {
                    target,
                    current,
                    proposed,
                });
            }
            (change.path.clone(), (impacts, complete))
        })
        .collect()
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

fn current_text(
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

struct ProposalMerge {
    text: Option<String>,
    conflicts: Vec<ProposalConflict>,
    structured: bool,
}

fn merge_change(
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

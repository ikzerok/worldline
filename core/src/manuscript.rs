//! 书稿编排的解析、来源投影、分页与基线保护写入。
//!
//! Project 只读取清单明确注册的书稿，并通过统一展示文档缓冲提交修改；
//! 书稿不修改源码语义或运行指纹。

use crate::ast::{Stmt, TextPart};
use crate::catalog::TargetRef;
use crate::presentation_commands::Revision;
use crate::project::{AuthoringDocument, Project};
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json};
use crate::{CompileResult, Diagnostic, Span};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

pub const MANUSCRIPT_SCHEMA_VERSION: u64 = 1;
pub const MANUSCRIPT_REQUIRED_FEATURE: &str = "presentation.manuscripts.v1";
pub const MAX_MANUSCRIPT_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptEntryKind {
    Section,
    Chapter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptReferenceStatus {
    Resolved,
    Missing,
    Unresolved,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManuscriptReferenceRole {
    Source,
    Perspective,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptSourceLocation {
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ManuscriptTextStats {
    pub han_characters: u64,
    pub words: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptSource {
    pub status: ManuscriptReferenceStatus,
    pub location: Option<ManuscriptSourceLocation>,
    pub stats: Option<ManuscriptTextStats>,
}

/// 一个已识别的节点。未知字段留在 `ManuscriptIndex::source_document` 中，
/// 写入端必须对原文档做局部更新，不能将此投影整体写回。
#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptEntry {
    pub id: String,
    pub kind: ManuscriptEntryKind,
    pub parent_id: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub perspective: Option<TargetRef>,
    pub perspective_status: Option<ManuscriptReferenceStatus>,
    pub status: Option<String>,
    pub goal: Option<String>,
    pub target_ref: Option<TargetRef>,
    pub source: Option<ManuscriptSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptChapterProjection {
    pub id: String,
    pub title: String,
    /// 从根到直接父项的 section ID；不包含本章 ID。
    pub section_path: Vec<String>,
    pub summary: Option<String>,
    pub perspective: Option<TargetRef>,
    pub perspective_status: Option<ManuscriptReferenceStatus>,
    pub status: Option<String>,
    pub goal: Option<String>,
    pub target_ref: Option<TargetRef>,
    pub source: Option<ManuscriptSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManuscriptPage {
    pub manuscript_id: Option<String>,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub chapters: Vec<ManuscriptChapterProjection>,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManuscriptReference {
    pub file: String,
    pub manuscript_id: String,
    pub chapter_id: String,
    pub role: ManuscriptReferenceRole,
    pub target: TargetRef,
}

/// 只读索引。`source_bytes` 与 `source_document` 保留原文，包含未知字段；
/// 不支持的版本或调用方标记为只读时，不生成章节投影。
#[derive(Debug, Clone)]
pub struct ManuscriptIndex {
    pub id: Option<String>,
    pub title: Option<String>,
    pub entries: Vec<ManuscriptEntry>,
    pub diagnostics: Vec<Diagnostic>,
    pub read_only: bool,
    file: String,
    source_bytes: Vec<u8>,
    source_document: Option<Value>,
    chapter_order: Vec<(usize, Vec<String>)>,
}

#[derive(Debug, Clone)]
pub struct ManuscriptEntryDraft {
    pub id: String,
    pub kind: ManuscriptEntryKind,
    pub parent_id: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub pov: Option<TargetRef>,
    pub status: Option<String>,
    pub goal: Option<String>,
    pub target_ref: Option<TargetRef>,
}

#[derive(Debug, Clone)]
pub struct ManuscriptDraft {
    pub id: String,
    pub title: String,
    /// 数组顺序是相同父项下的阅读顺序。
    pub entries: Vec<ManuscriptEntryDraft>,
}

impl ManuscriptDraft {
    pub fn from_index(index: &ManuscriptIndex) -> Self {
        Self {
            id: index.id.clone().unwrap_or_default(),
            title: index.title.clone().unwrap_or_default(),
            entries: index
                .entries
                .iter()
                .map(|entry| ManuscriptEntryDraft {
                    id: entry.id.clone(),
                    kind: entry.kind,
                    parent_id: entry.parent_id.clone(),
                    title: entry.title.clone(),
                    summary: entry.summary.clone(),
                    pov: entry.perspective.clone(),
                    status: entry.status.clone(),
                    goal: entry.goal.clone(),
                    target_ref: entry.target_ref.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManuscriptCommand {
    pub expected_revision: Revision,
    pub expected_baseline: String,
    /// `None` 创建新书稿；已有书稿的稳定 ID 不能改名。
    pub original: Option<String>,
    pub draft: ManuscriptDraft,
}

#[derive(Debug, Clone)]
pub struct ManuscriptResult {
    pub changed_files: Vec<PathBuf>,
    pub new_revision: Revision,
}

impl ManuscriptIndex {
    pub fn source_bytes(&self) -> &[u8] {
        &self.source_bytes
    }

    pub fn source_document(&self) -> Option<&Value> {
        self.source_document.as_ref()
    }

    /// 以稳定的树前序返回章节页。游标只对当前文档和分析快照有效。
    pub fn page(&self, offset: usize, requested_limit: usize) -> ManuscriptPage {
        let total = self.chapter_order.len();
        let limit = requested_limit.clamp(1, MAX_MANUSCRIPT_PAGE_SIZE);
        let start = offset.min(total);
        let end = start.saturating_add(limit).min(total);
        let chapters = self.chapter_order[start..end]
            .iter()
            .filter_map(|(entry_index, section_path)| {
                let entry = self.entries.get(*entry_index)?;
                Some(ManuscriptChapterProjection {
                    id: entry.id.clone(),
                    title: entry.title.clone(),
                    section_path: section_path.clone(),
                    summary: entry.summary.clone(),
                    perspective: entry.perspective.clone(),
                    perspective_status: entry.perspective_status,
                    status: entry.status.clone(),
                    goal: entry.goal.clone(),
                    target_ref: entry.target_ref.clone(),
                    source: entry.source.clone(),
                })
            })
            .collect();
        ManuscriptPage {
            manuscript_id: self.id.clone(),
            offset: start,
            limit,
            total,
            chapters,
            next_offset: (end < total).then_some(end),
        }
    }

    /// 返回源目标和 POV 的反向引用，不读取或修改 Project。
    pub fn references_to(&self, target: &TargetRef) -> Vec<ManuscriptReference> {
        let Some(manuscript_id) = self.id.as_ref() else {
            return Vec::new();
        };
        let mut references = Vec::new();
        for entry in &self.entries {
            if entry.kind != ManuscriptEntryKind::Chapter {
                continue;
            }
            if let Some(reference) = entry.target_ref.as_ref().filter(|reference| {
                crate::deletion_content_references::affected_by_deletion(reference, target)
            }) {
                references.push(ManuscriptReference {
                    file: self.file.clone(),
                    manuscript_id: manuscript_id.clone(),
                    chapter_id: entry.id.clone(),
                    role: ManuscriptReferenceRole::Source,
                    target: reference.clone(),
                });
            }
            if entry.perspective.as_ref() == Some(target) {
                references.push(ManuscriptReference {
                    file: self.file.clone(),
                    manuscript_id: manuscript_id.clone(),
                    chapter_id: entry.id.clone(),
                    role: ManuscriptReferenceRole::Perspective,
                    target: target.clone(),
                });
            }
        }
        references
    }

    fn references_to_targets_with_status(
        &self,
    ) -> Vec<(ManuscriptReference, ManuscriptReferenceStatus)> {
        let Some(manuscript_id) = self.id.as_ref() else {
            return Vec::new();
        };
        let mut references = Vec::new();
        for entry in &self.entries {
            if entry.kind != ManuscriptEntryKind::Chapter {
                continue;
            }
            if let (Some(target), Some(source)) = (&entry.target_ref, &entry.source) {
                references.push((
                    ManuscriptReference {
                        file: self.file.clone(),
                        manuscript_id: manuscript_id.clone(),
                        chapter_id: entry.id.clone(),
                        role: ManuscriptReferenceRole::Source,
                        target: target.clone(),
                    },
                    source.status,
                ));
            }
            if let (Some(target), Some(status)) = (&entry.perspective, entry.perspective_status) {
                references.push((
                    ManuscriptReference {
                        file: self.file.clone(),
                        manuscript_id: manuscript_id.clone(),
                        chapter_id: entry.id.clone(),
                        role: ManuscriptReferenceRole::Perspective,
                        target: target.clone(),
                    },
                    status,
                ));
            }
        }
        references
    }
}

/// 从已注册文档的原始字节建立只读书稿索引。
///
/// `registered_read_only` 由清单能力协商和 Project 文档保护状态提供；
/// `registered_id` 是清单中的键。该函数不访问磁盘，也不修改 Project。
pub fn build_manuscript_index(
    bytes: &[u8],
    file: &str,
    registered_id: &str,
    manifest_required_features: &[String],
    registered_read_only: bool,
    content: &CompileResult,
) -> ManuscriptIndex {
    let mut index = ManuscriptIndex {
        id: None,
        title: None,
        entries: Vec::new(),
        diagnostics: Vec::new(),
        read_only: registered_read_only,
        file: file.to_string(),
        source_bytes: bytes.to_vec(),
        source_document: None,
        chapter_order: Vec::new(),
    };
    let document = match parse_unique_json(bytes) {
        Ok(document) => document,
        Err(error) => {
            index.error("MAN001", format!("书稿 JSON 无法解析：{error}"));
            return index;
        }
    };
    index.source_document = Some(document.clone());
    let Some(object) = document.as_object() else {
        index.error("MAN001", "书稿顶层必须是 JSON 对象");
        return index;
    };

    let Some(schema_version) = object.get("schema_version").and_then(Value::as_u64) else {
        index.error("MAN001", "书稿 schema_version 必须是整数");
        return index;
    };
    if schema_version != MANUSCRIPT_SCHEMA_VERSION {
        index.read_only = true;
        index.error(
            "MAN002",
            format!("书稿格式版本 {schema_version} 不受支持，按只读处理"),
        );
        return index;
    }
    if !manifest_required_features
        .iter()
        .any(|feature| feature == MANUSCRIPT_REQUIRED_FEATURE)
    {
        index.read_only = true;
        index.error(
            "MAN002",
            format!("清单未声明必需能力 `{MANUSCRIPT_REQUIRED_FEATURE}`，书稿按只读处理"),
        );
        return index;
    }
    if registered_read_only {
        index.error("MAN002", "书稿注册能力不受支持，按只读处理");
        return index;
    }

    let Some(id) = object.get("id").and_then(Value::as_str) else {
        index.error("MAN001", "书稿 id 必须是字符串");
        return index;
    };
    if !valid_id(id) {
        index.error("MAN001", "书稿 id 格式无效");
        return index;
    }
    index.id = Some(id.to_string());
    if id != registered_id {
        index.error(
            "MAN001",
            format!("书稿 id `{id}` 与清单注册 ID `{registered_id}` 不一致"),
        );
        return index;
    }
    let title = match object.get("title").and_then(Value::as_str) {
        Some(title) if !title.trim().is_empty() => title,
        _ => {
            index.error("MAN001", "书稿 title 必须是非空字符串");
            id
        }
    };
    index.title = Some(title.to_string());
    let Some(entries) = object.get("entries").and_then(Value::as_array) else {
        index.error("MAN001", "书稿 entries 必须是数组");
        return index;
    };

    let mut first_by_id = HashMap::new();
    for value in entries {
        let Some(node) = value.as_object() else {
            index.error("MAN001", "书稿 entries 中的每项必须是对象");
            continue;
        };
        let Some(entry_id) = node.get("id").and_then(Value::as_str) else {
            index.error("MAN001", "书稿节点 id 必须是字符串");
            continue;
        };
        if !valid_id(entry_id) {
            index.error("MAN001", format!("书稿节点 ID `{entry_id}` 格式无效"));
            continue;
        }
        let kind = match node.get("kind").and_then(Value::as_str) {
            Some("section") => ManuscriptEntryKind::Section,
            Some("chapter") => ManuscriptEntryKind::Chapter,
            _ => {
                index.error(
                    "MAN001",
                    format!("书稿节点 `{entry_id}` 的 kind 只允许 section 或 chapter"),
                );
                continue;
            }
        };
        let node_title = match node.get("title").and_then(Value::as_str) {
            Some(title) if !title.trim().is_empty() => title,
            _ => {
                index.error(
                    "MAN001",
                    format!("书稿节点 `{entry_id}` 的 title 必须是非空字符串"),
                );
                entry_id
            }
        };

        let parent_id = parse_optional_string(node, "parent_id", entry_id, &mut index);
        let summary = parse_optional_string(node, "summary", entry_id, &mut index);
        let status = parse_optional_string(node, "status", entry_id, &mut index);
        let goal = parse_optional_string(node, "goal", entry_id, &mut index);
        let perspective = parse_optional_target(node, "pov", entry_id, &mut index);
        let target_ref = parse_optional_target(node, "target_ref", entry_id, &mut index);

        match kind {
            ManuscriptEntryKind::Section if target_ref.is_some() => {
                index.error("MAN008", format!("section `{entry_id}` 不能引用正文目标"));
            }
            ManuscriptEntryKind::Section if node.contains_key("pov") => {
                index.error("MAN008", format!("section `{entry_id}` 不能声明 POV"));
            }
            ManuscriptEntryKind::Chapter if target_ref.is_none() => {
                index.error(
                    "MAN001",
                    format!("chapter `{entry_id}` 必须声明 target_ref"),
                );
            }
            _ => {}
        }

        if let Some(first) = first_by_id.get(entry_id).copied() {
            let _ = first;
            index.error("MAN005", format!("书稿节点 ID `{entry_id}` 重复"));
        } else {
            first_by_id.insert(entry_id.to_string(), index.entries.len());
        }
        index.entries.push(ManuscriptEntry {
            id: entry_id.to_string(),
            kind,
            parent_id,
            title: node_title.to_string(),
            summary,
            perspective,
            perspective_status: None,
            status,
            goal,
            target_ref,
            source: None,
        });
    }

    validate_hierarchy(&mut index);
    for entry_index in 0..index.entries.len() {
        if index.entries[entry_index].kind != ManuscriptEntryKind::Chapter {
            continue;
        }
        if let Some(target) = index.entries[entry_index].target_ref.clone() {
            let source = resolve_source(&target, content, &mut index);
            index.entries[entry_index].source = Some(source);
        }
        if let Some(perspective) = index.entries[entry_index].perspective.clone() {
            let status = resolve_perspective(&perspective, content, &mut index);
            index.entries[entry_index].perspective_status = Some(status);
        }
    }
    build_chapter_order(&mut index);
    crate::diagnostic::sort_diagnostics(&mut index.diagnostics);
    index
}

impl Project {
    /// 返回当前缓冲中所有清单注册书稿的只读索引；不会刷新工程或编译写入缓冲。
    pub fn manuscript_indices(&self) -> BTreeMap<String, ManuscriptIndex> {
        let manifest = manifest_path(&self.root);
        let Some(manifest_document) = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted())
        else {
            return BTreeMap::new();
        };
        let registry = parse_registry(&self.root, manifest_document.bytes());
        let required_features: Vec<_> = registry.required_features.iter().cloned().collect();
        let content = self.compile_current();
        registry
            .manuscripts
            .iter()
            .map(|(id, path)| {
                let document = self.authoring_documents.get(path);
                let bytes = document
                    .filter(|document| !document.is_deleted())
                    .map(AuthoringDocument::bytes)
                    .unwrap_or_default();
                let read_only = registry.read_only(path)
                    || document.is_some_and(AuthoringDocument::is_read_only);
                (
                    id.clone(),
                    build_manuscript_index(
                        bytes,
                        &path.to_string_lossy(),
                        id,
                        &required_features,
                        read_only,
                        &content,
                    ),
                )
            })
            .collect()
    }

    pub fn manuscript_index(&self, id: &str) -> Result<ManuscriptIndex, String> {
        self.manuscript_indices()
            .remove(id)
            .ok_or_else(|| format!("书稿 `{id}` 未在工作区清单中注册"))
    }

    /// 预览章节编辑。索引构建仅读取当前缓冲，Project 与磁盘均不改变。
    pub fn preview_manuscript(
        &self,
        revision: Revision,
        command: &ManuscriptCommand,
    ) -> Result<ManuscriptIndex, String> {
        self.prepare_manuscript(revision, command)
            .map(|(_, index, _)| index)
    }

    /// 在完整内容基线、修订与磁盘保存基线匹配后一次提交书稿和清单。
    pub fn apply_manuscript(
        &mut self,
        revision: &mut Revision,
        command: ManuscriptCommand,
    ) -> Result<ManuscriptResult, String> {
        let (candidate, _index, changed_files) = self.prepare_manuscript(*revision, &command)?;
        let new_revision = revision.next_presentation();
        *self = candidate;
        *revision = new_revision;
        Ok(ManuscriptResult {
            changed_files,
            new_revision,
        })
    }

    fn prepare_manuscript(
        &self,
        revision: Revision,
        command: &ManuscriptCommand,
    ) -> Result<(Project, ManuscriptIndex, Vec<PathBuf>), String> {
        if command.expected_revision != revision {
            return Err("StaleRevision：书稿编辑修订已过期，请保留输入并重新预览".into());
        }
        if command.expected_baseline != self.content_baseline() {
            return Err("StaleRevision：书稿编辑基线已过期，请保留输入并重新预览".into());
        }
        self.ensure_workspace_writable()?;
        if !self.recovery_conflicts().is_empty() {
            return Err("工程有未解决的保存事务冲突".into());
        }
        ensure_disk_matches_saved_baselines(self)?;
        validate_draft_shape(&command.draft)?;

        let manifest = manifest_path(&self.root);
        let existing_manifest = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted());
        if existing_manifest.is_some_and(AuthoringDocument::is_read_only) {
            return Err("工作区清单为只读，不能编辑书稿注册".into());
        }
        let (mut manifest_value, registry) = if let Some(document) = existing_manifest {
            let value = parse_unique_json(document.bytes())
                .map_err(|error| format!("工作区清单 JSON 无法安全读取：{error}"))?;
            let registry = parse_registry(&self.root, document.bytes());
            if !registry.diagnostics.is_empty() {
                return Err("工作区清单诊断未修复，不能编辑书稿".into());
            }
            (value, registry)
        } else {
            if command.original.is_some() {
                return Err("待编辑书稿未注册".into());
            }
            let entry = self
                .entry
                .strip_prefix(&self.root)
                .map_err(|_| "工程入口不在工作区内")?
                .to_string_lossy()
                .replace('\\', "/");
            (
                json!({
                    "schema_version": 1,
                    "language_version": self.language_version(),
                    "entry": entry,
                    "required_features": [],
                    "manuscripts": {}
                }),
                crate::workspace_documents::Registry::default(),
            )
        };
        let original_index = match command.original.as_deref() {
            Some(original) => {
                if original != command.draft.id {
                    return Err("书稿 ID 是稳定身份，不能在编辑中改名".into());
                }
                let path = registry
                    .manuscripts
                    .get(original)
                    .ok_or("待编辑书稿未注册")?;
                let index = self.manuscript_index(original)?;
                if index.read_only {
                    return Err("书稿为只读，不能写入".into());
                }
                if index.diagnostics.iter().any(|diagnostic| {
                    matches!(
                        diagnostic.code,
                        "MAN001" | "MAN005" | "MAN006" | "MAN007" | "MAN008"
                    )
                }) {
                    return Err("书稿结构诊断未修复，不能安全更新文档".into());
                }
                Some((path.clone(), index))
            }
            None => {
                if registry.manuscripts.contains_key(&command.draft.id) {
                    return Err("书稿 ID 已注册，不能覆盖".into());
                }
                None
            }
        };

        let path = if let Some((path, _)) = &original_index {
            path.clone()
        } else {
            self.root
                .join(".world")
                .join("manuscripts")
                .join(format!("{}.json", command.draft.id))
        };
        crate::file_access::within(&self.root, &path)?;
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "书稿路径不在工作区内")?
            .to_string_lossy()
            .replace('\\', "/");
        if original_index.is_none() {
            if registry.documents.contains_key(&path) {
                return Err("书稿目标路径已被其他展示文档注册".into());
            }
            if let Some(manuscripts) = manifest_value.get("manuscripts").and_then(Value::as_object)
            {
                if manuscripts
                    .values()
                    .any(|value| value.as_str() == Some(&relative))
                {
                    return Err("书稿目标路径已被其他书稿注册".into());
                }
            }
        }

        let mut source = match &original_index {
            Some((_, index)) => index
                .source_document()
                .cloned()
                .ok_or("原書稿 JSON 无法读取")?,
            None => json!({"schema_version": MANUSCRIPT_SCHEMA_VERSION}),
        };
        let object = source.as_object_mut().ok_or("书稿 JSON 顶层必须是对象")?;
        object.insert("schema_version".into(), json!(MANUSCRIPT_SCHEMA_VERSION));
        object.insert("id".into(), json!(command.draft.id));
        object.insert("title".into(), json!(command.draft.title));
        let previous_entries = object
            .get("entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let entries = merge_draft_entries(previous_entries, &command.draft.entries)?;
        object.insert("entries".into(), Value::Array(entries));
        let bytes = serde_json::to_vec_pretty(&source).map_err(|error| error.to_string())?;

        let mut required_features = match manifest_value.get("required_features") {
            None => Vec::new(),
            Some(Value::Array(features)) => features.clone(),
            Some(_) => return Err("工作区清单 required_features 必须是数组".into()),
        };
        let feature_added = !required_features
            .iter()
            .any(|feature| feature.as_str() == Some(MANUSCRIPT_REQUIRED_FEATURE));
        if feature_added {
            required_features.push(json!(MANUSCRIPT_REQUIRED_FEATURE));
        }
        manifest_value["required_features"] = Value::Array(required_features);
        if manifest_value.get("manuscripts").is_none() {
            manifest_value["manuscripts"] = json!({});
        }
        let manuscripts = manifest_value
            .get_mut("manuscripts")
            .and_then(Value::as_object_mut)
            .ok_or("工作区清单 manuscripts 必须是对象")?;
        if original_index.is_none() {
            manuscripts.insert(command.draft.id.clone(), json!(relative));
        }

        let content = self.compile_current();
        let mut candidate = self.clone();
        let mut changed_files = Vec::new();
        let manifest_changed =
            existing_manifest.is_none() || original_index.is_none() || feature_added;
        if manifest_changed {
            let manifest_bytes =
                serde_json::to_vec_pretty(&manifest_value).map_err(|error| error.to_string())?;
            if existing_manifest.is_some() {
                candidate.set_authoring_document(&manifest, manifest_bytes)?;
            } else {
                candidate.create_authoring_document(&manifest, manifest_bytes)?;
            }
            changed_files.push(manifest.clone());
        }
        if original_index.is_some() {
            candidate.set_authoring_document(&path, bytes)?;
        } else {
            candidate.create_authoring_document(&path, bytes)?;
        }
        changed_files.push(path);
        changed_files.sort();
        changed_files.dedup();

        let index = candidate.manuscript_index(&command.draft.id)?;
        validate_manuscript_candidate(&index, original_index.as_ref().map(|(_, index)| index))?;
        // 使用同一内容快照验证引用；书稿编辑本身不重新解释或改写事件语义。
        if content.sources != candidate.sources()
            || content.options.language_version != candidate.language_version_kind()
        {
            return Err("StaleContent：书稿编辑期间内容快照已变化".into());
        }
        Ok((candidate, index, changed_files))
    }
}

fn validate_draft_shape(draft: &ManuscriptDraft) -> Result<(), String> {
    if !valid_id(&draft.id) || draft.title.trim().is_empty() {
        return Err("书稿 ID 或标题无效".into());
    }
    let mut ids = HashSet::new();
    for entry in &draft.entries {
        if !valid_id(&entry.id) || !ids.insert(entry.id.as_str()) || entry.title.trim().is_empty() {
            return Err("书稿节点 ID 重复、格式无效或标题为空".into());
        }
        match entry.kind {
            ManuscriptEntryKind::Section if entry.target_ref.is_some() || entry.pov.is_some() => {
                return Err(format!("section `{}` 不能引用正文或声明 POV", entry.id));
            }
            ManuscriptEntryKind::Chapter if entry.target_ref.is_none() => {
                return Err(format!("chapter `{}` 必须引用正文目标", entry.id));
            }
            _ => {}
        }
    }
    Ok(())
}

fn merge_draft_entries(
    previous: Vec<Value>,
    drafts: &[ManuscriptEntryDraft],
) -> Result<Vec<Value>, String> {
    let mut previous_by_id = BTreeMap::new();
    for entry in previous {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .ok_or("原书稿含有无法识别的节点 ID")?
            .to_string();
        if previous_by_id.insert(id.clone(), entry).is_some() {
            return Err(format!("原书稿节点 ID `{id}` 重复，不能安全更新"));
        }
    }
    let mut entries = Vec::with_capacity(drafts.len());
    for draft in drafts {
        let mut value = previous_by_id
            .remove(&draft.id)
            .unwrap_or_else(|| json!({}));
        let object = value.as_object_mut().ok_or("原书稿节点必须是对象")?;
        object.insert("id".into(), json!(draft.id));
        object.insert(
            "kind".into(),
            json!(match draft.kind {
                ManuscriptEntryKind::Section => "section",
                ManuscriptEntryKind::Chapter => "chapter",
            }),
        );
        object.insert("title".into(), json!(draft.title));
        set_optional_field(object, "parent_id", draft.parent_id.as_deref());
        set_optional_field(object, "summary", draft.summary.as_deref());
        set_optional_field(object, "status", draft.status.as_deref());
        set_optional_field(object, "goal", draft.goal.as_deref());
        set_optional_target(object, "pov", draft.pov.as_ref());
        set_optional_target(object, "target_ref", draft.target_ref.as_ref());
        entries.push(value);
    }
    Ok(entries)
}

fn set_optional_field(object: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        object.insert(key.into(), json!(value));
    } else {
        object.remove(key);
    }
}

fn set_optional_target(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&TargetRef>,
) {
    if let Some(value) = value {
        if let Some(Value::Object(existing)) = object.get_mut(key) {
            existing.insert("kind".into(), json!(value.kind));
            existing.insert("id".into(), json!(value.id));
        } else {
            object.insert(key.into(), json!(value));
        }
    } else {
        object.remove(key);
    }
}

fn validate_manuscript_candidate(
    candidate: &ManuscriptIndex,
    previous: Option<&ManuscriptIndex>,
) -> Result<(), String> {
    if candidate.read_only {
        return Err("书稿能力或格式不受支持，只能只读查看".into());
    }
    if let Some(diagnostic) = candidate.diagnostics.iter().find(|diagnostic| {
        matches!(
            diagnostic.code,
            "MAN001" | "MAN002" | "MAN005" | "MAN006" | "MAN007"
        )
    }) {
        return Err(format!("{}：{}", diagnostic.code, diagnostic.message));
    }
    let old_references = previous
        .map(|index| index.references_to_targets_with_status())
        .unwrap_or_default();
    for (reference, status) in candidate.references_to_targets_with_status() {
        if status != ManuscriptReferenceStatus::Resolved
            && !old_references
                .iter()
                .any(|(old, old_status)| old == &reference && old_status == &status)
        {
            return Err(format!(
                "UnresolvedReference：不能新增未解析的书稿引用 {}:{}",
                reference.target.kind, reference.target.id
            ));
        }
    }
    Ok(())
}

fn ensure_disk_matches_saved_baselines(project: &Project) -> Result<(), String> {
    for path in project
        .documents
        .keys()
        .chain(project.authoring_documents.keys())
    {
        crate::file_access::within(&project.root, path)?;
        let state = project.tracked_file_state(path).ok_or("文档基线不存在")?;
        let disk = match crate::file_access::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("无法检查文档基线：{error}")),
        };
        if disk != state.baseline {
            return Err(format!(
                "文档已被外部修改，请保留草稿并刷新：{}",
                path.display()
            ));
        }
    }
    Ok(())
}

impl ManuscriptIndex {
    fn error(&mut self, code: &'static str, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(
            code,
            &self.file,
            Span::new(1, 1, 1),
            message,
        ));
    }
}

fn parse_optional_string(
    node: &serde_json::Map<String, Value>,
    field: &str,
    entry_id: &str,
    index: &mut ManuscriptIndex,
) -> Option<String> {
    match node.get(field) {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            index.error(
                "MAN001",
                format!("书稿节点 `{entry_id}` 的 {field} 必须是字符串"),
            );
            None
        }
    }
}

fn parse_optional_target(
    node: &serde_json::Map<String, Value>,
    field: &str,
    entry_id: &str,
    index: &mut ManuscriptIndex,
) -> Option<TargetRef> {
    match node.get(field) {
        None | Some(Value::Null) => None,
        Some(value) => match parse_target(value) {
            Some(target) => Some(target),
            None => {
                index.error(
                    "MAN001",
                    format!("书稿节点 `{entry_id}` 的 {field} 必须含非空 kind 与 id"),
                );
                None
            }
        },
    }
}

fn parse_target(value: &Value) -> Option<TargetRef> {
    let object = value.as_object()?;
    let kind = object.get("kind")?.as_str()?;
    let id = object.get("id")?.as_str()?;
    if kind.is_empty() || id.is_empty() {
        return None;
    }
    Some(TargetRef::new(kind, id))
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn validate_hierarchy(index: &mut ManuscriptIndex) {
    let mut first_by_id = HashMap::new();
    for (position, entry) in index.entries.iter().enumerate() {
        first_by_id.entry(entry.id.clone()).or_insert(position);
    }
    let mut parents = vec![None; index.entries.len()];
    for (position, parent_slot) in parents.iter_mut().enumerate() {
        let entry_id = index.entries[position].id.clone();
        let Some(parent_id) = index.entries[position].parent_id.clone() else {
            continue;
        };
        let Some(parent_index) = first_by_id.get(&parent_id).copied() else {
            index.error(
                "MAN006",
                format!("节点 `{entry_id}` 的父 section `{parent_id}` 不存在"),
            );
            continue;
        };
        if parent_index == position {
            index.error("MAN006", format!("节点 `{entry_id}` 不能成为自己的父项"));
            continue;
        }
        let parent_is_section = index.entries[parent_index].kind == ManuscriptEntryKind::Section;
        if !parent_is_section {
            index.error(
                "MAN006",
                format!("节点 `{entry_id}` 的父项 `{parent_id}` 必须是 section"),
            );
            continue;
        }
        *parent_slot = Some(parent_index);
    }

    let mut complete = vec![false; parents.len()];
    for start in 0..parents.len() {
        if complete[start] {
            continue;
        }
        let mut path: Vec<usize> = Vec::new();
        let mut positions: HashMap<usize, usize> = HashMap::new();
        let mut current: Option<usize> = Some(start);
        while let Some(node) = current {
            if complete[node] {
                break;
            }
            if positions.contains_key(&node) {
                if let Some(edge_source) = path.last().copied() {
                    parents[edge_source] = None;
                    index.error(
                        "MAN007",
                        format!(
                            "书稿层级在节点 `{}` 处形成循环",
                            index.entries[edge_source].id
                        ),
                    );
                }
                break;
            }
            positions.insert(node, path.len());
            path.push(node);
            current = parents[node];
        }
        for node in path {
            complete[node] = true;
        }
    }

    // 校验后的父索引由顺序投影使用；无效父项按根节点展示，避免隐藏章节。
    for (position, parent) in parents.into_iter().enumerate() {
        let parent_id = parent.map(|parent| index.entries[parent].id.clone());
        index.entries[position].parent_id = parent_id;
    }
}

fn build_chapter_order(index: &mut ManuscriptIndex) {
    let mut first_by_id = HashMap::new();
    for (position, entry) in index.entries.iter().enumerate() {
        first_by_id.entry(entry.id.clone()).or_insert(position);
    }
    let mut children = vec![Vec::new(); index.entries.len()];
    let mut roots = Vec::new();
    for (position, entry) in index.entries.iter().enumerate() {
        match entry
            .parent_id
            .as_ref()
            .and_then(|parent_id| first_by_id.get(parent_id).copied())
        {
            Some(parent) => children[parent].push(position),
            None => roots.push(position),
        }
    }

    fn walk(
        position: usize,
        entries: &[ManuscriptEntry],
        children: &[Vec<usize>],
        visited: &mut [bool],
        order: &mut Vec<(usize, Vec<String>)>,
    ) {
        let mut section_path = Vec::new();
        let mut stack = vec![(position, false)];
        while let Some((node, leaving)) = stack.pop() {
            if leaving {
                section_path.pop();
                continue;
            }
            if visited[node] {
                continue;
            }
            visited[node] = true;
            let entry = &entries[node];
            if entry.kind == ManuscriptEntryKind::Chapter {
                order.push((node, section_path.clone()));
            } else {
                section_path.push(entry.id.clone());
                stack.push((node, true));
            }
            for child in children[node].iter().rev() {
                stack.push((*child, false));
            }
        }
    }

    let mut visited = vec![false; index.entries.len()];
    for root in roots {
        walk(
            root,
            &index.entries,
            &children,
            &mut visited,
            &mut index.chapter_order,
        );
    }
    // 重复 ID 造成的歧义节点仍可见，按文档顺序追加；原始数组不被改写。
    for position in 0..index.entries.len() {
        if !visited[position] {
            walk(
                position,
                &index.entries,
                &children,
                &mut visited,
                &mut index.chapter_order,
            );
        }
    }
}

fn resolve_source(
    target: &TargetRef,
    content: &CompileResult,
    index: &mut ManuscriptIndex,
) -> ManuscriptSource {
    if !matches!(target.kind.as_str(), "event" | "scene" | "entity") {
        index.error(
            "MAN008",
            format!("书稿正文目标类型 `{}` 不受支持", target.kind),
        );
        return ManuscriptSource {
            status: ManuscriptReferenceStatus::Invalid,
            location: None,
            stats: None,
        };
    }
    if target.kind == "entity" && !content.options.language_version.supports_entities() {
        index.error(
            "MAN004",
            format!("当前语言版本无法确认实体 `{}`", target.id),
        );
        return ManuscriptSource {
            status: ManuscriptReferenceStatus::Unresolved,
            location: None,
            stats: None,
        };
    }
    let Some(object) = content.analysis.catalog.object(target) else {
        let status = missing_or_unresolved(content);
        report_missing_target(target, status, index);
        return ManuscriptSource {
            status,
            location: None,
            stats: None,
        };
    };
    let text = match target.kind.as_str() {
        "event" => content
            .program
            .events
            .iter()
            .find(|event| event.name == target.id)
            .map(|event| narrative_text(&event.body)),
        "scene" => content
            .analysis
            .symbols
            .scenes
            .get(&target.id)
            .and_then(|path| {
                content
                    .program
                    .events
                    .get(path.event)
                    .map(|event| (event, path))
            })
            .and_then(|(event, path)| scene_body(&event.body, &path.scenes))
            .map(narrative_text),
        "entity" => content
            .analysis
            .catalog
            .entities
            .get(&target.id)
            .map(|entity| entity.description.clone()),
        _ => None,
    };
    let Some(text) = text else {
        index.error(
            "MAN004",
            format!("正文目标 `{}` 的源码范围无法确认", target.id),
        );
        return ManuscriptSource {
            status: ManuscriptReferenceStatus::Unresolved,
            location: None,
            stats: None,
        };
    };
    ManuscriptSource {
        status: ManuscriptReferenceStatus::Resolved,
        location: Some(ManuscriptSourceLocation {
            file: object.file.clone(),
            line: object.line,
        }),
        stats: Some(text_stats(&text)),
    }
}

fn resolve_perspective(
    target: &TargetRef,
    content: &CompileResult,
    index: &mut ManuscriptIndex,
) -> ManuscriptReferenceStatus {
    if target.kind != "character" {
        index.error(
            "MAN008",
            format!("POV 目标类型 `{}` 必须是 character", target.kind),
        );
        return ManuscriptReferenceStatus::Invalid;
    }
    if content.analysis.catalog.object(target).is_some() {
        ManuscriptReferenceStatus::Resolved
    } else {
        let status = missing_or_unresolved(content);
        report_missing_target(target, status, index);
        status
    }
}

fn missing_or_unresolved(content: &CompileResult) -> ManuscriptReferenceStatus {
    if content.has_errors() {
        ManuscriptReferenceStatus::Unresolved
    } else {
        ManuscriptReferenceStatus::Missing
    }
}

fn report_missing_target(
    target: &TargetRef,
    status: ManuscriptReferenceStatus,
    index: &mut ManuscriptIndex,
) {
    let (code, wording) = match status {
        ManuscriptReferenceStatus::Unresolved => ("MAN004", "无法确认是否存在"),
        ManuscriptReferenceStatus::Missing => ("MAN003", "不存在"),
        _ => return,
    };
    index.error(
        code,
        format!("书稿引用的 {} `{}` {wording}", target.kind, target.id),
    );
}

fn scene_body<'a>(body: &'a [Stmt], names: &[String]) -> Option<&'a [Stmt]> {
    let (name, rest) = names.split_first()?;
    let scene = body.iter().find_map(|statement| match statement {
        Stmt::Scene(scene) if &scene.name == name => Some(scene),
        _ => None,
    })?;
    if rest.is_empty() {
        Some(&scene.body)
    } else {
        scene_body(&scene.body, rest)
    }
}

fn narrative_text(statements: &[Stmt]) -> String {
    fn append_parts(parts: &[TextPart], output: &mut String) {
        for part in parts {
            match part {
                TextPart::Str(text) => output.push_str(text),
                TextPart::Link(link) => output.push_str(&link.label),
                TextPart::Expr(_) => {}
            }
        }
    }
    fn append(statements: &[Stmt], output: &mut String) {
        for statement in statements {
            match statement {
                Stmt::Text(text) => {
                    append_parts(&text.parts, output);
                    if !text.glue {
                        output.push('\n');
                    }
                }
                Stmt::Choice(choice) => {
                    append_parts(&choice.label, output);
                    output.push('\n');
                    append(&choice.body, output);
                    output.push('\n');
                }
                Stmt::If(condition) => {
                    for (_, branch) in &condition.branches {
                        append(branch, output);
                        output.push('\n');
                    }
                }
                Stmt::Scene(scene) => append(&scene.body, output),
                // 声明、表达式、跳转、效果和其他控制语法不属于静态阅读文本。
                Stmt::Divert(_)
                | Stmt::Let(_)
                | Stmt::Set(_)
                | Stmt::Change(_)
                | Stmt::Anchor(_)
                | Stmt::Effect(_) => {}
            }
        }
    }
    let mut output = String::new();
    append(statements, &mut output);
    output
}

fn text_stats(text: &str) -> ManuscriptTextStats {
    let mut stats = ManuscriptTextStats::default();
    let mut in_non_han_word = false;
    for character in text.chars() {
        if is_han_ideograph(character) {
            stats.han_characters += 1;
            stats.words += 1;
            in_non_han_word = false;
        } else if character.is_alphanumeric() {
            if !in_non_han_word {
                stats.words += 1;
            }
            in_non_han_word = true;
        } else {
            in_non_han_word = false;
        }
    }
    stats
}

fn is_han_ideograph(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
            | 0x2EBF0..=0x2EE5F
            | 0x30000..=0x3134F
            | 0x31350..=0x323AF
    )
}

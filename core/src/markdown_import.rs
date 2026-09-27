//! 外部 Markdown 的安全预检和候选映射。
//!
//! 此模块只处理显式调用方选择的目录，不参与普通工作区扫描。

use crate::catalog::TargetRef;
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

mod apply;
mod inline;
mod mapping;
mod markdown;
mod planning;
mod source;

pub const MAX_IMPORT_FILES: usize = 512;
pub const MAX_MARKDOWN_BYTES_PER_PAGE: usize = 1024 * 1024;
pub const MAX_MARKDOWN_BYTES_TOTAL: usize = 16 * 1024 * 1024;
pub const MAX_ATTACHMENT_BYTES_TOTAL: usize = 64 * 1024 * 1024;
pub const MAX_IMPORT_REFERENCES: usize = 4096;
const MAX_IMPORT_ENTRIES: usize = MAX_IMPORT_FILES * 4;
const MAX_NAMESPACE_BYTES: usize = 64;
const MAX_IMPORT_PATH_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownImportRequest {
    pub source_root: PathBuf,
    pub expected_baseline: String,
    #[serde(default)]
    pub id_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub accept_losses: bool,
    #[serde(default)]
    pub allow_language_upgrade: bool,
}

/// 目录和 Files 快照两种来源共用的迁移选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkdownImportOptions {
    pub expected_baseline: String,
    #[serde(default)]
    pub id_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub accept_losses: bool,
    #[serde(default)]
    pub allow_language_upgrade: bool,
}

impl From<&MarkdownImportRequest> for MarkdownImportOptions {
    fn from(request: &MarkdownImportRequest) -> Self {
        Self {
            expected_baseline: request.expected_baseline.clone(),
            id_overrides: request.id_overrides.clone(),
            namespace: request.namespace.clone(),
            accept_losses: request.accept_losses,
            allow_language_upgrade: request.allow_language_upgrade,
        }
    }
}

/// 用户显式选择的普通文件快照。`label` 只用于预览显示，不会作为宿主路径访问。
#[derive(Debug, Clone, Copy)]
pub struct MarkdownImportSourceSnapshot<'a> {
    pub label: &'a str,
    pub files: &'a crate::workspace_snapshot::Files,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownPageMapping {
    pub source: String,
    pub id: String,
    pub title: String,
    pub entity_type: String,
    pub target: TargetRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownLinkMapping {
    pub source: String,
    pub line: u32,
    pub href: String,
    pub label: String,
    pub target: TargetRef,
    pub relation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownAttachmentMapping {
    pub source: String,
    pub line: u32,
    pub href: String,
    pub id: String,
    pub output_path: String,
    pub alt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportLoss {
    pub code: String,
    pub source: String,
    pub line: u32,
    pub message: String,
    pub preserved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportConflict {
    pub code: String,
    pub source: Option<String>,
    pub preferred_id: Option<String>,
    pub candidates: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownNameConflict {
    pub title: String,
    pub sources: Vec<String>,
    pub existing_targets: Vec<TargetRef>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportFilePreview {
    pub path: String,
    pub kind: String,
    pub source: Option<String>,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportPlan {
    pub source_root: PathBuf,
    pub source_fingerprint: String,
    pub plan_digest: String,
    pub baseline: String,
    pub new_baseline: String,
    pub namespace: String,
    pub pages: Vec<MarkdownPageMapping>,
    pub links: Vec<MarkdownLinkMapping>,
    pub attachments: Vec<MarkdownAttachmentMapping>,
    pub losses: Vec<MarkdownImportLoss>,
    pub conflicts: Vec<MarkdownImportConflict>,
    pub name_conflicts: Vec<MarkdownNameConflict>,
    pub files: Vec<MarkdownImportFilePreview>,
    pub requires_language_upgrade: bool,
    pub can_apply: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarkdownImportResult {
    pub plan: MarkdownImportPlan,
    pub changed_files: Vec<PathBuf>,
    pub baseline: String,
    pub new_baseline: String,
}

/// 从快照应用迁移后，供宿主显式接收的完整候选工作区。
#[derive(Debug, Clone)]
pub struct MarkdownImportSnapshotResult {
    pub result: MarkdownImportResult,
    pub workspace_files: crate::workspace_snapshot::Files,
}

#[derive(Debug, Clone)]
enum InputFileSource<'a> {
    Snapshot(&'a [u8]),
    #[cfg(not(target_arch = "wasm32"))]
    Disk {
        root: PathBuf,
        absolute: PathBuf,
    },
}

#[derive(Debug, Clone)]
struct InputFile<'a> {
    relative: String,
    length: u64,
    source: InputFileSource<'a>,
}

#[derive(Debug, Clone)]
struct SourcePage {
    relative: String,
    bytes: Vec<u8>,
    id: String,
    invalid_front_matter_id: bool,
    title: String,
    entity_type: String,
    description: String,
    headings: Vec<Heading>,
    links: Vec<InlineReference>,
    attachments: Vec<InlineReference>,
    losses: Vec<MarkdownImportLoss>,
}

#[derive(Debug, Clone)]
struct Heading {
    slug: String,
    id: String,
    title: String,
    line: u32,
}

#[derive(Debug, Clone)]
struct InlineReference {
    href: String,
    label: String,
    line: u32,
}

#[derive(Debug, Clone)]
struct ResolvedAttachment<'a> {
    source_page: String,
    reference: InlineReference,
    input: InputFile<'a>,
    bytes: Arc<Vec<u8>>,
    id: String,
    output_path: String,
    quarantined: bool,
}

struct PreparedPreview {
    plan: MarkdownImportPlan,
    candidate: Project,
    additional_files: Vec<(PathBuf, Vec<u8>)>,
}

struct ParsedMarkdownSource<'a> {
    source_root: PathBuf,
    inputs: Vec<InputFile<'a>>,
    markdown: Vec<InputFile<'a>>,
    pages: Vec<SourcePage>,
}

type CandidateBuild = (
    Project,
    Vec<(PathBuf, Vec<u8>)>,
    Vec<MarkdownImportFilePreview>,
    Vec<MarkdownImportConflict>,
);

#[derive(Serialize)]
struct PlanDigest<'a> {
    source_fingerprint: &'a str,
    baseline: &'a str,
    namespace: &'a str,
    pages: &'a [MarkdownPageMapping],
    links: &'a [MarkdownLinkMapping],
    attachments: &'a [MarkdownAttachmentMapping],
    losses: &'a [MarkdownImportLoss],
    conflicts: &'a [MarkdownImportConflict],
    name_conflicts: &'a [MarkdownNameConflict],
    files: &'a [MarkdownImportFilePreview],
    requires_language_upgrade: bool,
}

impl Project {
    /// 对显式选择的 Markdown 目录构建只读迁移预览。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_markdown_import(
        &self,
        request: &MarkdownImportRequest,
    ) -> Result<MarkdownImportPlan, String> {
        let options = MarkdownImportOptions::from(request);
        planning::validate_project_for_import(self, &options)?;
        let source_root = source::checked_source_root(&request.source_root)?;
        let inputs = source::enumerate_source_files(&source_root)?;
        planning::prepare_preview(self, source_root, inputs, &options).map(|prepared| prepared.plan)
    }

    /// 重新验证预览后，在一个 Project 候选和可恢复保存事务中应用迁移。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_markdown_import(
        &mut self,
        request: &MarkdownImportRequest,
        plan_digest: &str,
    ) -> Result<MarkdownImportResult, String> {
        let options = MarkdownImportOptions::from(request);
        planning::validate_project_for_import(self, &options)?;
        let source_root = source::checked_source_root(&request.source_root)?;
        let inputs = source::enumerate_source_files(&source_root)?;
        let mut prepared = planning::prepare_preview(self, source_root, inputs, &options)?;
        if prepared.plan.plan_digest != plan_digest {
            return Err("Markdown 迁移预览已过期；来源、映射或工程基线已变化".into());
        }
        apply::ensure_plan_applicable(&prepared.plan, &options)?;
        let changed_files = prepared
            .plan
            .files
            .iter()
            .map(|file| self.root.join(&file.path))
            .chain(std::iter::once(self.entry.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let baseline = self.content_baseline();
        prepared
            .candidate
            .save_with_additional_files(&prepared.additional_files)?;
        let new_baseline = prepared.candidate.content_baseline();
        prepared.plan.new_baseline = new_baseline.clone();
        *self = prepared.candidate;
        Ok(MarkdownImportResult {
            plan: prepared.plan,
            changed_files,
            baseline,
            new_baseline,
        })
    }

    /// 对用户明确选择的普通文件快照构建只读预览，不访问 `label` 对应的宿主路径。
    pub fn preview_markdown_import_snapshot(
        &self,
        source: &MarkdownImportSourceSnapshot<'_>,
        options: &MarkdownImportOptions,
    ) -> Result<MarkdownImportPlan, String> {
        source::validate_snapshot_label(source.label)?;
        planning::validate_project_for_import(self, options)?;
        let inputs = source::enumerate_snapshot_files(source.files)?;
        planning::prepare_preview(self, PathBuf::from(source.label), inputs, options)
            .map(|prepared| prepared.plan)
    }

    /// 在内存中重算预览并返回完整候选 Files；不写宿主存储，也不修改当前 Project。
    pub fn apply_markdown_import_snapshot(
        &self,
        source: &MarkdownImportSourceSnapshot<'_>,
        options: &MarkdownImportOptions,
        plan_digest: &str,
    ) -> Result<MarkdownImportSnapshotResult, String> {
        source::validate_snapshot_label(source.label)?;
        planning::validate_project_for_import(self, options)?;
        let inputs = source::enumerate_snapshot_files(source.files)?;
        let mut prepared =
            planning::prepare_preview(self, PathBuf::from(source.label), inputs, options)?;
        if prepared.plan.plan_digest != plan_digest {
            return Err("Markdown 迁移预览已过期；来源、映射或工程基线已变化".into());
        }
        apply::ensure_plan_applicable(&prepared.plan, options)?;
        let changed_files = prepared
            .plan
            .files
            .iter()
            .map(|file| self.root.join(&file.path))
            .chain(std::iter::once(self.entry.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let baseline = self.content_baseline();
        let mut workspace_files = crate::workspace_snapshot::snapshot_files(&prepared.candidate)?;
        for (relative, bytes) in prepared.additional_files.drain(..) {
            apply::insert_candidate_file(&mut workspace_files, relative, bytes)?;
        }
        let new_baseline = prepared.candidate.content_baseline();
        prepared.plan.new_baseline = new_baseline.clone();
        Ok(MarkdownImportSnapshotResult {
            result: MarkdownImportResult {
                plan: prepared.plan,
                changed_files,
                baseline,
                new_baseline,
            },
            workspace_files,
        })
    }
}

fn valid_id(value: &str) -> bool {
    crate::lexer::valid_identifier(value)
}

fn loss(
    code: &str,
    source: &str,
    line: u32,
    message: String,
    preserved_at: Option<String>,
) -> MarkdownImportLoss {
    MarkdownImportLoss {
        code: code.to_string(),
        source: source.to_string(),
        line,
        message,
        preserved_at,
    }
}

fn source_fingerprint(
    inputs: &[InputFile<'_>],
    pages: &[SourcePage],
    attachments: &[ResolvedAttachment<'_>],
) -> Result<String, String> {
    let mut bytes = Vec::new();
    for input in inputs {
        bytes.extend_from_slice(input.relative.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&input.length.to_le_bytes());
        bytes.push(0xff);
    }
    for page in pages {
        bytes.extend_from_slice(page.relative.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&page.bytes);
        bytes.push(0xff);
    }
    let mut unique = BTreeMap::<&str, &[u8]>::new();
    for attachment in attachments {
        unique
            .entry(&attachment.input.relative)
            .or_insert_with(|| attachment.bytes.as_slice());
    }
    for (relative, contents) in unique {
        bytes.extend_from_slice(relative.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(contents);
        bytes.push(0xff);
    }
    Ok(format!("{:016x}", hash_bytes(&bytes)))
}

fn hash_strings<'a>(values: impl IntoIterator<Item = &'a str>) -> u64 {
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
    }
    hash_bytes(&bytes)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn digest_plan(plan: PlanDigest<'_>) -> Result<String, String> {
    let bytes = serde_json::to_vec(&plan)
        .map_err(|error| format!("无法生成 Markdown 预览摘要：{error}"))?;
    Ok(format!("{:016x}", hash_bytes(&bytes)))
}

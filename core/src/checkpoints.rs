//! 有界本地工程检查点与基于当前基线的受保护恢复。

use crate::catalog::TargetRef;
use crate::project::Project;
use crate::workspace_snapshot::Files;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use web_time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_CHECKPOINTS: usize = 20;
pub const DEFAULT_MAX_CHECKPOINT_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_CHECKPOINT_HISTORY_BYTES: usize = 256 * 1024 * 1024;
const MAX_CHECKPOINT_FILES: usize = 4096;
#[cfg(not(target_arch = "wasm32"))]
const MAX_CHECKPOINT_PAYLOADS: usize = MAX_CHECKPOINT_FILES * 2;
const MAX_CHECKPOINT_TEXT_FILES: usize = 32;
const MAX_CHECKPOINT_TEXT_BYTES: usize = 16 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_CHECKPOINT_RECORDS_ON_DISK: usize = 128;
#[cfg(not(target_arch = "wasm32"))]
const MAX_CHECKPOINT_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const CHECKPOINT_FORMAT_VERSION: u32 = 2;
const LEGACY_CHECKPOINT_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointLimits {
    pub max_count: usize,
    pub max_checkpoint_bytes: usize,
    pub max_total_bytes: usize,
}

impl Default for CheckpointLimits {
    fn default() -> Self {
        Self {
            max_count: DEFAULT_MAX_CHECKPOINTS,
            max_checkpoint_bytes: DEFAULT_MAX_CHECKPOINT_BYTES,
            max_total_bytes: DEFAULT_MAX_CHECKPOINT_HISTORY_BYTES,
        }
    }
}

impl CheckpointLimits {
    fn validate(&self) -> Result<(), String> {
        if self.max_count == 0
            || self.max_count > DEFAULT_MAX_CHECKPOINTS
            || self.max_checkpoint_bytes == 0
            || self.max_checkpoint_bytes > DEFAULT_MAX_CHECKPOINT_BYTES
            || self.max_total_bytes == 0
            || self.max_total_bytes > DEFAULT_MAX_CHECKPOINT_HISTORY_BYTES
        {
            return Err("检查点配额必须大于零且不能超过 core 默认上限".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointSummary {
    pub id: String,
    pub label: Option<String>,
    pub created_at_unix_ms: u64,
    pub file_count: usize,
    /// 当前工作区与额外保存基线负载的原始字节总数，不含清单与文件系统开销。
    pub payload_bytes: u64,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointFileOperation {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointFileChange {
    /// 工作区根目录下的安全相对路径。
    pub path: PathBuf,
    pub operation: CheckpointFileOperation,
    pub current_bytes: Option<u64>,
    pub checkpoint_bytes: Option<u64>,
    pub affected_objects: Vec<TargetRef>,
    pub objects_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointTextSourceRange {
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointTextDifference {
    pub path: PathBuf,
    pub base: Option<String>,
    pub current: Option<String>,
    pub checkpoint: Option<String>,
    pub base_range: Option<CheckpointTextSourceRange>,
    pub current_range: Option<CheckpointTextSourceRange>,
    pub checkpoint_range: Option<CheckpointTextSourceRange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointTextSourceSnippets {
    pub base: Option<String>,
    pub current: Option<String>,
    pub checkpoint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointTextDiffSummary {
    pub base_bytes: Option<u64>,
    pub current_bytes: Option<u64>,
    pub checkpoint_bytes: Option<u64>,
    /// None means unknown or undecodable; a known absent side is reported as Some(0).
    pub base_lines: Option<usize>,
    pub current_lines: Option<usize>,
    pub checkpoint_lines: Option<usize>,
    pub difference_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointTextDiff {
    pub path: PathBuf,
    /// False for version 1 records, which did not capture a saved-text baseline.
    pub base_available: bool,
    pub summary: CheckpointTextDiffSummary,
    pub differences: Vec<CheckpointTextDifference>,
    pub raw: CheckpointTextSourceSnippets,
    pub alignment_uncertain: bool,
    pub undecodable: bool,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRestorePlan {
    pub checkpoint_id: String,
    pub expected_content_baseline: String,
    pub expected_workspace_digest: String,
    pub expected_disk_digest: String,
    pub checkpoint_digest: String,
    pub fingerprint_before: u64,
    pub fingerprint_after: u64,
    pub changes: Vec<CheckpointFileChange>,
    #[serde(default)]
    pub text_differences: Vec<CheckpointTextDiff>,
    #[serde(default)]
    pub text_differences_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRestoreResult {
    pub checkpoint_id: String,
    pub restored_files: usize,
    pub fingerprint_before: u64,
    pub fingerprint_after: u64,
    pub content_baseline: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CheckpointManifest {
    version: u32,
    id: String,
    label: Option<String>,
    created_at_unix_ms: u64,
    payload_bytes: u64,
    snapshot_digest: String,
    files: Vec<CheckpointFileEntry>,
    #[serde(default)]
    text_base: Option<Vec<CheckpointTextBaseEntry>>,
    #[serde(default)]
    text_base_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CheckpointTextBaseEntry {
    path: String,
    source: CheckpointTextBaseSource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
enum CheckpointTextBaseSource {
    Absent,
    Snapshot,
    Stored {
        payload: String,
        bytes: u64,
        checksum: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CheckpointFileEntry {
    path: String,
    payload: String,
    bytes: u64,
    checksum: String,
}

#[derive(Clone, Debug)]
struct CheckpointBundle {
    manifest: CheckpointManifest,
    files: Files,
    text_base: Option<BTreeMap<PathBuf, Option<Vec<u8>>>>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CheckpointScopeKey {
    root: PathBuf,
    session_id: String,
}

#[cfg(any(target_arch = "wasm32", test))]
impl CheckpointScopeKey {
    fn new(root: &Path, session_id: &str) -> Self {
        Self {
            root: crate::compiler::source_path(root),
            session_id: session_id.to_owned(),
        }
    }
}

/// The browser production backend and its native regression test share this
/// exact store implementation; session identity is part of every lookup key.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
struct InMemoryCheckpointStore {
    records: BTreeMap<CheckpointScopeKey, BTreeMap<String, CheckpointBundle>>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl InMemoryCheckpointStore {
    fn publish(
        &mut self,
        scope: CheckpointScopeKey,
        manifest: CheckpointManifest,
        files: &Files,
        text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
        limits: &CheckpointLimits,
    ) -> Result<(), String> {
        validate_checkpoint_id(&manifest.id)?;
        let records = self.records.entry(scope).or_default();
        if records.len() >= limits.max_count {
            return Err("检查点数量配额已满，请显式删除旧记录".into());
        }
        let used_bytes = records
            .values()
            .try_fold(0u64, |total, checkpoint| {
                total.checked_add(checkpoint.manifest.payload_bytes)
            })
            .ok_or("检查点历史字节数超出可表示范围")?;
        if used_bytes.saturating_add(manifest.payload_bytes) > limits.max_total_bytes as u64 {
            return Err("检查点历史字节配额已满，请显式删除旧记录".into());
        }
        if files_digest(files) != manifest.snapshot_digest
            || manifest.text_base_digest.as_deref() != text_base_digest(Some(text_base)).as_deref()
        {
            return Err("检查点内存记录摘要不一致".into());
        }
        records.insert(
            manifest.id.clone(),
            CheckpointBundle {
                manifest,
                files: files.clone(),
                text_base: Some(text_base.clone()),
            },
        );
        Ok(())
    }

    fn list(&self, scope: &CheckpointScopeKey) -> Vec<CheckpointListing> {
        self.records
            .get(scope)
            .into_iter()
            .flat_map(|records| records.values())
            .map(|bundle| CheckpointListing {
                summary: summary_for_manifest(&bundle.manifest),
            })
            .collect()
    }

    fn load(&self, scope: &CheckpointScopeKey, id: &str) -> Result<CheckpointBundle, String> {
        validate_checkpoint_id(id)?;
        self.records
            .get(scope)
            .and_then(|records| records.get(id))
            .cloned()
            .ok_or_else(|| "检查点不存在或已损坏".into())
    }

    fn delete(&mut self, scope: &CheckpointScopeKey, id: &str) -> Result<(), String> {
        validate_checkpoint_id(id)?;
        let Some(records) = self.records.get_mut(scope) else {
            return Err("检查点不存在".into());
        };
        if records.remove(id).is_none() {
            return Err("检查点不存在".into());
        }
        if records.is_empty() {
            self.records.remove(scope);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct CheckpointListing {
    summary: CheckpointSummary,
}

#[derive(Clone, Debug)]
#[cfg(not(target_arch = "wasm32"))]
struct RestoreFile {
    relative: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

impl Project {
    /// 捕获当前 Project 缓冲与普通工作区文件；不保存作者文件或推进基线。
    pub fn create_checkpoint(
        &self,
        label: Option<String>,
        limits: CheckpointLimits,
    ) -> Result<CheckpointSummary, String> {
        limits.validate()?;
        if label
            .as_ref()
            .is_some_and(|label| label.chars().count() > 120)
        {
            return Err("检查点标签不能超过 120 个字符".into());
        }
        self.checkpoint_disk_baselines_match()?;
        ensure_workspace_snapshot_limits(self, limits.max_checkpoint_bytes as u64)?;
        let files = crate::workspace_snapshot::snapshot_files(self)?;
        validate_snapshot_files(&files)?;
        let text_base = saved_source_baselines(self);
        let payload_bytes = checkpoint_payload_bytes(&files, &text_base)?;
        if files.len() > MAX_CHECKPOINT_FILES || text_base.len() > MAX_CHECKPOINT_FILES {
            return Err("工作区文件或保存基线数量超过检查点上限".into());
        }
        if payload_bytes > limits.max_checkpoint_bytes as u64 {
            return Err("检查点超过单条字节配额".into());
        }

        let manifest = make_manifest(label, &files, &text_base, payload_bytes)?;
        publish_project_checkpoint(self, manifest.clone(), &files, &text_base, &limits)?;
        Ok(summary_for_manifest(&manifest))
    }

    /// 校验所有完整记录后按创建时间倒序列举；受损记录会标记不可用。
    pub fn list_checkpoints(&self) -> Result<Vec<CheckpointSummary>, String> {
        let mut records = list_project_checkpoint_records(self)?;
        records.sort_by(|left, right| {
            right
                .summary
                .created_at_unix_ms
                .cmp(&left.summary.created_at_unix_ms)
                .then_with(|| right.summary.id.cmp(&left.summary.id))
        });
        Ok(records.into_iter().map(|record| record.summary).collect())
    }

    /// 显式删除单条历史，包括已损坏且不可恢复的记录。
    pub fn delete_checkpoint(&self, id: &str) -> Result<(), String> {
        validate_checkpoint_id(id)?;
        delete_project_checkpoint_record(self, id)
    }

    /// 创建绑定当前缓冲和完整磁盘快照的逐文件恢复计划。
    pub fn preview_checkpoint_restore(&self, id: &str) -> Result<CheckpointRestorePlan, String> {
        validate_checkpoint_id(id)?;
        self.checkpoint_disk_baselines_match()?;
        let checkpoint = load_project_checkpoint(self, id)?;
        preview_restore(self, checkpoint)
    }

    /// 重新验证计划、Project 与磁盘基线，然后通过保存事务应用完整快照。
    pub fn restore_checkpoint(
        &mut self,
        plan: &CheckpointRestorePlan,
    ) -> Result<CheckpointRestoreResult, String> {
        if self.content_baseline() != plan.expected_content_baseline {
            return Err("StaleCheckpointPlan：Project 缓冲已变化，请重新预览".into());
        }
        self.checkpoint_disk_baselines_match()?;
        let checkpoint = load_project_checkpoint(self, &plan.checkpoint_id)?;
        ensure_workspace_snapshot_limits(self, DEFAULT_MAX_CHECKPOINT_BYTES as u64)?;
        let current_files = crate::workspace_snapshot::snapshot_files(self)?;
        validate_snapshot_files(&current_files)?;
        let disk_files = disk_workspace_files(&self.root)?;
        if files_digest(&current_files) != plan.expected_workspace_digest
            || files_digest(&disk_files) != plan.expected_disk_digest
            || checkpoint_record_digest(&checkpoint.manifest) != plan.checkpoint_digest
        {
            return Err("StaleCheckpointPlan：工作区或检查点已变化，请重新预览".into());
        }
        let verified_plan = preview_restore(self, checkpoint.clone())?;
        if &verified_plan != plan {
            return Err("CheckpointPlanMismatch：恢复计划与 core 预览不一致".into());
        }
        let disk_changes = differing_paths(&disk_files, &checkpoint.files);
        self.ensure_workspace_writable()?;
        ensure_restore_targets_writable(self, &checkpoint.files, &plan.changes, &disk_changes)?;
        let mut candidate = self.clone();
        candidate.reset_to_checkpoint_files(&checkpoint.files)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let pending = files_to_pending(&disk_files, &checkpoint.files);
            persist_restored_files(&self.root, &pending, &checkpoint.files)?;
        }
        #[cfg(target_arch = "wasm32")]
        persist_restored_files(&self.root, &checkpoint.files)?;
        let restored_disk = disk_workspace_files(&self.root)?;
        if restored_disk != checkpoint.files {
            return Err("检查点恢复后工作区与目标不一致，请刷新并检查外部修改".into());
        }

        let result = CheckpointRestoreResult {
            checkpoint_id: plan.checkpoint_id.clone(),
            restored_files: plan.changes.len(),
            fingerprint_before: plan.fingerprint_before,
            fingerprint_after: candidate.compile_current().analysis.fingerprint,
            content_baseline: candidate.content_baseline(),
        };
        *self = candidate;
        Ok(result)
    }
}

#[cfg(target_arch = "wasm32")]
impl Project {
    /// 身份不进入作品文件；宿主可在恢复同一浏览器保存时复用它。
    pub fn checkpoint_session_id(&self) -> &str {
        &self.checkpoint_session_id
    }

    /// 将浏览器检查点限定到宿主确认的工作区会话。
    pub fn set_checkpoint_session_id(&mut self, id: impl Into<String>) -> Result<(), String> {
        let id = id.into();
        validate_checkpoint_session_id(&id)?;
        self.checkpoint_session_id = id;
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
fn validate_checkpoint_session_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("检查点会话标识无效".into());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn next_checkpoint_session_id() -> String {
    format!("browser-{}", next_checkpoint_id())
}

fn publish_project_checkpoint(
    project: &Project,
    manifest: CheckpointManifest,
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    limits: &CheckpointLimits,
) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        publish_checkpoint(&project.root, manifest, files, text_base, limits)
    }
    #[cfg(target_arch = "wasm32")]
    {
        publish_checkpoint(
            &project.root,
            &project.checkpoint_session_id,
            manifest,
            files,
            text_base,
            limits,
        )
    }
}

fn list_project_checkpoint_records(project: &Project) -> Result<Vec<CheckpointListing>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        list_checkpoint_records(&project.root)
    }
    #[cfg(target_arch = "wasm32")]
    {
        list_checkpoint_records(&project.root, &project.checkpoint_session_id)
    }
}

fn load_project_checkpoint(project: &Project, id: &str) -> Result<CheckpointBundle, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        load_checkpoint(&project.root, id)
    }
    #[cfg(target_arch = "wasm32")]
    {
        load_checkpoint(&project.root, &project.checkpoint_session_id, id)
    }
}

fn delete_project_checkpoint_record(project: &Project, id: &str) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        delete_checkpoint_record(&project.root, id)
    }
    #[cfg(target_arch = "wasm32")]
    {
        delete_checkpoint_record(&project.root, &project.checkpoint_session_id, id)
    }
}

fn preview_restore(
    project: &Project,
    checkpoint: CheckpointBundle,
) -> Result<CheckpointRestorePlan, String> {
    ensure_workspace_snapshot_limits(project, DEFAULT_MAX_CHECKPOINT_BYTES as u64)?;
    let current_files = crate::workspace_snapshot::snapshot_files(project)?;
    validate_snapshot_files(&current_files)?;
    let disk_files = disk_workspace_files(&project.root)?;
    let (current_compile, current_complete) = compile_snapshot(project, &current_files);
    let (checkpoint_compile, checkpoint_complete) = compile_snapshot(project, &checkpoint.files);
    let current_objects = objects_by_source(&current_compile);
    let checkpoint_objects = objects_by_source(&checkpoint_compile);
    let before_fingerprint = current_compile.analysis.fingerprint;
    let after_fingerprint = checkpoint_compile.analysis.fingerprint;
    let mut changed_paths = BTreeSet::new();
    changed_paths.extend(current_files.keys().cloned());
    changed_paths.extend(checkpoint.files.keys().cloned());
    let mut changes = Vec::new();
    for path in changed_paths {
        let current = current_files.get(&path);
        let restored = checkpoint.files.get(&path);
        if current == restored {
            continue;
        }
        let operation = match (current, restored) {
            (None, Some(_)) => CheckpointFileOperation::Added,
            (Some(_), None) => CheckpointFileOperation::Deleted,
            (Some(_), Some(_)) => CheckpointFileOperation::Modified,
            (None, None) => continue,
        };
        let source_file = path.extension().is_some_and(|extension| extension == "wl");
        let mut affected_objects = BTreeSet::new();
        if source_file {
            let absolute = crate::compiler::source_path(&project.root.join(&path));
            for objects in [&current_objects, &checkpoint_objects] {
                if let Some(objects) = objects.get(&absolute) {
                    affected_objects.extend(objects.iter().cloned());
                }
            }
        }
        changes.push(CheckpointFileChange {
            path,
            operation,
            current_bytes: current.map(|bytes| bytes.len() as u64),
            checkpoint_bytes: restored.map(|bytes| bytes.len() as u64),
            affected_objects: affected_objects.into_iter().collect(),
            objects_complete: !source_file
                || (current_complete
                    && checkpoint_complete
                    && !current_compile.has_errors()
                    && !checkpoint_compile.has_errors()),
        });
    }
    let (text_differences, text_differences_truncated) =
        checkpoint_text_differences(&current_files, &checkpoint);
    Ok(CheckpointRestorePlan {
        checkpoint_id: checkpoint.manifest.id.clone(),
        expected_content_baseline: project.content_baseline(),
        expected_workspace_digest: files_digest(&current_files),
        expected_disk_digest: files_digest(&disk_files),
        checkpoint_digest: checkpoint_record_digest(&checkpoint.manifest),
        fingerprint_before: before_fingerprint,
        fingerprint_after: after_fingerprint,
        changes,
        text_differences,
        text_differences_truncated,
    })
}

fn checkpoint_text_differences(
    current_files: &Files,
    checkpoint: &CheckpointBundle,
) -> (Vec<CheckpointTextDiff>, bool) {
    let mut paths = BTreeSet::new();
    for files in [current_files, &checkpoint.files] {
        paths.extend(
            files
                .keys()
                .filter(|path| path.extension().is_some_and(|extension| extension == "wl"))
                .cloned(),
        );
    }
    if let Some(bases) = &checkpoint.text_base {
        paths.extend(bases.keys().cloned());
    }

    let mut result = Vec::new();
    let mut truncated_files = false;
    for path in paths {
        let current = current_files.get(&path);
        let target = checkpoint.files.get(&path);
        let captured_base = checkpoint
            .text_base
            .as_ref()
            .and_then(|bases| bases.get(&path));
        let base_available = captured_base.is_some();
        let base = captured_base.and_then(Option::as_ref);
        let differs = if base_available {
            base != current && base != target || current != target
        } else {
            current != target
        };
        if !differs {
            continue;
        }
        if result.len() == MAX_CHECKPOINT_TEXT_FILES {
            truncated_files = true;
            break;
        }
        result.push(project_checkpoint_text_diff(
            path,
            base_available,
            base.map(Vec::as_slice),
            current.map(Vec::as_slice),
            target.map(Vec::as_slice),
        ));
    }
    (result, truncated_files)
}

fn project_checkpoint_text_diff(
    path: PathBuf,
    base_available: bool,
    base: Option<&[u8]>,
    current: Option<&[u8]>,
    checkpoint: Option<&[u8]>,
) -> CheckpointTextDiff {
    let decoded_base = base.map(std::str::from_utf8).transpose();
    let decoded_current = current.map(std::str::from_utf8).transpose();
    let decoded_checkpoint = checkpoint.map(std::str::from_utf8).transpose();
    let undecodable =
        decoded_base.is_err() || decoded_current.is_err() || decoded_checkpoint.is_err();
    let mut truncated = false;
    let (differences, alignment_uncertain, raw) = if undecodable {
        (
            Vec::new(),
            true,
            CheckpointTextSourceSnippets {
                base: base.map(|bytes| escaped_byte_snippet(bytes, &mut truncated)),
                current: current.map(|bytes| escaped_byte_snippet(bytes, &mut truncated)),
                checkpoint: checkpoint.map(|bytes| escaped_byte_snippet(bytes, &mut truncated)),
            },
        )
    } else if !base_available {
        (
            Vec::new(),
            true,
            CheckpointTextSourceSnippets {
                base: None,
                current: decoded_current
                    .ok()
                    .flatten()
                    .map(|text| bounded_text(text, &mut truncated)),
                checkpoint: decoded_checkpoint
                    .ok()
                    .flatten()
                    .map(|text| bounded_text(text, &mut truncated)),
            },
        )
    } else {
        let (proposal_differences, review_truncated, uncertain, raw) =
            crate::collaboration::review_checkpoint_text(
                decoded_base.ok().flatten(),
                decoded_current.ok().flatten(),
                decoded_checkpoint.ok().flatten(),
            );
        truncated |= review_truncated;
        let mut differences = proposal_differences
            .into_iter()
            .map(|difference| CheckpointTextDifference {
                path: path.clone(),
                base: difference.base,
                current: difference.current,
                checkpoint: difference.proposed,
                base_range: difference.base_range.map(checkpoint_source_range),
                current_range: difference.current_range.map(checkpoint_source_range),
                checkpoint_range: difference.proposed_range.map(checkpoint_source_range),
            })
            .collect::<Vec<_>>();
        truncated |= limit_checkpoint_hunks(&mut differences);
        (
            differences,
            uncertain,
            CheckpointTextSourceSnippets {
                base: raw.base,
                current: raw.current,
                checkpoint: raw.proposed,
            },
        )
    };
    let summary = CheckpointTextDiffSummary {
        base_bytes: base.map(|bytes| bytes.len() as u64),
        current_bytes: current.map(|bytes| bytes.len() as u64),
        checkpoint_bytes: checkpoint.map(|bytes| bytes.len() as u64),
        base_lines: if !base_available || decoded_base.is_err() {
            None
        } else {
            Some(
                decoded_base
                    .ok()
                    .flatten()
                    .map_or(0, |text| text.lines().count()),
            )
        },
        current_lines: if decoded_current.is_err() {
            None
        } else {
            Some(
                decoded_current
                    .ok()
                    .flatten()
                    .map_or(0, |text| text.lines().count()),
            )
        },
        checkpoint_lines: if decoded_checkpoint.is_err() {
            None
        } else {
            Some(
                decoded_checkpoint
                    .ok()
                    .flatten()
                    .map_or(0, |text| text.lines().count()),
            )
        },
        difference_count: differences.len(),
    };
    CheckpointTextDiff {
        path,
        base_available,
        summary,
        differences,
        raw,
        alignment_uncertain,
        undecodable,
        truncated,
    }
}

fn checkpoint_source_range(
    range: crate::collaboration::ProposalSourceRange,
) -> CheckpointTextSourceRange {
    CheckpointTextSourceRange {
        start_byte: range.start_byte,
        end_byte: range.end_byte,
    }
}

fn bounded_text(value: &str, truncated: &mut bool) -> String {
    if value.len() <= MAX_CHECKPOINT_TEXT_BYTES {
        return value.to_owned();
    }
    *truncated = true;
    let mut end = MAX_CHECKPOINT_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn escaped_byte_snippet(bytes: &[u8], truncated: &mut bool) -> String {
    const PREFIX: &str = "hex:";
    let max_bytes = (MAX_CHECKPOINT_TEXT_BYTES - PREFIX.len()) / 2;
    let shown = bytes.len().min(max_bytes);
    if shown < bytes.len() {
        *truncated = true;
    }
    let mut output = String::with_capacity(PREFIX.len() + shown * 2);
    output.push_str(PREFIX);
    for byte in &bytes[..shown] {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn limit_checkpoint_hunks(differences: &mut [CheckpointTextDifference]) -> bool {
    let mut remaining = MAX_CHECKPOINT_TEXT_BYTES;
    let mut truncated = false;
    for difference in differences {
        for snippet in [
            &mut difference.base,
            &mut difference.current,
            &mut difference.checkpoint,
        ] {
            let Some(text) = snippet else {
                continue;
            };
            if text.len() <= remaining {
                remaining -= text.len();
                continue;
            }
            let mut end = remaining;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            remaining = 0;
            truncated = true;
        }
    }
    truncated
}

fn objects_by_source(result: &crate::CompileResult) -> BTreeMap<PathBuf, BTreeSet<TargetRef>> {
    let mut objects_by_source = BTreeMap::new();
    for object in &result.analysis.catalog.objects {
        objects_by_source
            .entry(crate::compiler::source_path(Path::new(&object.file)))
            .or_insert_with(BTreeSet::new)
            .insert(object.target.clone());
    }
    objects_by_source
}

fn compile_snapshot(project: &Project, files: &Files) -> (crate::CompileResult, bool) {
    let manifest_path = Path::new(".world/project.json");
    let registry = files
        .get(manifest_path)
        .map(|manifest| crate::workspace_documents::parse_registry(&project.root, manifest))
        .unwrap_or_default();
    let mut complete = registry.diagnostics.is_empty();
    let mut sources = BTreeMap::new();
    for (relative, bytes) in files {
        if relative
            .extension()
            .is_some_and(|extension| extension == "wl")
        {
            match String::from_utf8(bytes.clone()) {
                Ok(text) => {
                    sources.insert(
                        crate::compiler::source_path(&project.root.join(relative)),
                        text,
                    );
                }
                Err(_) => complete = false,
            }
        }
    }
    let active = registry.source_selection.as_ref();
    let inactive = sources
        .keys()
        .filter(|path| active.is_some_and(|selection| !selection.is_active(path)))
        .cloned()
        .collect::<HashSet<_>>();
    let deleted = project
        .documents
        .keys()
        .filter(|path| !sources.contains_key(*path))
        .cloned()
        .collect::<HashSet<_>>();
    let result = crate::compiler::compile_sources_excluding_inactive_with_options(
        &project.entry,
        &sources,
        deleted,
        inactive,
        crate::CompileOptions::new(registry.language_version),
    );
    (result, complete)
}

fn validate_snapshot_files(files: &Files) -> Result<(), String> {
    if files.len() > MAX_CHECKPOINT_FILES {
        return Err("工作区文件数超过检查点上限".into());
    }
    for (path, bytes) in files {
        validate_relative_file(path)?;
        if path.extension().is_some_and(|extension| extension == "wl")
            && std::str::from_utf8(bytes).is_err()
        {
            return Err(format!("检查点源码不是有效 UTF-8：{}", path.display()));
        }
    }
    Ok(())
}

fn validate_relative_file(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.components().any(|component| match component {
            Component::Normal(name) => name
                .to_str()
                .is_none_or(|name| name.is_empty() || name.contains(['\\', '/', ':'])),
            _ => true,
        })
        || is_reserved_store_path(path, ".checkpoints")
        || is_reserved_store_path(path, ".transactions")
    {
        return Err(format!("检查点文件路径无效：{}", path.display()));
    }
    Ok(())
}

fn is_reserved_store_path(path: &Path, store: &str) -> bool {
    let mut components = path.components();
    let (Some(Component::Normal(world)), Some(Component::Normal(directory))) =
        (components.next(), components.next())
    else {
        return false;
    };
    store_name_matches(world, ".world") && store_name_matches(directory, store)
}

fn store_name_matches(name: &std::ffi::OsStr, expected: &str) -> bool {
    #[cfg(windows)]
    {
        name.to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
    }
    #[cfg(not(windows))]
    {
        name == expected
    }
}

fn ensure_workspace_snapshot_limits(project: &Project, max_bytes: u64) -> Result<(), String> {
    let root = crate::compiler::source_path(&project.root);
    let disk_paths = match crate::file_access::workspace_files(&root) {
        Ok(paths) => paths,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("无法读取检查点工作区：{error}")),
    };
    let mut managed = HashSet::new();
    let mut count = 0usize;
    let mut bytes = 0u64;
    for (path, document) in &project.documents {
        managed.insert(crate::compiler::source_path(path));
        if !document.is_deleted() {
            count += 1;
            bytes = bytes
                .checked_add(document.text.len() as u64)
                .ok_or("检查点字节数超出可表示范围")?;
        }
    }
    for (path, document) in &project.authoring_documents {
        managed.insert(crate::compiler::source_path(path));
        if !document.is_deleted() {
            count += 1;
            bytes = bytes
                .checked_add(document.bytes().len() as u64)
                .ok_or("检查点字节数超出可表示范围")?;
        }
    }
    for path in disk_paths {
        if managed.contains(&crate::compiler::source_path(&path)) {
            continue;
        }
        count += 1;
        let size = disk_file_size(&path)?;
        bytes = bytes
            .checked_add(size)
            .ok_or("检查点字节数超出可表示范围")?;
        if count > MAX_CHECKPOINT_FILES || bytes > max_bytes {
            return Err("工作区超过检查点文件数或字节上限".into());
        }
    }
    if count > MAX_CHECKPOINT_FILES || bytes > max_bytes {
        return Err("工作区超过检查点文件数或字节上限".into());
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn disk_file_size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("无法读取工作区文件大小 {}：{error}", path.display()))
}

fn ensure_disk_file_limits(paths: &[PathBuf], max_bytes: u64) -> Result<(), String> {
    if paths.len() > MAX_CHECKPOINT_FILES {
        return Err("工作区超过检查点文件数上限".into());
    }
    let mut total = 0u64;
    for path in paths {
        total = total
            .checked_add(disk_file_size(path)?)
            .ok_or("检查点字节数超出可表示范围")?;
        if total > max_bytes {
            return Err("工作区超过检查点字节上限".into());
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn disk_file_size(path: &Path) -> Result<u64, String> {
    crate::file_access::read(path)
        .map(|bytes| bytes.len() as u64)
        .map_err(|error| format!("无法读取工作区文件大小 {}：{error}", path.display()))
}

fn files_digest(files: &Files) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-files-v1");
    for (path, bytes) in files {
        let path = path.to_string_lossy().replace('\\', "/");
        mix(&mut hash, path.as_bytes());
        mix(&mut hash, bytes);
    }
    format!("{hash:016x}")
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn file_checksum(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn saved_source_baselines(project: &Project) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let root = crate::compiler::source_path(&project.root);
    project
        .documents
        .keys()
        .filter_map(|path| {
            let path = crate::compiler::source_path(path);
            if !path.extension().is_some_and(|extension| extension == "wl") {
                return None;
            }
            let relative = path.strip_prefix(&root).ok()?.to_path_buf();
            let state = project.tracked_file_state(&path)?;
            Some((relative, state.baseline))
        })
        .collect()
}

fn checkpoint_payload_bytes(
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
) -> Result<u64, String> {
    files
        .values()
        .chain(text_base.iter().filter_map(|(path, bytes)| {
            bytes
                .as_ref()
                .filter(|bytes| files.get(path) != Some(*bytes))
        }))
        .try_fold(0u64, |total, bytes| total.checked_add(bytes.len() as u64))
        .ok_or_else(|| "检查点字节数超出可表示范围".into())
}

fn text_base_digest(text_base: Option<&BTreeMap<PathBuf, Option<Vec<u8>>>>) -> Option<String> {
    let text_base = text_base?;
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-text-base-v1");
    for (path, bytes) in text_base {
        let path = path.to_string_lossy().replace('\\', "/");
        mix(&mut hash, path.as_bytes());
        match bytes {
            Some(bytes) => {
                mix(&mut hash, b"present");
                mix(&mut hash, bytes);
            }
            None => mix(&mut hash, b"absent"),
        }
    }
    Some(format!("{hash:016x}"))
}

fn checkpoint_record_digest(manifest: &CheckpointManifest) -> String {
    if manifest.version == LEGACY_CHECKPOINT_FORMAT_VERSION {
        return manifest.snapshot_digest.clone();
    }
    let mut hash = 0xcbf29ce484222325u64;
    mix(&mut hash, b"worldline-checkpoint-record-v2");
    mix(&mut hash, manifest.snapshot_digest.as_bytes());
    mix(
        &mut hash,
        manifest
            .text_base_digest
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    format!("{hash:016x}")
}

fn make_manifest(
    label: Option<String>,
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    payload_bytes: u64,
) -> Result<CheckpointManifest, String> {
    let id = next_checkpoint_id();
    let created_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let entries = files
        .iter()
        .enumerate()
        .map(|(index, (path, bytes))| {
            validate_relative_file(path)?;
            Ok(CheckpointFileEntry {
                path: path.to_string_lossy().replace('\\', "/"),
                payload: format!("files/{index:08}.bin"),
                bytes: bytes.len() as u64,
                checksum: file_checksum(bytes),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let base_entries = text_base
        .iter()
        .enumerate()
        .map(|(index, (path, bytes))| {
            validate_relative_file(path)?;
            if !path.extension().is_some_and(|extension| extension == "wl") {
                return Err("检查点文本基线只能引用 .wl 文件".into());
            }
            let source = match bytes {
                None => CheckpointTextBaseSource::Absent,
                Some(bytes) if files.get(path) == Some(bytes) => CheckpointTextBaseSource::Snapshot,
                Some(bytes) => CheckpointTextBaseSource::Stored {
                    payload: format!("files/base-{index:08}.bin"),
                    bytes: bytes.len() as u64,
                    checksum: file_checksum(bytes),
                },
            };
            Ok(CheckpointTextBaseEntry {
                path: path.to_string_lossy().replace('\\', "/"),
                source,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(CheckpointManifest {
        version: CHECKPOINT_FORMAT_VERSION,
        id,
        label,
        created_at_unix_ms,
        payload_bytes,
        snapshot_digest: files_digest(files),
        files: entries,
        text_base: Some(base_entries),
        text_base_digest: text_base_digest(Some(text_base)),
    })
}

fn summary_for_manifest(manifest: &CheckpointManifest) -> CheckpointSummary {
    CheckpointSummary {
        id: manifest.id.clone(),
        label: manifest.label.clone(),
        created_at_unix_ms: manifest.created_at_unix_ms,
        file_count: manifest.files.len(),
        payload_bytes: manifest.payload_bytes,
        available: true,
        unavailable_reason: None,
    }
}

fn validate_checkpoint_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("检查点 ID 无效".into());
    }
    Ok(())
}

fn disk_workspace_files(root: &Path) -> Result<Files, String> {
    let paths = match crate::file_access::workspace_files(root) {
        Ok(paths) => paths,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Files::new()),
        Err(error) => return Err(format!("无法读取工作区文件清单：{error}")),
    };
    ensure_disk_file_limits(&paths, DEFAULT_MAX_CHECKPOINT_BYTES as u64)?;
    let mut files = Files::new();
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("工作区文件越界：{}", path.display()))?
            .to_path_buf();
        validate_relative_file(&relative)?;
        let bytes = crate::file_access::read(&path)
            .map_err(|error| format!("无法读取工作区文件 {}：{error}", path.display()))?;
        files.insert(relative, bytes);
    }
    validate_snapshot_files(&files)?;
    Ok(files)
}

#[cfg(not(target_arch = "wasm32"))]
fn files_to_pending(current: &Files, target: &Files) -> Vec<RestoreFile> {
    differing_paths(current, target)
        .into_iter()
        .map(|relative| RestoreFile {
            before: current.get(&relative).cloned(),
            after: target.get(&relative).cloned(),
            relative,
        })
        .collect()
}

fn differing_paths(current: &Files, target: &Files) -> BTreeSet<PathBuf> {
    current
        .keys()
        .chain(target.keys())
        .filter(|relative| current.get(*relative) != target.get(*relative))
        .cloned()
        .collect()
}

fn ensure_restore_targets_writable(
    project: &Project,
    checkpoint: &Files,
    changes: &[CheckpointFileChange],
    disk_changes: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    let manifest = checkpoint.get(Path::new(".world/project.json"));
    let target_registry = manifest
        .map(|bytes| crate::workspace_documents::parse_registry(&project.root, bytes))
        .unwrap_or_default();
    let mut paths = disk_changes.clone();
    paths.extend(changes.iter().map(|change| change.path.clone()));
    for relative in paths {
        let absolute = crate::compiler::source_path(&project.root.join(&relative));
        if project
            .authoring_documents
            .get(&absolute)
            .is_some_and(crate::workspace_documents::AuthoringDocument::is_read_only)
            || target_registry.read_only(&absolute)
            || checkpoint
                .get(&relative)
                .is_some_and(|bytes| crate::workspace_documents::document_read_only(bytes, false))
        {
            return Err(format!(
                "只读文件不能通过检查点恢复：{}",
                relative.display()
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if native_path_is_read_only(&project.root.join(&relative))? {
            return Err(format!(
                "只读文件或目录不能通过检查点恢复：{}",
                relative.display()
            ));
        }
    }
    Ok(())
}

static NEXT_CHECKPOINT: AtomicU64 = AtomicU64::new(0);

fn next_checkpoint_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_CHECKPOINT.fetch_add(1, Ordering::Relaxed);
    format!("cp-{millis:016x}-{sequence:08x}")
}

// Persistent native backend and module-lifetime browser backend are below.
#[cfg(not(target_arch = "wasm32"))]
include!("checkpoints_native.rs");

#[cfg(target_arch = "wasm32")]
include!("checkpoints_wasm.rs");

#[cfg(test)]
mod browser_checkpoint_scope_tests {
    use super::{make_manifest, CheckpointLimits, CheckpointScopeKey, InMemoryCheckpointStore};
    use crate::workspace_snapshot::Files;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn browser_checkpoint_history_isolated_by_session_at_a_shared_mount_root() {
        let files = Files::from([(PathBuf::from("world.wl"), b"event start\n".to_vec())]);
        let text_base = BTreeMap::new();
        let manifest = make_manifest(None, &files, &text_base, 12).unwrap();
        let checkpoint_id = manifest.id.clone();
        let limits = CheckpointLimits::default();
        let root = Path::new("/world");
        let session_a = CheckpointScopeKey::new(root, "browser-session-a");
        let session_b = CheckpointScopeKey::new(root, "browser-session-b");
        let same_session_after_reopen = CheckpointScopeKey::new(root, "browser-session-a");
        let mut store = InMemoryCheckpointStore::default();

        store
            .publish(session_a.clone(), manifest, &files, &text_base, &limits)
            .unwrap();

        assert_eq!(store.list(&session_a).len(), 1);
        assert!(store.list(&session_b).is_empty());
        assert!(store.load(&session_b, &checkpoint_id).is_err());
        assert_eq!(
            store
                .load(&same_session_after_reopen, &checkpoint_id)
                .unwrap()
                .files,
            files,
            "the same persisted browser session can reopen its own checkpoint"
        );
        assert!(store.delete(&session_b, &checkpoint_id).is_err());
        assert_eq!(store.list(&session_a).len(), 1);
        store.delete(&session_a, &checkpoint_id).unwrap();
        assert!(store.list(&session_a).is_empty());
    }
}

#[cfg(test)]
mod checkpoint_clock_tests {
    use super::{make_manifest, next_checkpoint_id};
    use crate::workspace_snapshot::Files;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn checkpoint_ids_are_unique_and_manifest_times_are_available() {
        let files = Files::from([(PathBuf::from("world.wl"), b"event start\n".to_vec())]);
        let text_base = BTreeMap::new();
        let first = make_manifest(None, &files, &text_base, 12).unwrap();
        let second = make_manifest(None, &files, &text_base, 12).unwrap();

        assert_ne!(next_checkpoint_id(), next_checkpoint_id());
        assert_ne!(first.id, second.id);
        assert!(first.created_at_unix_ms > 0);
        assert!(second.created_at_unix_ms > 0);
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_project_checkpoint_session_tests {
    use super::CheckpointLimits;
    use crate::project::Project;
    use crate::workspace_snapshot::{self, Files};
    use std::path::{Path, PathBuf};

    fn mount(files: &Files) {
        crate::file_access::mount(
            files
                .iter()
                .map(|(path, bytes)| (Path::new("/world").join(path), bytes.clone()))
                .collect(),
        );
    }

    #[test]
    fn same_saved_session_keeps_history_but_a_new_project_at_world_cannot_restore_it() {
        let files = Files::from([(
            PathBuf::from("world.wl"),
            "event start\n  Original.\n  -> END\n".as_bytes().to_vec(),
        )]);
        mount(&files);
        let project = Project::open(Path::new("/world/world.wl")).unwrap();
        let session_id = project.checkpoint_session_id().to_owned();
        let checkpoint = project
            .create_checkpoint(None, CheckpointLimits::default())
            .unwrap();
        assert!(checkpoint.created_at_unix_ms > 0);
        let plan = project.preview_checkpoint_restore(&checkpoint.id).unwrap();

        // Browser save/reopen restores the session identity alongside the package.
        mount(&files);
        let mut reopened = Project::open(Path::new("/world/world.wl")).unwrap();
        reopened
            .set_checkpoint_session_id(session_id.clone())
            .unwrap();
        assert_eq!(reopened.list_checkpoints().unwrap().len(), 1);
        reopened.restore_checkpoint(&plan).unwrap();

        // A second work can have the exact same mounted root and bytes; its fresh
        // Project identity still hides the old history and invalidates the plan.
        mount(&files);
        let mut other = Project::open(Path::new("/world/world.wl")).unwrap();
        assert_ne!(other.checkpoint_session_id(), session_id);
        assert!(other.list_checkpoints().unwrap().is_empty());
        let before = workspace_snapshot::snapshot_files(&other).unwrap();
        assert!(other.restore_checkpoint(&plan).is_err());
        assert_eq!(workspace_snapshot::snapshot_files(&other).unwrap(), before);
    }
}

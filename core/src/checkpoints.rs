//! 有界本地工程检查点与基于当前基线的受保护恢复。

use crate::catalog::TargetRef;
use crate::project::Project;
use crate::workspace_snapshot::Files;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
const MAX_CHECKPOINT_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(any(target_arch = "wasm32", test))]
const MAX_CHECKPOINT_SNAPSHOT_BYTES: usize = DEFAULT_MAX_CHECKPOINT_HISTORY_BYTES
    + DEFAULT_MAX_CHECKPOINTS * MAX_CHECKPOINT_MANIFEST_BYTES as usize
    + 1024 * 1024;
#[cfg(any(target_arch = "wasm32", test))]
const CHECKPOINT_SNAPSHOT_MAGIC: &[u8; 8] = b"WLCPST01";
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

#[derive(Clone, Debug)]
struct CheckpointListing {
    summary: CheckpointSummary,
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
        capture::ensure_workspace_snapshot_limits(self, limits.max_checkpoint_bytes as u64)?;
        let files = crate::workspace_snapshot::snapshot_files(self)?;
        capture::validate_snapshot_files(&files)?;
        let text_base = capture::saved_source_baselines(self);
        let payload_bytes = capture::checkpoint_payload_bytes(&files, &text_base)?;
        if files.len() > MAX_CHECKPOINT_FILES || text_base.len() > MAX_CHECKPOINT_FILES {
            return Err("工作区文件或保存基线数量超过检查点上限".into());
        }
        if payload_bytes > limits.max_checkpoint_bytes as u64 {
            return Err("检查点超过单条字节配额".into());
        }

        let manifest = capture::make_manifest(label, &files, &text_base, payload_bytes)?;
        history::publish_project_checkpoint(self, manifest.clone(), &files, &text_base, &limits)?;
        Ok(capture::summary_for_manifest(&manifest))
    }

    /// 校验所有完整记录后按创建时间倒序列举；受损记录会标记不可用。
    pub fn list_checkpoints(&self) -> Result<Vec<CheckpointSummary>, String> {
        let mut records = history::list_project_checkpoint_records(self)?;
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
        capture::validate_checkpoint_id(id)?;
        history::delete_project_checkpoint_record(self, id)
    }

    /// 创建绑定当前缓冲和完整磁盘快照的逐文件恢复计划。
    pub fn preview_checkpoint_restore(&self, id: &str) -> Result<CheckpointRestorePlan, String> {
        capture::validate_checkpoint_id(id)?;
        self.checkpoint_disk_baselines_match()?;
        let checkpoint = history::load_project_checkpoint(self, id)?;
        preview::preview_restore(self, checkpoint)
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
        let checkpoint = history::load_project_checkpoint(self, &plan.checkpoint_id)?;
        capture::ensure_workspace_snapshot_limits(self, DEFAULT_MAX_CHECKPOINT_BYTES as u64)?;
        let current_files = crate::workspace_snapshot::snapshot_files(self)?;
        capture::validate_snapshot_files(&current_files)?;
        let disk_files = preview::disk_workspace_files(&self.root)?;
        if capture::files_digest(&current_files) != plan.expected_workspace_digest
            || capture::files_digest(&disk_files) != plan.expected_disk_digest
            || capture::checkpoint_record_digest(&checkpoint.manifest) != plan.checkpoint_digest
        {
            return Err("StaleCheckpointPlan：工作区或检查点已变化，请重新预览".into());
        }
        let verified_plan = preview::preview_restore(self, checkpoint.clone())?;
        if &verified_plan != plan {
            return Err("CheckpointPlanMismatch：恢复计划与 core 预览不一致".into());
        }
        let disk_changes = restore::differing_paths(&disk_files, &checkpoint.files);
        self.ensure_workspace_writable()?;
        restore::ensure_restore_targets_writable(
            self,
            &checkpoint.files,
            &plan.changes,
            &disk_changes,
        )?;
        let mut candidate = self.clone();
        candidate.reset_to_checkpoint_files(&checkpoint.files)?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let pending = restore::files_to_pending(&disk_files, &checkpoint.files);
            native::persist_restored_files(&self.root, &pending, &checkpoint.files)?;
        }
        #[cfg(target_arch = "wasm32")]
        wasm::persist_restored_files(&self.root, &checkpoint.files)?;
        let restored_disk = preview::disk_workspace_files(&self.root)?;
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
pub(crate) fn next_checkpoint_session_id() -> String {
    format!("browser-{}", capture::next_checkpoint_id())
}

#[path = "checkpoints/capture.rs"]
mod capture;
#[path = "checkpoints/history.rs"]
mod history;
#[path = "checkpoints/preview.rs"]
mod preview;
#[path = "checkpoints/restore.rs"]
mod restore;
#[cfg(any(target_arch = "wasm32", test))]
#[path = "checkpoints/snapshot.rs"]
mod snapshot;
#[cfg(any(target_arch = "wasm32", test))]
#[path = "checkpoints/store.rs"]
mod store;
#[cfg(test)]
#[path = "checkpoints/tests.rs"]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
#[path = "checkpoints_native.rs"]
mod native;
#[cfg(target_arch = "wasm32")]
#[path = "checkpoints_wasm.rs"]
mod wasm;

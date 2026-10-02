//! 多文件文档缓冲、保存冲突检测与可移植目录导出。
use crate::compiler::{entry_path, source_path};
use crate::{CompileOptions, CompileResult, LanguageVersion};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
mod authoring_documents;
mod checkpoint_files;
mod compilation;
mod conflicts;
mod document_buffers;
mod export;
mod lifecycle;
mod save;
mod snapshot;
mod source_lifecycle;
mod source_navigation;

pub use snapshot::{SnapshotDocument, SnapshotState};

pub use crate::checkpoints::{
    CheckpointFileChange, CheckpointFileOperation, CheckpointLimits, CheckpointRestorePlan,
    CheckpointRestoreResult, CheckpointSummary, CheckpointTextDiff, CheckpointTextDiffSummary,
    CheckpointTextDifference, CheckpointTextSourceRange, CheckpointTextSourceSnippets,
};
pub use crate::workspace_documents::AuthoringDocument;

#[derive(Clone)]
pub struct Document {
    pub text: String,
    saved: Option<String>,
    deleted: bool,
}
impl Document {
    pub fn is_dirty(&self) -> bool {
        if self.deleted {
            self.saved.is_some()
        } else {
            self.saved.as_ref() != Some(&self.text)
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted
    }
}

#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub entry: PathBuf,
    pub documents: BTreeMap<PathBuf, Document>,
    pub authoring_documents: BTreeMap<PathBuf, AuthoringDocument>,
    authoring_diagnostics: Vec<crate::Diagnostic>,
    refresh_generation: u64,
    recovery_conflicts: Vec<PathBuf>,
    language_version: LanguageVersion,
    source_selection: Option<crate::source_config::SourceSelection>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) checkpoint_session_id: String,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub preview: String,
}

/// 一个文件的只读三方冲突快照。
///
/// `None` 表示对应一方不存在：例如本地删除的文件没有 `local`，
/// 外部删除的文件没有 `disk`。字节保持原样，因此坏 UTF-8 的展示文档
/// 也能交给上层显示或另存，不会在查询时被替换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSnapshot {
    pub path: PathBuf,
    pub baseline: Option<Vec<u8>>,
    pub local: Option<Vec<u8>>,
    pub disk: Option<Vec<u8>>,
}

/// 提案捕获使用的受控文件状态；只暴露 Project 已跟踪缓冲与其保存基线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedFileState {
    pub path: PathBuf,
    pub baseline: Option<Vec<u8>>,
    pub current: Option<Vec<u8>>,
    pub authoring: bool,
}

impl Project {
    #[cfg(not(target_arch = "wasm32"))]
    fn validate_destination(&self, destination: &Path) -> Result<(), String> {
        if source_path(destination).starts_with(&self.root) {
            return Err("另存或导出目标必须在当前工作区外".into());
        }
        Ok(())
    }

    pub(crate) fn ensure_workspace_writable(&self) -> Result<(), String> {
        if self.authoring_diagnostics.is_empty() {
            Ok(())
        } else {
            Err("工作区清单含有不支持的能力，只能只读查看".into())
        }
    }
}

fn validate_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path.extension().and_then(|e| e.to_str()) != Some("wl")
    {
        return Err("文件路径须为工程内的相对 .wl 路径,例如 events/harbor.wl".into());
    }
    Ok(())
}

fn read_conflict_disk(
    root: &Path,
    path: &Path,
    disk_files: &[PathBuf],
) -> Result<Option<Vec<u8>>, String> {
    let path = crate::file_access::within(root, path)?;
    if disk_files.binary_search(&path).is_err() {
        return Ok(None);
    }
    match crate::file_access::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取冲突文件：{error}")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_disk(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn validate_authoring_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.extension().and_then(|extension| extension.to_str()) != Some("json")
    {
        return Err("展示文档路径须为工程内的相对 .json 路径".into());
    }
    Ok(())
}

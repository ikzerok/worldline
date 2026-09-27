use super::store::{CheckpointScopeKey, InMemoryCheckpointStore};
use super::*;
use std::path::Path;

use std::cell::RefCell;

thread_local! {
    static WEB_CHECKPOINTS: RefCell<InMemoryCheckpointStore> =
        const { RefCell::new(InMemoryCheckpointStore { records: BTreeMap::new() }) };
}

pub(super) fn publish_checkpoint(
    root: &Path,
    session_id: &str,
    manifest: CheckpointManifest,
    files: &Files,
    text_base: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    limits: &CheckpointLimits,
) -> Result<(), String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| {
        storage
            .borrow_mut()
            .publish(scope, manifest, files, text_base, limits)
    })
}

pub(super) fn list_checkpoint_records(
    root: &Path,
    session_id: &str,
) -> Result<Vec<CheckpointListing>, String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| Ok(storage.borrow().list(&scope)))
}

pub(super) fn load_checkpoint(
    root: &Path,
    session_id: &str,
    id: &str,
) -> Result<CheckpointBundle, String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| storage.borrow().load(&scope, id))
}

pub(super) fn delete_checkpoint_record(
    root: &Path,
    session_id: &str,
    id: &str,
) -> Result<(), String> {
    let scope = CheckpointScopeKey::new(root, session_id);
    WEB_CHECKPOINTS.with(|storage| storage.borrow_mut().delete(&scope, id))
}

fn export_checkpoint_snapshot(
    scope: &CheckpointScopeKey,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    WEB_CHECKPOINTS.with(|storage| storage.borrow().export_snapshot(scope, max_bytes))
}

fn restore_checkpoint_snapshot(scope: &CheckpointScopeKey, bytes: &[u8]) -> Result<(), String> {
    WEB_CHECKPOINTS.with(|storage| storage.borrow_mut().import_snapshot(scope.clone(), bytes))
}

pub(super) fn persist_restored_files(root: &Path, target: &Files) -> Result<(), String> {
    crate::file_access::replace_workspace_files(root, target)
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

    /// 将当前浏览器会话的检查点内容编码为受限、可持久化快照。
    pub fn export_checkpoint_snapshot(&self, max_bytes: usize) -> Result<Vec<u8>, String> {
        let scope = CheckpointScopeKey::new(&self.root, &self.checkpoint_session_id);
        export_checkpoint_snapshot(&scope, max_bytes)
    }

    /// 在当前工程会话中恢复经校验的检查点快照；同 ID 冲突或超限时零写入。
    pub fn restore_checkpoint_snapshot(&self, bytes: &[u8]) -> Result<(), String> {
        let scope = CheckpointScopeKey::new(&self.root, &self.checkpoint_session_id);
        restore_checkpoint_snapshot(&scope, bytes)
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

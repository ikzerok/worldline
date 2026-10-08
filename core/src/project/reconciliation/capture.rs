use super::*;
use crate::workspace_documents::{
    document_read_only, manifest_path, parse_registry, parse_unique_json,
};
use std::collections::{BTreeMap, BTreeSet};

impl Project {
    pub fn capture_reconciliation(&self) -> Result<ReconciliationSession, String> {
        self.capture_reconciliation_with_progress(&mut |_| true)
    }

    pub fn capture_reconciliation_with_progress(
        &self,
        progress: Progress<'_>,
    ) -> Result<ReconciliationSession, String> {
        checkpoint(progress, ReconciliationStage::Capture)?;
        let guard = capture_guard(self, progress)?;
        let mut blockers = Vec::new();
        if let Err(error) = storage_ready(self) {
            blockers.push(error);
        }
        if let Err(error) = self.ensure_workspace_writable() {
            blockers.push(error);
        }
        let manifest = manifest_path(&self.root);
        if let Some(bytes) = guard.disk.get(&manifest) {
            let registry = parse_registry(&self.root, bytes);
            if !registry.diagnostics.is_empty() || registry.read_only(&manifest) {
                blockers.push("磁盘工作区清单无效或能力未知，不能用选择旧稿覆盖".into());
            }
        }
        let mut files = Vec::new();
        for buffer in &guard.buffers {
            let disk = guard.disk.get(&buffer.path).cloned();
            let local = (!buffer.deleted).then(|| buffer.retained.clone());
            if disk == buffer.saved {
                continue;
            }
            let path = relative(&self.root, &buffer.path)?;
            if local == buffer.saved {
                blockers.push(format!("请先刷新无本地修改的外改文件：{}", path.display()));
                continue;
            }
            let protected_reason = protected(buffer, disk.as_deref());
            files.push(ReconciliationFile {
                path,
                authoring: buffer.authoring,
                baseline: buffer.saved.clone(),
                local,
                disk,
                protected_reason,
            });
        }
        for path in guard.disk.keys() {
            if path.extension().is_some_and(|e| e == "wl") && !self.documents.contains_key(path) {
                blockers.push(format!(
                    "请先刷新新增源码：{}",
                    relative(&self.root, path)?.display()
                ));
            }
            if path == &manifest && !self.authoring_documents.contains_key(path) {
                blockers.push("请先刷新新增工作区清单".into());
            }
        }
        Ok(ReconciliationSession {
            schema_version: 1,
            workspace_root: self.root.clone(),
            entry: relative(&self.root, &self.entry)?,
            content_baseline: self.content_baseline(),
            session_digest: digest(&guard)?,
            files,
            blockers,
            guard,
        })
    }
}

pub(super) fn capture_guard(project: &Project, progress: Progress<'_>) -> Result<Guard, String> {
    if crate::compiler::source_path(&project.root) != project.root {
        return Err("工作区根目录身份已变化".into());
    }
    let disk = disk_files(project, progress)?;
    let mut buffers = Vec::new();
    let mut bytes = 0usize;
    for (path, document) in &project.documents {
        relative(&project.root, path)?;
        budget(&mut bytes, document.text.len())?;
        if let Some(saved) = &document.saved {
            budget(&mut bytes, saved.len())?;
        }
        buffers.push(BufferEvidence {
            path: path.clone(),
            authoring: false,
            retained: document.text.as_bytes().to_vec(),
            deleted: document.deleted,
            saved: document.saved.as_ref().map(|s| s.as_bytes().to_vec()),
            read_only: false,
        });
    }
    for (path, document) in &project.authoring_documents {
        relative(&project.root, path)?;
        budget(&mut bytes, document.bytes.len())?;
        if let Some(saved) = &document.saved {
            budget(&mut bytes, saved.len())?;
        }
        buffers.push(BufferEvidence {
            path: path.clone(),
            authoring: true,
            retained: document.bytes.clone(),
            deleted: document.deleted,
            saved: document.saved.clone(),
            read_only: document.read_only,
        });
    }
    if buffers.len() > MAX_FILES {
        return Err("外部改稿超过4096个受跟踪文件预算".into());
    }
    buffers.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Guard {
        root: project.root.clone(),
        entry: project.entry.clone(),
        generation: project.refresh_generation,
        buffers,
        disk,
        diagnostics: serde_json::to_vec(&project.authoring_diagnostics)
            .map_err(|e| e.to_string())?,
        language_version: project.language_version().into(),
        recovery_conflicts: project.recovery_conflicts.clone(),
    })
}

pub(super) fn disk_files(
    project: &Project,
    progress: Progress<'_>,
) -> Result<BTreeMap<std::path::PathBuf, Vec<u8>>, String> {
    let paths = match crate::file_access::workspace_files_limited(&project.root, MAX_FILES) {
        Ok(paths) => paths,
        Err(error) if root_is_missing(&project.root, &error) => Vec::new(),
        Err(error) => return Err(format!("无法捕获完整工作区：{error}")),
    };
    let mut disk = BTreeMap::new();
    let mut bytes = 0usize;
    let mut identities = BTreeSet::new();
    #[cfg(not(target_arch = "wasm32"))]
    let mut handles = std::collections::HashSet::new();
    for path in paths {
        checkpoint(progress, ReconciliationStage::Capture)?;
        let name = relative(&project.root, &path)?;
        if !identities.insert(name.to_string_lossy().to_lowercase()) {
            return Err("外部改稿库存存在大小写别名，无法确认唯一文件身份".into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let handle = same_file::Handle::from_path(&path)
                .map_err(|error| format!("无法确认文件身份：{error}"))?;
            if !handles.insert(handle) {
                return Err("外部改稿库存存在硬链接别名，无法确认唯一文件身份".into());
            }
        }
        let content = crate::file_access::read_limited(
            &path,
            MAX_FILE_BYTES.min(MAX_BYTES.saturating_sub(bytes)),
        )
        .map_err(|error| format!("无法读取外部改稿文件 {}：{error}", name.display()))?;
        budget(&mut bytes, content.len())?;
        disk.insert(path, content);
    }
    Ok(disk)
}

pub(super) fn budget(total: &mut usize, size: usize) -> Result<(), String> {
    *total = total.checked_add(size).ok_or("外部改稿字节预算溢出")?;
    if size > MAX_FILE_BYTES || *total > MAX_BYTES {
        return Err("外部改稿超过单项16 MiB或总计64 MiB预算".into());
    }
    Ok(())
}

pub(super) fn storage_ready(project: &Project) -> Result<(), String> {
    if !project.recovery_conflicts.is_empty() {
        return Err("尚有保存事务恢复冲突，请使用原事务救援流程".into());
    }
    #[cfg(not(target_arch = "wasm32"))]
    project.ensure_storage_ready()?;
    Ok(())
}

fn protected(buffer: &BufferEvidence, disk: Option<&[u8]>) -> Option<String> {
    if buffer.read_only {
        return Some("已注册文档为只读，原始字节仅供救援".into());
    }
    if let Err(error) = crate::source_lifecycle::safety::writable_path(&buffer.path) {
        return Some(error);
    }
    for bytes in [
        buffer.saved.as_deref(),
        (!buffer.deleted).then_some(buffer.retained.as_slice()),
        disk,
    ]
    .into_iter()
    .flatten()
    {
        if std::str::from_utf8(bytes).is_err() {
            return Some("存在非UTF-8原始字节，不能安全采纳；请保留副本后处理".into());
        }
        if buffer.authoring {
            if let Err(error) = supported_json(bytes) {
                return Some(error);
            }
        }
    }
    None
}

pub(super) fn supported_json(bytes: &[u8]) -> Result<(), String> {
    let value =
        parse_unique_json(bytes).map_err(|e| format!("展示文档不是完整无重复键JSON：{e}"))?;
    if !value.is_object()
        || value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
        || document_read_only(bytes, false)
    {
        return Err("展示文档格式或必需能力不受支持，只能保留原字节".into());
    }
    Ok(())
}

// 扫描中一个子目录消失不能被解释成整个工作区缺失。
pub(super) fn root_is_missing(root: &Path, error: &std::io::Error) -> bool {
    if error.kind() != std::io::ErrorKind::NotFound {
        return false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::fs::symlink_metadata(root)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = root;
        false
    }
}

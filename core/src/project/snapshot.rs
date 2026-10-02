use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_FILES: usize = 10_000;
const MAX_BYTES: usize = 128 * 1024 * 1024;

/// 隔离计算所需的精确跟踪集合，不包含原会话撤销历史。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotState {
    pub schema_version: u32,
    pub documents: Vec<SnapshotDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotDocument {
    pub path: PathBuf,
    pub authoring: bool,
    pub deleted: bool,
    pub read_only: bool,
    /// 墓碑保留原字节供内容基线重建；活动文件从文件侧通道取得。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_bytes: Option<Vec<u8>>,
}

impl Project {
    pub fn snapshot_files(&self) -> Result<crate::workspace_snapshot::Files, String> {
        crate::workspace_snapshot::snapshot_files(self)
    }

    pub fn snapshot_files_limited(
        &self,
        max_files: usize,
        max_bytes: usize,
    ) -> Result<crate::workspace_snapshot::Files, String> {
        crate::workspace_snapshot::snapshot_files_limited(self, max_files, max_bytes)
    }

    pub fn snapshot_state(&self) -> Result<SnapshotState, String> {
        if self
            .documents
            .len()
            .saturating_add(self.authoring_documents.len())
            > MAX_FILES
        {
            return Err("后台快照跟踪集合超过10000项限制".into());
        }
        let mut retained_size = 0usize;
        let mut documents = Vec::new();
        for (path, document) in &self.documents {
            if document.deleted {
                reserve_bytes(&mut retained_size, document.text.len())?;
            }
            documents.push(SnapshotDocument {
                path: relative(&self.root, path)?,
                authoring: false,
                deleted: document.deleted,
                read_only: false,
                retained_bytes: document.deleted.then(|| document.text.as_bytes().to_vec()),
            });
        }
        for (path, document) in &self.authoring_documents {
            if document.deleted {
                reserve_bytes(&mut retained_size, document.bytes.len())?;
            }
            documents.push(SnapshotDocument {
                path: relative(&self.root, path)?,
                authoring: true,
                deleted: document.deleted,
                read_only: document.read_only,
                retained_bytes: document.deleted.then(|| document.bytes.clone()),
            });
        }
        Ok(SnapshotState {
            schema_version: 1,
            documents,
        })
    }

    /// 从已取得的字节建立私有 Project，不恢复、刷新、保存或迁移磁盘。
    /// 浏览器调用方需先把相同活动文件挂到隔离 worker 的 file_access。
    pub fn from_snapshot(
        root: &Path,
        entry: &Path,
        files: &crate::workspace_snapshot::Files,
    ) -> Result<Self, String> {
        validate_files(files, None)?;
        let root = source_path(root);
        let entry = checked_relative(entry)?;
        if entry.extension().and_then(|value| value.to_str()) != Some("wl") {
            return Err("后台快照入口必须是相对 .wl 路径".into());
        }
        for path in files.keys() {
            checked_relative(path)?;
        }
        let registry = files
            .get(Path::new(crate::workspace_documents::MANIFEST_RELATIVE))
            .map(|bytes| crate::workspace_documents::parse_registry(&root, bytes))
            .unwrap_or_default();
        let mut project = Self {
            entry: root.join(entry),
            root: root.clone(),
            documents: BTreeMap::new(),
            authoring_documents: BTreeMap::new(),
            authoring_diagnostics: registry.diagnostics.clone(),
            refresh_generation: 0,
            recovery_conflicts: Vec::new(),
            language_version: registry.language_version,
            source_selection: registry.source_selection.clone(),
            #[cfg(target_arch = "wasm32")]
            checkpoint_session_id: crate::checkpoints::next_checkpoint_session_id(),
        };
        for (path, bytes) in files {
            let path = root.join(path);
            if path.extension().and_then(|value| value.to_str()) == Some("wl") {
                let text =
                    String::from_utf8(bytes.clone()).map_err(|_| "后台快照源码不是有效 UTF-8")?;
                project.documents.insert(
                    path,
                    Document {
                        saved: Some(text.clone()),
                        text,
                        deleted: false,
                    },
                );
            } else if registry.is_registered(&path) {
                let read_only = crate::workspace_documents::document_read_only(
                    bytes,
                    registry.read_only(&path),
                );
                project
                    .authoring_documents
                    .insert(path, AuthoringDocument::from_disk(bytes.clone(), read_only));
            }
        }
        for (path, read_only) in registry.documents {
            project
                .authoring_documents
                .entry(path)
                .or_insert_with(|| AuthoringDocument::missing(read_only));
        }
        Ok(project)
    }

    pub fn from_snapshot_with_state(
        root: &Path,
        entry: &Path,
        files: &crate::workspace_snapshot::Files,
        state: &SnapshotState,
    ) -> Result<Self, String> {
        if state.schema_version != 1 {
            return Err("不支持的后台快照状态版本".into());
        }
        validate_files(files, Some(state))?;
        let mut project = Self::from_snapshot(root, entry, files)?;
        let registry = project.authoring_documents.clone();
        project.documents.clear();
        project.authoring_documents.clear();
        let mut seen = BTreeSet::new();
        for item in &state.documents {
            let relative = checked_relative(&item.path)?;
            if !seen.insert(relative.clone()) {
                return Err("后台快照文档身份重复".into());
            }
            let extension = if item.authoring { "json" } else { "wl" };
            if relative.extension().and_then(|value| value.to_str()) != Some(extension) {
                return Err("后台快照文档类型与路径不一致".into());
            }
            let bytes = if item.deleted {
                if files.contains_key(&relative) {
                    return Err("后台快照墓碑仍包含活动文件".into());
                }
                item.retained_bytes
                    .as_ref()
                    .ok_or("后台快照墓碑缺少原字节")?
            } else {
                if item.retained_bytes.is_some() {
                    return Err("活动快照文档不能另带墓碑字节".into());
                }
                files.get(&relative).ok_or("后台快照文档缺少活动字节")?
            };
            let path = project.root.join(relative);
            if item.authoring {
                let inherited = registry.get(&path).is_some_and(|doc| doc.read_only);
                let read_only = item.read_only
                    || crate::workspace_documents::document_read_only(bytes, inherited);
                project.authoring_documents.insert(
                    path,
                    AuthoringDocument {
                        bytes: bytes.clone(),
                        saved: (!item.deleted).then(|| bytes.clone()),
                        deleted: item.deleted,
                        read_only,
                    },
                );
            } else {
                if item.read_only {
                    return Err("后台快照源码不能伪造展示文档只读标记".into());
                }
                let text =
                    String::from_utf8(bytes.clone()).map_err(|_| "后台快照源码不是有效 UTF-8")?;
                project.documents.insert(
                    path,
                    Document {
                        saved: (!item.deleted).then(|| text.clone()),
                        text,
                        deleted: item.deleted,
                    },
                );
            }
        }
        for path in files.keys() {
            if path.extension().and_then(|value| value.to_str()) == Some("wl")
                && !seen.contains(path)
            {
                return Err("后台快照跟踪集合遗漏源码".into());
            }
        }
        for path in registry.keys() {
            let relative = relative(&project.root, path)?;
            if !seen.contains(&relative) {
                return Err("后台快照跟踪集合遗漏已注册展示文档".into());
            }
        }
        Ok(project)
    }
}

fn reserve_bytes(used: &mut usize, bytes: usize) -> Result<(), String> {
    *used = used.checked_add(bytes).ok_or("后台快照字节数溢出")?;
    if *used > MAX_BYTES {
        return Err("后台快照超过128MiB总字节预算".into());
    }
    Ok(())
}

fn validate_files(
    files: &crate::workspace_snapshot::Files,
    state: Option<&SnapshotState>,
) -> Result<(), String> {
    let tombstones = state.map_or(0, |value| {
        value.documents.iter().filter(|item| item.deleted).count()
    });
    if files.len().saturating_add(tombstones) > MAX_FILES
        || state.is_some_and(|value| value.documents.len() > MAX_FILES)
    {
        return Err("后台快照超过10000项限制".into());
    }
    let mut used = 0usize;
    for bytes in files.values() {
        reserve_bytes(&mut used, bytes.len())?;
    }
    if let Some(state) = state {
        for document in &state.documents {
            if let Some(bytes) = &document.retained_bytes {
                reserve_bytes(&mut used, bytes.len())?;
            }
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let path = path
        .strip_prefix(root)
        .map_err(|_| "后台快照文档越出工作区")?;
    checked_relative(path)
}

fn checked_relative(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_str().ok_or("后台快照路径必须是 UTF-8")?;
    if text.is_empty()
        || text.contains([':', '\\'])
        || path.is_absolute()
        || text
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.starts_with(".world/.transactions")
        || path.starts_with(".world/.checkpoints")
    {
        return Err("后台快照路径不是安全工作区相对路径".into());
    }
    Ok(path.to_path_buf())
}

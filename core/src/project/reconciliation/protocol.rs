//! 一次性CLI的显式旧基线/本地稿输入；只建立隔离Project，不冒充已打开编辑器会话。
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationInput {
    pub schema_version: u32,
    pub files: Vec<ReconciliationInputFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciliationInputFile {
    pub path: PathBuf,
    pub baseline: Option<Vec<u8>>,
    pub local: Option<Vec<u8>>,
}

impl Project {
    /// 旧/本地稿是调用方明确提供的材料；磁盘侧始终重新读取，不由DTO宣称。
    pub fn open_reconciliation_input(
        path: &Path,
        input: &ReconciliationInput,
    ) -> Result<Self, String> {
        if input.schema_version != 1 || input.files.len() > MAX_FILES {
            return Err("外部改稿输入版本或文件数无效".into());
        }
        path_budget(path)?;
        let entry = crate::compiler::entry_path(path);
        let root = entry.parent().ok_or("工作区缺少根目录")?.to_path_buf();
        let placeholder = Self::new(&root);
        capture::storage_ready(&placeholder)?;
        let disk = capture::disk_files(&placeholder, &mut |_| true)?;
        let mut files = crate::workspace_snapshot::Files::new();
        for (path, bytes) in &disk {
            files.insert(relative(&root, path)?, bytes.clone());
        }
        let mut paths = BTreeSet::new();
        let mut used = 0;
        for file in &input.files {
            if relative(&root, &root.join(&file.path))? != file.path
                || !paths.insert(file.path.clone())
            {
                return Err("外部改稿输入路径重复或不是工作区内相对路径".into());
            }
            if !file
                .path
                .extension()
                .is_some_and(|e| e == "wl" || e == "json")
            {
                return Err("外部改稿输入只接受.wl或明确登记的JSON".into());
            }
            for bytes in [file.baseline.as_ref(), file.local.as_ref()]
                .into_iter()
                .flatten()
            {
                capture::budget(&mut used, bytes.len())?;
            }
            if let Some(bytes) = &file.local {
                files.insert(file.path.clone(), bytes.clone());
            } else {
                files.remove(&file.path);
            }
        }
        let manifest = crate::workspace_documents::manifest_path(&root);
        let manifest_relative = relative(&root, &manifest)?;
        let baseline_manifest = input
            .files
            .iter()
            .find(|file| file.path == manifest_relative)
            .map(|file| file.baseline.as_ref())
            .unwrap_or_else(|| disk.get(&manifest));
        let old_registry = baseline_manifest
            .map(|bytes| crate::workspace_documents::parse_registry(&root, bytes))
            .unwrap_or_default();
        let mut project = Self::from_snapshot(&root, &relative(&root, &entry)?, &files)?;
        for file in &input.files {
            let absolute = root.join(&file.path);
            if file.path.extension().is_some_and(|e| e == "wl") {
                let saved = file
                    .baseline
                    .as_ref()
                    .map(|bytes| String::from_utf8(bytes.clone()))
                    .transpose()
                    .map_err(|_| "显式旧源码基线不是UTF-8")?;
                let text = file
                    .local
                    .as_ref()
                    .or(file.baseline.as_ref())
                    .map(|bytes| String::from_utf8(bytes.clone()))
                    .transpose()
                    .map_err(|_| "显式本地源码不是UTF-8")?
                    .unwrap_or_default();
                project.documents.insert(
                    absolute,
                    Document {
                        text,
                        saved,
                        deleted: file.local.is_none(),
                    },
                );
            } else {
                if absolute != manifest
                    && !old_registry.is_registered(&absolute)
                    && !project.authoring_documents.contains_key(&absolute)
                {
                    return Err("不能把未登记的普通JSON作为冲突输入接管".into());
                }
                let read_only = old_registry.read_only(&absolute)
                    || project
                        .authoring_documents
                        .get(&absolute)
                        .is_some_and(|doc| doc.read_only);
                project.authoring_documents.insert(
                    absolute,
                    AuthoringDocument {
                        bytes: file
                            .local
                            .as_ref()
                            .or(file.baseline.as_ref())
                            .cloned()
                            .unwrap_or_default(),
                        saved: file.baseline.clone(),
                        deleted: file.local.is_none(),
                        read_only,
                    },
                );
            }
        }
        capture::capture_guard(&project, &mut |_| true)?;
        Ok(project)
    }
}

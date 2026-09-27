use super::*;

impl Project {
    /// 检查点写入前逐一确认所有受控文档仍符合保存基线。
    pub(crate) fn checkpoint_disk_baselines_match(&self) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        self.ensure_storage_ready()?;
        if !self.recovery_conflicts.is_empty() {
            return Err("工程存在未解决的保存事务冲突，不能创建或恢复检查点".into());
        }
        let disk_files = match crate::file_access::workspace_files(&self.root) {
            Ok(paths) => paths,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("无法读取检查点工作区：{error}")),
        };
        for (path, document) in &self.documents {
            let disk = read_conflict_disk(&self.root, path, &disk_files)?;
            let baseline = document.saved.as_ref().map(|text| text.as_bytes().to_vec());
            if disk != baseline {
                return Err(format!(
                    "{} 已被外部修改或删除，不能按旧检查点覆盖",
                    path.display()
                ));
            }
        }
        for (path, document) in &self.authoring_documents {
            let disk = read_conflict_disk(&self.root, path, &disk_files)?;
            if disk != document.saved {
                return Err(format!(
                    "{} 已被外部修改或删除，不能按旧检查点覆盖",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    /// 恢复事务成功后，以精确快照更新内存缓冲，并保留删除墓碑供撤销使用。
    pub(crate) fn reset_to_checkpoint_files(
        &mut self,
        files: &crate::workspace_snapshot::Files,
    ) -> Result<(), String> {
        let mut documents = BTreeMap::new();
        for (relative, bytes) in files {
            if !relative
                .extension()
                .is_some_and(|extension| extension == "wl")
            {
                continue;
            }
            let text = String::from_utf8(bytes.clone())
                .map_err(|error| format!("检查点源码不是有效 UTF-8：{error}"))?;
            let path = crate::compiler::source_path(&self.root.join(relative));
            documents.insert(
                path,
                Document {
                    saved: Some(text.clone()),
                    text,
                    deleted: false,
                },
            );
        }
        for (path, previous) in &self.documents {
            if documents.contains_key(path) {
                continue;
            }
            let mut tombstone = previous.clone();
            tombstone.saved = None;
            tombstone.deleted = true;
            documents.insert(path.clone(), tombstone);
        }

        let registry = files
            .get(Path::new(".world/project.json"))
            .map(|bytes| crate::workspace_documents::parse_registry(&self.root, bytes))
            .unwrap_or_default();
        let mut authoring_documents = BTreeMap::new();
        for (path, registered_read_only) in &registry.documents {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| format!("展示文档路径越出工作区：{}", path.display()))?;
            let document = match files.get(relative) {
                Some(bytes) => AuthoringDocument::from_disk(
                    bytes.clone(),
                    *registered_read_only
                        || crate::workspace_documents::document_read_only(bytes, false),
                ),
                None => AuthoringDocument::missing(*registered_read_only),
            };
            authoring_documents.insert(path.clone(), document);
        }
        self.documents = documents;
        self.authoring_documents = authoring_documents;
        self.authoring_diagnostics = registry.diagnostics;
        self.language_version = registry.language_version;
        self.source_selection = registry.source_selection;
        self.recovery_conflicts.clear();
        Ok(())
    }
}

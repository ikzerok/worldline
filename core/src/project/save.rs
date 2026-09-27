use super::*;

impl Project {
    /// 另存工程保留未完成的源码,不要求编译通过。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_as(&mut self, destination: &Path) -> Result<(), String> {
        self.ensure_storage_ready()?;
        if destination.exists() {
            return Err("目标文件夹已存在,请选择新名称".into());
        }
        self.validate_destination(destination)?;
        if self.root.exists() {
            self.refresh()?;
        }
        if self.authoring_diagnostics.is_empty() {
            self.migrate_permissions()?;
        }
        let root = source_path(destination);
        let portable = self.portable_assets()?;
        let mut documents = BTreeMap::new();
        for (path, document) in &self.documents {
            if document.deleted {
                continue;
            }
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| "请先把目录外的引用移入工程再另存")?;
            documents.insert(
                root.join(relative),
                Document {
                    text: portable
                        .sources
                        .get(path)
                        .cloned()
                        .unwrap_or_else(|| document.text.clone()),
                    saved: None,
                    deleted: false,
                },
            );
        }
        let mut authoring_documents = BTreeMap::new();
        for (path, document) in &self.authoring_documents {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| "请先把目录外的引用移入工程再另存")?;
            authoring_documents.insert(
                root.join(relative),
                AuthoringDocument {
                    bytes: portable
                        .authoring
                        .get(path)
                        .cloned()
                        .unwrap_or_else(|| document.bytes.clone()),
                    saved: None,
                    deleted: document.deleted,
                    read_only: document.read_only,
                },
            );
        }
        let mut candidate = Self {
            entry: root.join(
                self.entry
                    .strip_prefix(&self.root)
                    .map_err(|e| e.to_string())?,
            ),
            root,
            documents,
            authoring_documents,
            authoring_diagnostics: self.authoring_diagnostics.clone(),
            refresh_generation: 0,
            recovery_conflicts: Vec::new(),
            language_version: self.language_version,
            source_selection: self.source_selection.clone(),
        };
        candidate.save_buffers(true)?;
        for (relative, source) in portable.copies {
            let path = candidate.root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::copy(source, path).map_err(|e| e.to_string())?;
        }
        *self = candidate;
        Ok(())
    }
    /// 先检查所有修改文件，再以可恢复事务逐文件替换；跨文件不宣称原子性。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&mut self) -> Result<(), String> {
        self.save_buffers(false)
    }

    /// 将普通附件与当前 Project 文档放在同一可恢复保存事务中。
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn save_with_additional_files(
        &mut self,
        files: &[(PathBuf, Vec<u8>)],
    ) -> Result<(), String> {
        self.save_buffers_with_additional_files(false, files)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn ensure_storage_ready(&self) -> Result<(), String> {
        if !self.recovery_conflicts.is_empty()
            || crate::storage::has_unresolved_transactions(&self.root)?
        {
            return Err("工程存在未解决的保存事务，请重新打开并处理冲突".into());
        }
        Ok(())
    }

    // 只有另存到新的独立目录，才允许复制未知格式的原始字节。
    #[cfg(not(target_arch = "wasm32"))]
    fn save_buffers(&mut self, copying_to_new_directory: bool) -> Result<(), String> {
        self.save_buffers_with_additional_files(copying_to_new_directory, &[])
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_buffers_with_additional_files(
        &mut self,
        copying_to_new_directory: bool,
        additional_files: &[(PathBuf, Vec<u8>)],
    ) -> Result<(), String> {
        self.ensure_storage_ready()?;
        if self.documents.values().any(Document::is_dirty)
            || self
                .authoring_documents
                .values()
                .any(AuthoringDocument::is_dirty)
            || !additional_files.is_empty()
        {
            self.preflight_save(copying_to_new_directory)?;
        }

        let mut pending = Vec::new();
        for (path, document) in self.documents.iter().filter(|(_, d)| d.is_dirty()) {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| format!("文件不在工程目录内:{}", path.display()))?
                .to_path_buf();
            pending.push(crate::storage::PendingFile {
                relative,
                before: document.saved.as_ref().map(|text| text.as_bytes().to_vec()),
                after: (!document.deleted).then(|| document.text.as_bytes().to_vec()),
            });
        }
        for (path, document) in self
            .authoring_documents
            .iter()
            .filter(|(_, document)| document.is_dirty())
        {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| format!("文件不在工程目录内:{}", path.display()))?
                .to_path_buf();
            pending.push(crate::storage::PendingFile {
                relative,
                before: document.saved.clone(),
                after: (!document.deleted).then(|| document.bytes.clone()),
            });
        }
        let mut additional_paths = std::collections::BTreeSet::new();
        for (relative, bytes) in additional_files {
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(format!("普通附件目标路径无效：{}", relative.display()));
            }
            if !additional_paths.insert(relative.clone())
                || pending.iter().any(|file| file.relative == *relative)
            {
                return Err(format!(
                    "保存事务中存在重复目标路径：{}",
                    relative.display()
                ));
            }
            let target = crate::file_access::within(&self.root, &self.root.join(relative))?;
            if read_disk(&target)?.is_some() {
                return Err(format!("普通附件目标已存在：{}", target.display()));
            }
            pending.push(crate::storage::PendingFile {
                relative: relative.clone(),
                before: None,
                after: Some(bytes.clone()),
            });
        }
        if pending.is_empty() {
            return Ok(());
        }
        crate::storage::save(&self.root, &pending)?;

        // 事务提交并清理成功后才一次性推进所有文档的保存基线。
        for document in self.documents.values_mut().filter(|d| d.is_dirty()) {
            document.saved = (!document.deleted).then(|| document.text.clone());
        }
        for document in self
            .authoring_documents
            .values_mut()
            .filter(|document| document.is_dirty())
        {
            document.saved = (!document.deleted).then(|| document.bytes.clone());
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn preflight_save(&self, copying_to_new_directory: bool) -> Result<(), String> {
        for (path, document) in self.documents.iter().filter(|(_, d)| d.is_dirty()) {
            crate::file_access::within(&self.root, path)?;
            let current = read_disk(path)?;
            let expected = document.saved.as_ref().map(|text| text.as_bytes().to_vec());
            if current != expected {
                return Err(format!(
                    "{} 已被外部修改或无法读取;请导出副本后合并,避免覆盖他人修改",
                    path.display()
                ));
            }
        }
        for (path, document) in self
            .authoring_documents
            .iter()
            .filter(|(_, document)| document.is_dirty())
        {
            if document.read_only && !copying_to_new_directory {
                return Err("展示文档已变为只读，请另存副本保留本地修改".into());
            }
            crate::file_access::within(&self.root, path)?;
            let current = read_disk(path)?;
            if current != document.saved {
                return Err(format!(
                    "{} 已被外部修改或无法读取;请导出副本后合并,避免覆盖他人修改",
                    path.display()
                ));
            }
        }
        Ok(())
    }
}

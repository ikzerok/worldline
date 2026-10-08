use super::*;

impl Project {
    pub fn document(&self, path: &Path) -> Result<&str, String> {
        self.documents
            .get(&source_path(path))
            .filter(|document| !document.is_deleted())
            .map(|document| document.text.as_str())
            .ok_or_else(|| format!("文件未载入:{}", path.display()))
    }

    /// 检索当前工程缓冲,每个命中行返回一次;列号按 Unicode 字符计数。
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        if query.is_empty() {
            return Vec::new();
        }
        self.documents
            .iter()
            .filter(|(_, document)| !document.is_deleted())
            .flat_map(|(file, document)| {
                document
                    .text
                    .lines()
                    .enumerate()
                    .filter_map(move |(line, text)| {
                        text.find(query).map(|offset| SearchHit {
                            file: file.clone(),
                            line: line as u32 + 1,
                            column: text[..offset].chars().count() as u32 + 1,
                            preview: text.into(),
                        })
                    })
            })
            .collect()
    }

    pub fn compile(&mut self) -> CompileResult {
        let result = self.compile_current();
        for (path, text) in &result.sources {
            self.documents
                .entry(path.clone())
                .or_insert_with(|| Document {
                    text: text.clone(),
                    saved: Some(text.clone()),
                    deleted: false,
                });
        }
        result
    }

    pub fn is_dirty(&self) -> bool {
        self.documents.values().any(Document::is_dirty)
            || self
                .authoring_documents
                .values()
                .any(AuthoringDocument::is_dirty)
    }

    /// 外部存储成功接收所有缓冲后推进保存基线；不进行磁盘写入。
    pub fn mark_saved(&mut self) {
        for document in self.documents.values_mut() {
            if document.deleted {
                document.saved = None;
            } else {
                document.saved = Some(document.text.clone());
            }
        }
        for document in self.authoring_documents.values_mut() {
            if document.deleted {
                document.saved = None;
            } else {
                document.saved = Some(document.bytes.clone());
            }
        }
    }

    /// 外部刷新使旧撤销快照失效；拒绝时保持当前工程不变并返回 false。
    pub fn restore(&mut self, mut previous: Self) -> bool {
        if self.root != previous.root || self.refresh_generation != previous.refresh_generation {
            return false;
        }
        for (path, current) in &self.documents {
            if !previous.documents.contains_key(path) {
                let mut deleted = current.clone();
                deleted.deleted = true;
                previous.documents.insert(path.clone(), deleted);
            }
        }
        for (path, current) in &self.authoring_documents {
            if !previous.authoring_documents.contains_key(path) {
                let mut deleted = current.clone();
                deleted.deleted = true;
                previous.authoring_documents.insert(path.clone(), deleted);
            }
        }
        for (path, document) in &mut previous.documents {
            if let Some(current) = self.documents.get(path) {
                document.saved = current.saved.clone();
            }
        }
        for (path, document) in &mut previous.authoring_documents {
            if let Some(current) = self.authoring_documents.get(path) {
                document.saved = current.saved.clone();
            }
        }
        // 外部元数据观测不属于作者撤销；旧快照不能恢复旧附件可用性。
        previous.query_observation = self.query_observation.clone();
        *self = previous;
        true
    }
    pub fn set_text(&mut self, path: &Path, text: String) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        let document = self
            .documents
            .get_mut(&source_path(path))
            .ok_or("文件未载入")?;
        if document.deleted {
            return Err("文件已标记删除,请先恢复工程快照".into());
        }
        document.text = text;
        Ok(())
    }

    /// 新建活动源码、清单成员和入口引用一次提交；失败不保留孤立缓冲。
    pub fn add_file(&mut self, relative: &Path) -> Result<PathBuf, String> {
        self.ensure_workspace_writable()?;
        self.source_lifecycle_disk_baselines_match()?;
        let path = crate::source_lifecycle::safety::native_destination(self, relative)?;
        let mut candidate = self.clone();
        candidate.add_file_candidate(relative, &path)?;
        *self = candidate;
        Ok(path)
    }

    /// 调用方持有已完整验证的私有候选；失败时必须丢弃候选。
    pub(crate) fn add_file_candidate(
        &mut self,
        relative: &Path,
        path: &Path,
    ) -> Result<(), String> {
        self.documents.insert(
            path.to_path_buf(),
            Document {
                text: "// 在此文件编写事件,ID 在工程内唯一。\n".into(),
                saved: None,
                deleted: false,
            },
        );
        self.add_active_source(relative)?;
        self.include_file_in_memory(path)?;
        crate::source_lifecycle::safety::writable_path(&self.entry)?;
        crate::source_lifecycle::safety::writable_path(path)?;
        if self.source_selection.is_some() {
            crate::source_lifecycle::safety::writable_path(
                &crate::workspace_documents::manifest_path(&self.root),
            )?;
        }
        crate::source_lifecycle::safety::buffer_budget(self)?;
        Ok(())
    }

    /// 引用既有活动源码；归档或非活动文件不会被此操作暗中启用。
    pub fn include_file(&mut self, path: &Path) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        self.source_lifecycle_disk_baselines_match()?;
        let mut candidate = self.clone();
        candidate.include_file_in_memory(path)?;
        crate::source_lifecycle::safety::writable_path(&self.entry)?;
        crate::source_lifecycle::safety::buffer_budget(&candidate)?;
        *self = candidate;
        Ok(())
    }

    fn include_file_in_memory(&mut self, path: &Path) -> Result<(), String> {
        let path = crate::file_access::within(&self.root, path)?;
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "请先把文件放入工程文件夹,再添加引用")?;
        crate::source_lifecycle::safety::native_relative(relative)?;
        if path == self.entry {
            return Err("总入口不能引用自身".into());
        }
        if self
            .source_selection
            .as_ref()
            .is_some_and(|set| !set.is_active(&path))
        {
            return Err("引用目标为归档或非活动源码；请先另行明确启用，工程未修改".into());
        }
        if self.documents.get(&path).is_some_and(Document::is_deleted) {
            return Err("引用目标已标记删除，工程未修改".into());
        }
        if !self.documents.contains_key(&path) {
            let bytes = crate::source_lifecycle::resources::disk_bytes(self, &path)?;
            let text =
                String::from_utf8(bytes).map_err(|error| format!("引用源码不是 UTF-8：{error}"))?;
            self.documents.insert(
                path.clone(),
                Document {
                    saved: Some(text.clone()),
                    text,
                    deleted: false,
                },
            );
        }
        let relative = relative.to_string_lossy().replace('\\', "/");
        let entry = self.documents.get_mut(&self.entry).ok_or("总入口未载入")?;
        if entry.deleted {
            return Err("总入口已标记删除，工程未修改".into());
        }
        entry.text.push_str(&format!(
            "\ninclude {}\n",
            crate::authoring::quote(&relative)
        ));
        Ok(())
    }

    fn add_active_source(&mut self, relative: &Path) -> Result<(), String> {
        if self.source_selection.is_none() {
            return Ok(());
        }
        let manifest = crate::workspace_documents::manifest_path(&self.root);
        let document = self.authoring_document(&manifest)?;
        let mut value = crate::workspace_documents::parse_unique_json(document.bytes())?;
        let active = value
            .get_mut("source_config")
            .and_then(|config| config.get_mut("active"))
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("显式源码清单缺少 active 数组")?;
        active.push(serde_json::Value::String(
            relative.to_string_lossy().replace('\\', "/"),
        ));
        self.set_authoring_document(
            &manifest,
            serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
        )?;
        self.ensure_workspace_writable()
    }
}

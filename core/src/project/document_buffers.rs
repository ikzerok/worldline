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

    pub fn add_file(&mut self, relative: &Path) -> Result<PathBuf, String> {
        self.ensure_workspace_writable()?;
        validate_relative(relative)?;
        let path = crate::file_access::within(&self.root, &self.root.join(relative))?;
        if self.documents.contains_key(&path) || path.exists() {
            return Err("文件已存在,请使用引用文件或更换名称".into());
        }
        self.documents.insert(
            path.clone(),
            Document {
                text: "// 在此文件编写事件,ID 在工程内唯一。\n".into(),
                saved: None,
                deleted: false,
            },
        );
        self.include_file(&path)?;
        Ok(path)
    }

    pub fn include_file(&mut self, path: &Path) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        let path = crate::file_access::within(&self.root, path)?;
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "请先把文件放入工程文件夹,再添加引用")?;
        validate_relative(relative)?;
        if path == self.entry {
            return Err("总入口不能引用自身".into());
        }
        if !self.documents.contains_key(&path) {
            let text = crate::file_access::read_to_string(&path).map_err(|e| e.to_string())?;
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
        entry.text.push_str(&format!(
            "\ninclude {}\n",
            crate::authoring::quote(&relative)
        ));
        Ok(())
    }
}

use super::*;

impl Project {
    pub fn authoring_document(&self, path: &Path) -> Result<&AuthoringDocument, String> {
        self.authoring_documents
            .get(&source_path(path))
            .ok_or_else(|| format!("展示文档未注册:{}", path.display()))
    }

    /// 返回最近一次清单读取产生的注册诊断，供工作区诊断层继续投影。
    pub fn authoring_diagnostics(&self) -> &[crate::Diagnostic] {
        &self.authoring_diagnostics
    }

    /// 显式创建清单或清单已注册的新文档，不接管普通 JSON 文件。
    pub fn create_authoring_document(&mut self, path: &Path, bytes: Vec<u8>) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        let path = source_path(path);
        crate::file_access::within(&self.root, &path)?;
        let manifest = crate::workspace_documents::manifest_path(&self.root);
        if self
            .authoring_documents
            .get(&path)
            .is_some_and(|document| !document.deleted)
        {
            return Err("展示文档已存在".into());
        }
        if path != manifest {
            let document = self.authoring_document(&path)?;
            if document.read_only {
                return Err("展示文档格式或能力未知，只能只读查看".into());
            }
        }
        match crate::file_access::read(&path) {
            Ok(_) => return Err("目标文件已存在，不能覆盖未载入的文件".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let read_only = crate::workspace_documents::document_read_only(&bytes, false);
        if read_only {
            return Err("不能创建当前工具不支持的展示文档格式或必需能力".into());
        }
        self.authoring_documents.insert(
            path.clone(),
            AuthoringDocument {
                bytes,
                saved: None,
                deleted: false,
                read_only,
            },
        );
        if path == manifest {
            self.update_authoring_registry();
        }
        Ok(())
    }

    fn update_authoring_registry(&mut self) {
        let manifest = crate::workspace_documents::manifest_path(&self.root);
        let Some(document) = self
            .authoring_documents
            .get(&manifest)
            .filter(|d| !d.deleted)
        else {
            self.language_version = LanguageVersion::V1_9;
            self.source_selection = None;
            self.authoring_diagnostics.clear();
            return;
        };
        let registry = crate::workspace_documents::parse_registry(&self.root, &document.bytes);
        self.language_version = registry.language_version;
        self.source_selection = registry.source_selection.clone();
        self.authoring_diagnostics = registry.diagnostics;
        for (path, inherited) in registry.documents {
            let document = self
                .authoring_documents
                .entry(path)
                .or_insert_with(|| AuthoringDocument::missing(inherited));
            document.read_only =
                crate::workspace_documents::document_read_only(&document.bytes, inherited);
        }
    }

    pub fn set_authoring_document(&mut self, path: &Path, bytes: Vec<u8>) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        let path = source_path(path);
        let document = self
            .authoring_documents
            .get_mut(&path)
            .ok_or_else(|| format!("展示文档未注册:{}", path.display()))?;
        if document.read_only {
            return Err("展示文档格式或能力未知，只能只读查看".into());
        }
        if crate::workspace_documents::document_read_only(&bytes, false) {
            return Err("不能写入当前工具不支持的展示文档格式或必需能力".into());
        }
        document.bytes = bytes;
        document.deleted = false;
        document.read_only = crate::workspace_documents::document_read_only(&document.bytes, false);
        if path == crate::workspace_documents::manifest_path(&self.root) {
            self.update_authoring_registry();
        }
        Ok(())
    }

    pub fn delete_authoring_document(&mut self, path: &Path) -> Result<(), String> {
        let path = source_path(path);
        self.ensure_workspace_writable()?;
        {
            let document = self
                .authoring_documents
                .get_mut(&path)
                .ok_or_else(|| format!("展示文档未注册:{}", path.display()))?;
            if document.read_only {
                return Err("展示文档格式或能力未知，只能只读查看".into());
            }
            document.deleted = true;
        }
        if path == crate::workspace_documents::manifest_path(&self.root) {
            self.update_authoring_registry();
        }
        Ok(())
    }

    pub fn delete_document(&mut self, path: &Path) -> Result<(), String> {
        self.ensure_workspace_writable()?;
        let path = source_path(path);
        if let Some(document) = self.documents.get_mut(&path) {
            document.deleted = true;
            return Ok(());
        }
        self.delete_authoring_document(&path)
    }
}

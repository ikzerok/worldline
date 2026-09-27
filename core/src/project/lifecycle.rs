use super::*;

impl Project {
    pub fn refresh(&mut self) -> Result<Vec<PathBuf>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        let recovery = crate::storage::recover(&self.root)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.recovery_conflicts = recovery.conflicts.clone();
        }
        let paths = crate::file_access::workspace_files(&self.root).map_err(|e| e.to_string())?;
        let mut disk_sources = BTreeMap::new();
        for path in &paths {
            if path.extension().is_some_and(|e| e == "wl") {
                let text = crate::file_access::read_to_string(path).map_err(|e| e.to_string())?;
                disk_sources.insert(path.clone(), text);
            }
        }

        let manifest = crate::workspace_documents::manifest_path(&self.root);
        let manifest_bytes = match self.authoring_documents.get(&manifest) {
            Some(document) if document.is_dirty() => {
                (!document.deleted).then(|| document.bytes.clone())
            }
            _ if paths.contains(&manifest) => {
                Some(crate::file_access::read(&manifest).map_err(|error| error.to_string())?)
            }
            _ => None,
        };
        let registry = manifest_bytes
            .as_deref()
            .map(|bytes| crate::workspace_documents::parse_registry(&self.root, bytes))
            .unwrap_or_default();
        self.language_version = registry.language_version;
        self.source_selection = registry.source_selection.clone();
        self.authoring_diagnostics = registry.diagnostics.clone();

        let tracked_paths: std::collections::BTreeSet<_> = registry
            .documents
            .keys()
            .chain(self.authoring_documents.keys())
            .collect();
        let disk_authoring = tracked_paths
            .into_iter()
            .filter(|path| paths.binary_search(path).is_ok())
            .map(|path| crate::file_access::read(path).map(|bytes| (path.clone(), bytes)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(|error| error.to_string())?;

        #[cfg(not(target_arch = "wasm32"))]
        self.reconcile_recovered_documents(&recovery.recovered, &disk_sources, &disk_authoring);

        let external_change = self
            .documents
            .iter()
            .any(|(path, document)| document.saved.as_ref() != disk_sources.get(path))
            || disk_sources
                .keys()
                .any(|path| !self.documents.contains_key(path))
            || self
                .authoring_documents
                .iter()
                .any(|(path, document)| document.saved.as_ref() != disk_authoring.get(path))
            || (disk_authoring.contains_key(&manifest)
                && !self.authoring_documents.contains_key(&manifest));
        if external_change {
            self.refresh_generation = self.refresh_generation.wrapping_add(1);
        }

        let mut conflicts = Vec::new();
        self.documents.retain(|path, document| {
            if document.deleted {
                if document.is_dirty() {
                    if document.saved.as_ref() != disk_sources.get(path) {
                        conflicts.push(path.clone());
                    }
                } else if let Some(text) = disk_sources.get(path) {
                    document.text = text.clone();
                    document.saved = Some(text.clone());
                    document.deleted = false;
                }
                disk_sources.remove(path);
                return true;
            }
            if document.is_dirty() {
                if disk_sources.get(path) != document.saved.as_ref() {
                    conflicts.push(path.clone());
                }
                disk_sources.remove(path);
                return true;
            }
            match disk_sources.remove(path) {
                Some(text) => {
                    document.text = text.clone();
                    document.saved = Some(text);
                    true
                }
                None => false,
            }
        });
        for (path, text) in disk_sources {
            self.documents.insert(
                path,
                Document {
                    saved: Some(text.clone()),
                    text,
                    deleted: false,
                },
            );
        }

        let mut disk_authoring = disk_authoring;
        self.authoring_documents.retain(|path, document| {
            let registered = registry.is_registered(path);
            document.read_only = crate::workspace_documents::document_read_only(
                &document.bytes,
                registry.read_only(path),
            ) || disk_authoring
                .get(path)
                .is_some_and(|bytes| crate::workspace_documents::document_read_only(bytes, false));
            if !registered && !document.is_dirty() {
                return false;
            }
            if document.deleted {
                if document.is_dirty() {
                    if document.saved.as_ref() != disk_authoring.get(path) {
                        conflicts.push(path.clone());
                    }
                } else if let Some(bytes) = disk_authoring.get(path) {
                    document.bytes = bytes.clone();
                    document.saved = Some(bytes.clone());
                    document.deleted = false;
                    document.read_only = crate::workspace_documents::document_read_only(
                        bytes,
                        registry.read_only(path),
                    );
                }
                disk_authoring.remove(path);
                return true;
            }
            if document.is_dirty() {
                if disk_authoring.get(path) != document.saved.as_ref() {
                    conflicts.push(path.clone());
                }
                disk_authoring.remove(path);
                return true;
            }
            match disk_authoring.remove(path) {
                Some(bytes) => {
                    document.bytes = bytes.clone();
                    document.saved = Some(bytes.clone());
                    document.read_only = crate::workspace_documents::document_read_only(
                        &bytes,
                        registry.read_only(path),
                    );
                    true
                }
                None => false,
            }
        });
        for (path, bytes) in disk_authoring {
            if !registry.is_registered(&path) {
                continue;
            }
            self.authoring_documents.insert(
                path.clone(),
                AuthoringDocument::from_disk(
                    bytes.clone(),
                    registry.read_only(&path)
                        || crate::workspace_documents::document_read_only(
                            &bytes,
                            registry.read_only(&path),
                        ),
                ),
            );
        }
        for (path, read_only) in registry.documents {
            self.authoring_documents
                .entry(path)
                .or_insert_with(|| AuthoringDocument::missing(read_only));
        }
        Ok(conflicts)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn reconcile_recovered_documents(
        &mut self,
        recovered: &[crate::storage::RecoveredFile],
        disk_sources: &BTreeMap<PathBuf, String>,
        disk_authoring: &BTreeMap<PathBuf, Vec<u8>>,
    ) {
        // 只接受 storage 根据 journal 前后 hash 实际恢复出的文件；当前缓冲
        // 可以是在中断后继续编辑的版本，推进的是磁盘保存基线而不是它本身。
        for evidence in recovered {
            let expected = evidence.after.as_deref();
            if let Some(document) = self.documents.get_mut(&evidence.path) {
                let disk_matches = match (expected, disk_sources.get(&evidence.path)) {
                    (None, None) => true,
                    (Some(expected), Some(current)) => current.as_bytes() == expected,
                    _ => false,
                };
                if disk_matches {
                    document.saved = match expected {
                        None => None,
                        Some(expected) => String::from_utf8(expected.to_vec()).ok(),
                    };
                }
                continue;
            }
            let Some(document) = self.authoring_documents.get_mut(&evidence.path) else {
                continue;
            };
            let disk_matches = match (expected, disk_authoring.get(&evidence.path)) {
                (None, None) => true,
                (Some(expected), Some(current)) => current.as_slice() == expected,
                _ => false,
            };
            if disk_matches {
                document.saved = expected.map(|bytes| bytes.to_vec());
            }
        }
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let entry = entry_path(path);
        let root = entry.parent().ok_or("工作区缺少主目录")?.to_path_buf();
        let recovery = crate::storage::recover(&root)?;
        crate::file_access::read_to_string(&entry)
            .map_err(|e| format!("无法打开 {}: {e}", entry.display()))?;
        let mut project = Self {
            root,
            entry,
            documents: BTreeMap::new(),
            authoring_documents: BTreeMap::new(),
            authoring_diagnostics: Vec::new(),
            refresh_generation: 0,
            recovery_conflicts: recovery.conflicts,
            language_version: LanguageVersion::V1_9,
            source_selection: None,
            #[cfg(target_arch = "wasm32")]
            checkpoint_session_id: crate::checkpoints::next_checkpoint_session_id(),
        };
        project.refresh()?;
        if project.authoring_diagnostics.is_empty() {
            project.migrate_permissions()?;
        }
        Ok(project)
    }

    pub fn new(root: &Path) -> Self {
        let root = source_path(root);
        let entry = root.join("world.wl");
        let documents = [
            (
                "world.wl",
                include_str!("../../../examples/harbor-world/world.wl"),
            ),
            (
                "characters.wl",
                include_str!("../../../examples/harbor-world/characters.wl"),
            ),
            (
                "events/harbor.wl",
                include_str!("../../../examples/harbor-world/events/harbor.wl"),
            ),
            (
                "events/lighthouse.wl",
                include_str!("../../../examples/harbor-world/events/lighthouse.wl"),
            ),
        ]
        .into_iter()
        .map(|(path, text)| {
            (
                root.join(path),
                Document {
                    text: text.into(),
                    saved: None,
                    deleted: false,
                },
            )
        })
        .collect();
        Self {
            root,
            entry,
            documents,
            authoring_documents: BTreeMap::new(),
            authoring_diagnostics: Vec::new(),
            refresh_generation: 0,
            recovery_conflicts: Vec::new(),
            language_version: LanguageVersion::V1_9,
            source_selection: None,
            #[cfg(target_arch = "wasm32")]
            checkpoint_session_id: crate::checkpoints::next_checkpoint_session_id(),
        }
    }
}

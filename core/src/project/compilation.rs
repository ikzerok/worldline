use super::*;

impl Project {
    /// 将已授权的旧权限输入迁移到缓冲；返回修改文件数，不改变保存基线或磁盘。
    pub fn migrate_permissions(&mut self) -> Result<usize, String> {
        if !self.authoring_diagnostics.is_empty() {
            return Err("工作区清单含有不支持的能力，只能只读查看".into());
        }
        let result = self.compile_current();
        let sources = crate::migration::rewrite_sources(&result)?;
        let changed = sources
            .iter()
            .filter(|(path, text)| result.sources.get(*path) != Some(*text))
            .count();
        if changed == 0 {
            return Ok(0);
        }
        if result
            .diagnostics
            .iter()
            .any(|d| d.severity == crate::Severity::Error && d.code.starts_with('P'))
        {
            return Err("源码有语法错误，无法安全迁移权限".into());
        }
        let migrated = self.compile_source_buffers(&sources);
        if migrated
            .diagnostics
            .iter()
            .any(|d| d.severity == crate::Severity::Error && d.code.starts_with('P'))
            || migrated.analysis.fingerprint != result.analysis.fingerprint
        {
            return Err("迁移后的程序与原程序不等价，缓冲未修改".into());
        }
        for (path, text) in sources {
            let baseline = result.sources.get(&path).cloned();
            self.documents
                .entry(path)
                .or_insert_with(|| Document {
                    text: String::new(),
                    saved: baseline,
                    deleted: false,
                })
                .text = text;
        }
        Ok(changed)
    }

    pub fn sources(&self) -> BTreeMap<PathBuf, String> {
        self.documents
            .iter()
            .filter(|(path, document)| {
                !document.is_deleted()
                    && self
                        .source_selection
                        .as_ref()
                        .is_none_or(|selection| selection.is_active(path))
            })
            .map(|(p, d)| (p.clone(), d.text.clone()))
            .collect()
    }

    pub fn source_selection(&self) -> Option<&crate::source_config::SourceSelection> {
        self.source_selection.as_ref()
    }

    /// 当前工程清单选择的语言版本；无清单的旧工程固定返回 `"1.9"`。
    pub fn language_version(&self) -> &'static str {
        self.language_version.as_str()
    }

    pub fn language_version_kind(&self) -> LanguageVersion {
        self.language_version
    }

    pub fn compile_options(&self) -> CompileOptions {
        let manifest = crate::workspace_documents::manifest_path(&self.root);
        let required_features = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted())
            .map(|document| {
                crate::workspace_documents::parse_registry(&self.root, document.bytes())
                    .required_features
            })
            .unwrap_or_default();
        CompileOptions::new(self.language_version)
            .with_object_refs(
                required_features.contains(crate::project_templates::OBJECT_REFS_REQUIRED_FEATURE),
            )
            .with_character_refs(
                required_features
                    .contains(crate::project_templates::CHARACTER_REFS_REQUIRED_FEATURE),
            )
            .with_localization_ids(
                required_features.contains(crate::localization::LOCALIZATION_REQUIRED_FEATURE),
            )
    }

    pub(crate) fn compile_current(&self) -> CompileResult {
        self.compile_source_buffers(&self.sources())
    }

    fn compile_source_buffers(&self, sources: &BTreeMap<PathBuf, String>) -> CompileResult {
        self.compile_source_buffers_with_access(sources, true)
    }

    pub(crate) fn compile_problems_snapshot(&self) -> CompileResult {
        self.compile_source_buffers_with_access(&self.sources(), false)
    }

    fn compile_source_buffers_with_access(
        &self,
        sources: &BTreeMap<PathBuf, String>,
        allow_disk_fallback: bool,
    ) -> CompileResult {
        let deleted = self
            .documents
            .iter()
            .filter(|(_, document)| document.deleted)
            .map(|(path, _)| path.clone())
            .collect();
        let inactive = self
            .source_selection
            .as_ref()
            .map(|selection| {
                self.documents
                    .iter()
                    .filter(|(path, document)| !document.deleted && !selection.is_active(path))
                    .map(|(path, _)| path.clone())
                    .collect()
            })
            .unwrap_or_default();
        crate::compiler::compile_sources_with_access(
            &self.entry,
            sources,
            deleted,
            inactive,
            self.compile_options(),
            allow_disk_fallback,
        )
    }
}

//! 多文件文档缓冲、保存冲突检测与可移植目录导出。
use crate::compiler::{entry_path, source_path};
use crate::{CompileOptions, CompileResult, LanguageVersion};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub use crate::checkpoints::{
    CheckpointFileChange, CheckpointFileOperation, CheckpointLimits, CheckpointRestorePlan,
    CheckpointRestoreResult, CheckpointSummary, CheckpointTextDiff, CheckpointTextDiffSummary,
    CheckpointTextDifference, CheckpointTextSourceRange, CheckpointTextSourceSnippets,
};
pub use crate::workspace_documents::AuthoringDocument;

#[derive(Clone)]
pub struct Document {
    pub text: String,
    saved: Option<String>,
    deleted: bool,
}
impl Document {
    pub fn is_dirty(&self) -> bool {
        if self.deleted {
            self.saved.is_some()
        } else {
            self.saved.as_ref() != Some(&self.text)
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted
    }
}

#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub entry: PathBuf,
    pub documents: BTreeMap<PathBuf, Document>,
    pub authoring_documents: BTreeMap<PathBuf, AuthoringDocument>,
    authoring_diagnostics: Vec<crate::Diagnostic>,
    refresh_generation: u64,
    recovery_conflicts: Vec<PathBuf>,
    language_version: LanguageVersion,
    source_selection: Option<crate::source_config::SourceSelection>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) checkpoint_session_id: String,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub preview: String,
}

/// 一个文件的只读三方冲突快照。
///
/// `None` 表示对应一方不存在：例如本地删除的文件没有 `local`，
/// 外部删除的文件没有 `disk`。字节保持原样，因此坏 UTF-8 的展示文档
/// 也能交给上层显示或另存，不会在查询时被替换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSnapshot {
    pub path: PathBuf,
    pub baseline: Option<Vec<u8>>,
    pub local: Option<Vec<u8>>,
    pub disk: Option<Vec<u8>>,
}

/// 提案捕获使用的受控文件状态；只暴露 Project 已跟踪缓冲与其保存基线。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedFileState {
    pub path: PathBuf,
    pub baseline: Option<Vec<u8>>,
    pub current: Option<Vec<u8>>,
    pub authoring: bool,
}

impl Project {
    #[cfg(not(target_arch = "wasm32"))]
    fn validate_destination(&self, destination: &Path) -> Result<(), String> {
        if source_path(destination).starts_with(&self.root) {
            return Err("另存或导出目标必须在当前工作区外".into());
        }
        Ok(())
    }

    /// 从磁盘建立完整文件索引；冲突文件保留本地缓冲和保存基线。
    ///
    /// `.wl` 仍是编译器唯一的 source；展示 JSON 只读取清单明确注册的
    /// 路径，并以原始字节进入 authoring_documents。
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
                include_str!("../../examples/harbor-world/world.wl"),
            ),
            (
                "characters.wl",
                include_str!("../../examples/harbor-world/characters.wl"),
            ),
            (
                "events/harbor.wl",
                include_str!("../../examples/harbor-world/events/harbor.wl"),
            ),
            (
                "events/lighthouse.wl",
                include_str!("../../examples/harbor-world/events/lighthouse.wl"),
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

    /// 返回最近一次打开时发现的保存事务冲突。
    ///
    /// 冲突文件保留磁盘上的第三方内容，未完成事务和日志继续保留，直到
    /// 用户完成合并；保存与导出会拒绝在此状态下继续覆盖工程。
    pub fn recovery_conflicts(&self) -> &[PathBuf] {
        &self.recovery_conflicts
    }

    /// 返回当前缓冲与磁盘相对于保存基线发生交叉变化的文件。
    ///
    /// 查询只复制缓冲字节，并通过 `file_access` 读取磁盘；不会刷新、修改
    /// 或推进任何工程状态。`.wl` 以 UTF-8 文本缓冲保存，注册展示 JSON
    /// 则直接保留原始字节，因而两类文件都能安全展示缺失和坏 UTF-8 状态。
    pub fn conflict_snapshots(&self) -> Result<Vec<ConflictSnapshot>, String> {
        let disk_files = match crate::file_access::workspace_files(&self.root) {
            Ok(paths) => paths,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(format!("无法读取工作区文件清单：{error}")),
        };
        let mut snapshots = Vec::new();
        for (path, document) in &self.documents {
            if !document.is_dirty() {
                continue;
            }
            let baseline = document.saved.as_ref().map(|text| text.as_bytes().to_vec());
            let local = (!document.deleted).then(|| document.text.as_bytes().to_vec());
            let disk = read_conflict_disk(&self.root, path, &disk_files)?;
            if disk != baseline {
                snapshots.push(ConflictSnapshot {
                    path: path.clone(),
                    baseline,
                    local,
                    disk,
                });
            }
        }
        for (path, document) in &self.authoring_documents {
            if !document.is_dirty() {
                continue;
            }
            let baseline = document.saved.clone();
            let local = (!document.deleted).then(|| document.bytes.clone());
            let disk = read_conflict_disk(&self.root, path, &disk_files)?;
            if disk != baseline {
                snapshots.push(ConflictSnapshot {
                    path: path.clone(),
                    baseline,
                    local,
                    disk,
                });
            }
        }
        Ok(snapshots)
    }

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
        let object_refs = self
            .authoring_documents
            .get(&manifest)
            .filter(|document| !document.is_deleted())
            .is_some_and(|document| {
                crate::workspace_documents::parse_registry(&self.root, document.bytes())
                    .required_features
                    .contains(crate::project_templates::OBJECT_REFS_REQUIRED_FEATURE)
            });
        CompileOptions::new(self.language_version).with_object_refs(object_refs)
    }

    pub(crate) fn compile_current(&self) -> CompileResult {
        self.compile_source_buffers(&self.sources())
    }

    fn compile_source_buffers(&self, sources: &BTreeMap<PathBuf, String>) -> CompileResult {
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
        crate::compiler::compile_sources_excluding_inactive_with_options(
            &self.entry,
            sources,
            deleted,
            inactive,
            self.compile_options(),
        )
    }

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

    pub fn document(&self, path: &Path) -> Result<&str, String> {
        self.documents
            .get(&source_path(path))
            .filter(|d| !d.is_deleted())
            .map(|d| d.text.as_str())
            .ok_or_else(|| format!("文件未载入:{}", path.display()))
    }

    /// 返回一个已跟踪文件的保存基线和当前缓冲；不读取磁盘、不推进基线。
    pub fn tracked_file_state(&self, path: &Path) -> Option<TrackedFileState> {
        let path = source_path(path);
        if let Some(document) = self.documents.get(&path) {
            return Some(TrackedFileState {
                path,
                baseline: document.saved.as_ref().map(|text| text.as_bytes().to_vec()),
                current: (!document.deleted).then(|| document.text.as_bytes().to_vec()),
                authoring: false,
            });
        }
        self.authoring_documents
            .get(&path)
            .map(|document| TrackedFileState {
                path,
                baseline: document.saved.clone(),
                current: (!document.deleted).then(|| document.bytes.clone()),
                authoring: true,
            })
    }

    /// 只返回相对保存基线发生变化的受控文档；普通附件不进入结构化提案。
    pub fn dirty_tracked_files(&self) -> Vec<TrackedFileState> {
        let mut out = Vec::new();
        for (path, document) in &self.documents {
            if document.is_dirty() {
                if let Some(state) = self.tracked_file_state(path) {
                    out.push(state);
                }
            }
        }
        for (path, document) in &self.authoring_documents {
            if document.is_dirty() {
                if let Some(state) = self.tracked_file_state(path) {
                    out.push(state);
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    pub(crate) fn ensure_workspace_writable(&self) -> Result<(), String> {
        if self.authoring_diagnostics.is_empty() {
            Ok(())
        } else {
            Err("工作区清单含有不支持的能力，只能只读查看".into())
        }
    }

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
    fn ensure_storage_ready(&self) -> Result<(), String> {
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

    /// 导出可直接编译的完整目录;不覆盖目标目录,包含所有内存修改。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export(&self, destination: &Path) -> Result<(), String> {
        self.ensure_storage_ready()?;
        if destination.exists() {
            return Err("导出目标已存在,请选择新的文件夹名称".into());
        }
        self.validate_destination(destination)?;
        let mut migrated = self.clone();
        if migrated.root.exists() || cfg!(target_arch = "wasm32") {
            migrated.refresh()?;
        }
        if migrated.authoring_diagnostics.is_empty() {
            migrated.migrate_permissions()?;
        }
        let files = migrated.export_file_contents()?;
        let parent = destination.parent().ok_or("导出目录缺少父目录")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let staging = parent.join(format!(".worldedit-export-{}", std::process::id()));
        std::fs::create_dir(&staging).map_err(|e| format!("无法建立导出暂存目录:{e}"))?;
        let write_result = (|| -> std::io::Result<()> {
            for (path, text) in files.sources {
                let path = staging.join(path);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, text)?;
            }
            for (path, bytes) in files.authoring {
                let path = staging.join(path);
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(path, bytes)?;
            }
            for (relative, source) in files.copies {
                let target = staging.join(relative);
                std::fs::create_dir_all(target.parent().unwrap())?;
                std::fs::copy(source, target)?;
            }
            std::fs::rename(&staging, destination)
        })();
        if let Err(e) = write_result {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("导出失败:{e}"));
        }
        Ok(())
    }

    /// 校验并生成完整可移植工程；桌面目录与浏览器下载共用同一导出内容。
    pub fn export_files(&self) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        self.ensure_storage_ready()?;
        let mut migrated = self.clone();
        if migrated.root.exists() || cfg!(target_arch = "wasm32") {
            migrated.refresh()?;
        }
        if migrated.authoring_diagnostics.is_empty() {
            migrated.migrate_permissions()?;
        }
        let contents = migrated.export_file_contents()?;
        let mut files: BTreeMap<_, _> = contents
            .sources
            .into_iter()
            .map(|(path, text)| (path, text.into_bytes()))
            .collect();
        files.extend(contents.authoring);
        for (relative, source) in contents.copies {
            files.insert(
                relative,
                crate::file_access::read(source).map_err(|e| e.to_string())?,
            );
        }
        Ok(files)
    }

    fn export_file_contents(&self) -> Result<crate::catalog_edit::PortableAssets, String> {
        let result = self.compile_current();
        if result.has_errors() {
            return Err("工程存在编译错误,修复后才能导出".into());
        }
        for asset in result.analysis.catalog.assets.values() {
            if !asset.available {
                return Err(format!("素材 `{}` 不可用，请修复引用后导出", asset.id));
            }
        }
        let portable = self.portable_assets()?;
        let mut files = BTreeMap::<PathBuf, String>::new();
        for (path, text) in &portable.sources {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| format!("引用文件在工程目录之外:{}", path.display()))?;
            validate_relative(relative)?;
            for line in crate::lexer::lex_source(&path.to_string_lossy(), text, &mut Vec::new()) {
                if let crate::lexer::LineKind::Include { path, .. } = line.kind {
                    if Path::new(&path).is_absolute() {
                        return Err("导出要求 include 使用相对路径".into());
                    }
                }
            }
            files.insert(relative.into(), text.clone());
        }
        let mut authoring = BTreeMap::new();
        for (path, bytes) in portable.authoring {
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| format!("展示文档在工程目录之外:{}", path.display()))?;
            validate_authoring_relative(relative)?;
            authoring.insert(relative.into(), bytes);
        }
        if self.entry.file_name() != Some(std::ffi::OsStr::new("world.wl")) {
            if files.contains_key(Path::new("world.wl"))
                || authoring.contains_key(Path::new("world.wl"))
            {
                return Err("world.wl 已被其他引用文件占用,请先整理总入口".into());
            }
            files.insert(
                "world.wl".into(),
                format!(
                    "include {}\n",
                    crate::authoring::quote(&self.entry.file_name().unwrap().to_string_lossy())
                ),
            );
        }
        Ok(crate::catalog_edit::PortableAssets {
            sources: files,
            authoring,
            copies: portable.copies,
        })
    }
}

fn validate_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path.extension().and_then(|e| e.to_str()) != Some("wl")
    {
        return Err("文件路径须为工程内的相对 .wl 路径,例如 events/harbor.wl".into());
    }
    Ok(())
}

fn read_conflict_disk(
    root: &Path,
    path: &Path,
    disk_files: &[PathBuf],
) -> Result<Option<Vec<u8>>, String> {
    let path = crate::file_access::within(root, path)?;
    if disk_files.binary_search(&path).is_err() {
        return Ok(None);
    }
    match crate::file_access::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法读取冲突文件：{error}")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_disk(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn validate_authoring_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.extension().and_then(|extension| extension.to_str()) != Some("json")
    {
        return Err("展示文档路径须为工程内的相对 .json 路径".into());
    }
    Ok(())
}

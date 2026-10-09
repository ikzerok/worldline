//! 不应用正文的独立编译快照；所有可编辑内容仍属于原 WritingBuffer。
mod input;
mod navigation;
#[cfg(test)]
mod tests;

use crate::{manuscript::WritingBuffer, project::Project, CompileResult};
pub use input::{
    DraftRehearsalExcludedInput, DraftRehearsalInput, DraftRehearsalRequest,
    MAX_DRAFT_REHEARSAL_BYTES, MAX_DRAFT_REHEARSAL_FILES,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub const DRAFT_REHEARSAL_CAPABILITY: &str = "authoring.draft_rehearsal.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftRehearsalSourceKind {
    Applied,
    WritingDraft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRehearsalSource {
    pub path: PathBuf,
    pub kind: DraftRehearsalSourceKind,
    pub generation: Option<u64>,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftRehearsalScope {
    pub schema_version: u32,
    pub content_baseline: String,
    pub entry: PathBuf,
    pub language_version: String,
    /// 字符串避免 JSON 消费者以浮点截断 u64。
    pub runtime_fingerprint: String,
    pub sources: Vec<DraftRehearsalSource>,
    pub excluded_inputs: Vec<DraftRehearsalExcludedInput>,
}

/// 不可反序列化的真实编译快照；范围 DTO 不构成新建会话的凭证。
pub struct DraftRehearsalSnapshot {
    request: DraftRehearsalRequest,
    compiled: CompileResult,
    scope: DraftRehearsalScope,
    root: PathBuf,
    refresh_generation: u64,
}

impl DraftRehearsalSnapshot {
    /// 本地化只读准备必须绑定同一工程、已应用稿、刷新代次与编译能力。
    pub(crate) fn verify_localization_project(&self, project: &Project) -> Result<(), String> {
        if self.root != project.root
            || self.refresh_generation != project.search_refresh_generation()
            || self.compiled.options != project.compile_options()
            || self.scope.content_baseline != project.content_baseline()
        {
            return Err("草稿试演的工程或已应用来源已变化，请明确重新试演".into());
        }
        Ok(())
    }

    /// 精确定位本次草稿 AST 中的可翻译正文；调用者回源前仍须验证当前稿与导航守卫。
    pub fn localization_source(
        &self,
        source: &crate::localization::LocalizationSource,
    ) -> Result<crate::search_replace::SearchMatch, String> {
        let draft = self
            .request
            .drafts
            .iter()
            .any(|draft| draft.path == std::path::Path::new(&source.file));
        crate::localization::localization_source_hit(&self.compiled, &self.root, source, draft)
    }

    pub fn compiled(&self) -> &CompileResult {
        &self.compiled
    }
    pub fn scope(&self) -> &DraftRehearsalScope {
        &self.scope
    }
    pub fn request(&self) -> &DraftRehearsalRequest {
        &self.request
    }
    pub fn relative_source_path(&self, path: &std::path::Path) -> Result<PathBuf, String> {
        if !self.compiled.sources.contains_key(path) {
            return Err("来源不属于本次完整编译快照".into());
        }
        path.strip_prefix(&self.root)
            .map(PathBuf::from)
            .map_err(|_| "试演来源不属于原工作区".into())
    }

    /// 完整的动作时检查；界面不能在空闲绘制帧反复调用。
    pub fn verify_current(
        &self,
        project: &Project,
        buffers: &[WritingBuffer],
        composing: bool,
    ) -> Result<(), String> {
        if self.root != project.root
            || self.refresh_generation != project.search_refresh_generation()
            || self.compiled.options != project.compile_options()
        {
            return Err("试演快照的工作区、刷新代次或语言已变化；请明确重新试演".into());
        }
        self.request.verify_current(project, buffers, composing)
    }

    /// 只读磁盘观察仅在明确启动/回源时执行；WASM 使用导入快照。
    pub fn verify_navigation(
        &self,
        project: &Project,
        buffers: &[WritingBuffer],
        composing: bool,
    ) -> Result<(), String> {
        self.verify_current(project, buffers, composing)?;
        project.verify_review_navigation()
    }
}

impl Project {
    pub fn compile_draft_rehearsal(
        &self,
        request: &DraftRehearsalRequest,
    ) -> Result<DraftRehearsalSnapshot, String> {
        request.validate()?;
        if request.composing {
            return Err("正文仍有输入法组合或未插入的保留输入，请先完成输入；草稿未改变".into());
        }
        self.ensure_workspace_writable()?;
        if request.content_baseline != self.content_baseline() {
            return Err("正文草稿试演的完整工程基线已过期".into());
        }
        input::validate_project_sources(self)?;
        self.verify_review_navigation()?;
        let mut expected = self.sources();
        let mut buffers = Vec::with_capacity(request.drafts.len());
        let mut paths = Vec::with_capacity(request.drafts.len());
        for draft in &request.drafts {
            let path = crate::file_access::within(&self.root, &self.root.join(&draft.path))?;
            if expected.get(&path) != Some(&draft.original_source) {
                return Err(format!(
                    "草稿来源不是当前活动源的完整原文：{}",
                    draft.path.display()
                ));
            }
            let mut buffer = self.open_source_writing_buffer(&path)?;
            buffer.replace_source(draft.source.clone());
            expected.insert(path.clone(), draft.source.clone());
            paths.push(path);
            buffers.push(buffer);
        }
        input::verify_file_identities(&paths)?;
        input::validate_sources(&expected)?;
        let compiled = self.compile_writing_drafts(&buffers)?;
        if compiled.sources != expected {
            return Err("真实编译来源与已核对的完整活动源集合不一致；未启动试演".into());
        }
        // 编译可能读取附件/来源；结束时再次观察，不能发布夹杂外部变化的快照。
        self.verify_review_navigation()?;
        let included: BTreeMap<_, _> = request
            .drafts
            .iter()
            .map(|draft| (self.root.join(&draft.path), draft.generation))
            .collect();
        let scope = DraftRehearsalScope {
            schema_version: 1,
            content_baseline: request.content_baseline.clone(),
            entry: self
                .entry
                .strip_prefix(&self.root)
                .unwrap_or(&self.entry)
                .into(),
            language_version: compiled.options.language_version.as_str().into(),
            runtime_fingerprint: compiled.analysis.fingerprint.to_string(),
            sources: compiled
                .sources
                .iter()
                .map(|(path, text)| DraftRehearsalSource {
                    path: path.strip_prefix(&self.root).unwrap_or(path).into(),
                    kind: if included.contains_key(path) {
                        DraftRehearsalSourceKind::WritingDraft
                    } else {
                        DraftRehearsalSourceKind::Applied
                    },
                    generation: included.get(path).copied(),
                    bytes: text.len(),
                })
                .collect(),
            excluded_inputs: request.excluded_inputs.clone(),
        };
        Ok(DraftRehearsalSnapshot {
            request: request.clone(),
            compiled,
            scope,
            root: self.root.clone(),
            refresh_generation: self.search_refresh_generation(),
        })
    }
}

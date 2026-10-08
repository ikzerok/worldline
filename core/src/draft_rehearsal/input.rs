use crate::{manuscript::WritingBuffer, project::Project};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub const MAX_DRAFT_REHEARSAL_FILES: usize = 256;
pub const MAX_DRAFT_REHEARSAL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRehearsalInput {
    pub path: PathBuf,
    pub original_source: String,
    pub source: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRehearsalExcludedInput {
    pub kind: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRehearsalRequest {
    pub schema_version: u32,
    pub content_baseline: String,
    pub drafts: Vec<DraftRehearsalInput>,
    #[serde(default)]
    pub excluded_inputs: Vec<DraftRehearsalExcludedInput>,
    #[serde(default)]
    pub composing: bool,
}

impl DraftRehearsalRequest {
    pub fn from_writing_buffers(
        project: &Project,
        buffers: &[WritingBuffer],
        excluded_inputs: Vec<DraftRehearsalExcludedInput>,
        composing: bool,
    ) -> Result<Self, String> {
        let changed = buffers.iter().filter(|buffer| buffer.is_changed());
        let mut count = 0usize;
        let mut bytes = 0usize;
        for buffer in changed {
            count += 1;
            bytes = bytes
                .saturating_add(buffer.source().len())
                .saturating_add(project.document(buffer.path())?.len());
            if count > MAX_DRAFT_REHEARSAL_FILES || bytes > MAX_DRAFT_REHEARSAL_BYTES {
                return Err("试演草稿超过256文件或前后源文16MiB预算".into());
            }
        }
        let baseline = project.content_baseline();
        let mut paths = BTreeSet::new();
        let mut drafts = Vec::new();
        for buffer in buffers {
            if !paths.insert(buffer.path()) {
                return Err("同一文件只允许一个正文草稿，即使内容相同也不能重复传入".into());
            }
            if !buffer.is_changed() {
                continue;
            }
            if buffer.baseline() != baseline {
                return Err("正文草稿的完整基线已过期；所有输入保留".into());
            }
            let relative = buffer
                .path()
                .strip_prefix(&project.root)
                .map_err(|_| "正文草稿来源不在当前工作区")?;
            drafts.push(DraftRehearsalInput {
                path: relative.into(),
                original_source: project.document(buffer.path())?.into(),
                source: buffer.source().into(),
                generation: buffer.generation(),
            });
        }
        drafts.sort_by(|left, right| left.path.cmp(&right.path));
        let request = Self {
            schema_version: 1,
            content_baseline: baseline,
            drafts,
            excluded_inputs,
            composing,
        };
        request.validate()?;
        Ok(request)
    }

    /// 宿主在使用后台结果前核对同一份作者输入；不编译，不读取磁盘。
    pub fn verify_current(
        &self,
        project: &Project,
        buffers: &[WritingBuffer],
        composing: bool,
    ) -> Result<(), String> {
        self.validate()?;
        project.ensure_workspace_writable()?;
        if composing {
            return Err("正文组合或保留输入尚未完成，旧试演来源不可用".into());
        }
        let current =
            Self::from_writing_buffers(project, buffers, self.excluded_inputs.clone(), composing)?;
        // DTO数组次序不参与文件身份；只排序借用，仍逐项比较完整原文、新文及代次。
        let mut expected: Vec<_> = self.drafts.iter().collect();
        expected.sort_by(|left, right| left.path.cmp(&right.path));
        if current.content_baseline != self.content_baseline
            || current.composing != self.composing
            || !current.drafts.iter().eq(expected)
        {
            return Err("正文草稿、代次或完整工程基线已变化，请重新试演".into());
        }
        Ok(())
    }

    /// 机器接口可先区分 DTO 结构/预算违规，再执行真实编译。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("不支持的正文草稿试演请求版本".into());
        }
        if self.content_baseline.is_empty() || self.content_baseline.len() > 256 {
            return Err("试演需要有效的完整工程内容基线".into());
        }
        if self.drafts.is_empty() || self.drafts.len() > MAX_DRAFT_REHEARSAL_FILES {
            return Err("当前草稿试演要求 1–256 个真实改变的活动源文件".into());
        }
        let mut bytes = 0usize;
        let mut paths = BTreeSet::new();
        for draft in &self.drafts {
            crate::source_lifecycle::safety::relative(&draft.path)?;
            if draft.path.as_os_str().len() > 4096 {
                return Err("试演源码路径超过4096字节".into());
            }
            if !paths.insert(draft.path.clone()) {
                return Err("同一文件只允许一个正文草稿".into());
            }
            if draft.generation > 9_007_199_254_740_991 {
                return Err("草稿代次超出 JSON 安全整数范围".into());
            }
            if draft.source == draft.original_source {
                return Err("试演覆盖项必须是未应用的正文变化".into());
            }
            bytes = bytes
                .saturating_add(draft.source.len())
                .saturating_add(draft.original_source.len());
            if bytes > MAX_DRAFT_REHEARSAL_BYTES {
                return Err("试演草稿前后源文合计超过 16 MiB；未截断或运行部分稿".into());
            }
        }
        if self.excluded_inputs.len() > 256
            || self
                .excluded_inputs
                .iter()
                .any(|input| input.kind.len().saturating_add(input.source.len()) > 4096)
        {
            return Err("排除输入说明超过 256 项或单项 4096 字节".into());
        }
        Ok(())
    }
}

pub(super) fn validate_project_sources(project: &Project) -> Result<(), String> {
    let mut count = 0usize;
    let mut bytes = 0usize;
    for (path, document) in &project.documents {
        if document.is_deleted()
            || project
                .source_selection()
                .is_some_and(|selection| !selection.is_active(path))
        {
            continue;
        }
        count += 1;
        bytes = bytes.saturating_add(document.text.len());
        if count > 4096 || bytes > 64 * 1024 * 1024 {
            return Err("试演完整活动源超过4096文件或64MiB".into());
        }
    }
    Ok(())
}

pub(super) fn validate_sources(sources: &BTreeMap<PathBuf, String>) -> Result<(), String> {
    let bytes = sources
        .values()
        .fold(0usize, |sum, text| sum.saturating_add(text.len()));
    if sources.len() > 4096 || bytes > 64 * 1024 * 1024 {
        return Err("试演完整活动源超过 4096 文件或 64 MiB".into());
    }
    Ok(())
}

pub(super) fn verify_file_identities(paths: &[PathBuf]) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut handles = std::collections::HashSet::new();
        for path in paths {
            match same_file::Handle::from_path(path) {
                Ok(handle) => {
                    if !handles.insert(handle) {
                        return Err("不同草稿路径指向同一物理文件，无法确认唯一来源".into());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("无法确认草稿物理文件身份：{error}")),
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = paths;
    Ok(())
}

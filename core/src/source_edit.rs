//! 完整源码草稿的显式机器事务，复用正文唯一缓冲和磁盘冲突验证。
use crate::{project::Project, Diagnostic};
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEditRequest {
    pub schema_version: u32,
    pub path: PathBuf,
    pub expected_baseline: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceEditPreview {
    pub schema_version: u32,
    pub path: PathBuf,
    pub changed: bool,
    pub plan_digest: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl Project {
    /// 完整源码可以包含诊断，未知工作区能力/外部变化仍拒绝。
    pub fn preview_source_edit(
        &self,
        request: &SourceEditRequest,
    ) -> Result<SourceEditPreview, String> {
        let (_, preview) = self.prepare_source_edit(request)?;
        Ok(preview)
    }

    /// 仅改内存，调用方显式保存；前后可通过Project快照统一撤销。
    pub fn apply_source_edit(
        &mut self,
        request: &SourceEditRequest,
        plan_digest: &str,
    ) -> Result<SourceEditPreview, String> {
        let (candidate, preview) = self.prepare_source_edit(request)?;
        if preview.plan_digest != plan_digest {
            return Err("源码预览摘要不符，请重新预览；原稿未改变".into());
        }
        *self = candidate;
        Ok(preview)
    }

    pub(crate) fn prepare_source_edit(
        &self,
        request: &SourceEditRequest,
    ) -> Result<(Project, SourceEditPreview), String> {
        if request.schema_version != 1 {
            return Err("不支持的源码草稿DTO版本".into());
        }
        if request.expected_baseline != self.content_baseline() {
            return Err("源码草稿基线已过期，请重新预览".into());
        }
        if request.path.as_os_str().is_empty()
            || request.path.is_absolute()
            || request
                .path
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || request.path.extension().is_none_or(|e| e != "wl")
        {
            return Err("源码路径必须是工作区内不含上级跳转的相对.wl文件".into());
        }
        let mut buffer = self.open_source_writing_buffer(&request.path)?;
        buffer.replace_source(request.source.clone());
        let changed = buffer.is_changed();
        let mut candidate = self.clone();
        let diagnostics = candidate.apply_source_writing_buffer(&buffer)?;
        let preview = SourceEditPreview {
            schema_version: 1,
            path: request.path.clone(),
            changed,
            plan_digest: candidate.content_baseline(),
            diagnostics,
        };
        Ok((candidate, preview))
    }
}

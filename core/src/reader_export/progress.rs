use serde::{Deserialize, Serialize};

/// 当前阶段的计数；回调返回 false 时取消，未发布目标不得留下部分站点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderExportProgress {
    pub phase: String,
    pub completed: usize,
    pub total: usize,
}

pub(super) fn report(
    progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
    phase: &str,
    completed: usize,
    total: usize,
) -> Result<(), String> {
    if progress(&ReaderExportProgress {
        phase: phase.into(),
        completed,
        total,
    }) {
        Ok(())
    } else {
        Err("READER_CANCELLED：已取消阅读包，未发布目标".into())
    }
}

impl super::Project {
    pub fn preview_reader_export_with_progress(
        &self,
        selection: &super::ReaderExportSelection,
        progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
    ) -> Result<super::ReaderExportPreview, String> {
        Ok(super::plan::prepare_with_routes(self, selection, &[], progress)?.preview)
    }

    pub fn build_reader_export_with_progress(
        &self,
        selection: &super::ReaderExportSelection,
        expected_plan_digest: &str,
        progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
    ) -> Result<std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>, String> {
        let prepared = super::plan::prepare_with_routes(self, selection, &[], progress)?;
        build_prepared(prepared, expected_plan_digest, progress)
    }
}

pub(super) fn build_prepared(
    prepared: super::PreparedExport,
    expected_plan_digest: &str,
    progress: &mut dyn FnMut(&ReaderExportProgress) -> bool,
) -> Result<std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>, String> {
    if prepared.preview.plan_digest != expected_plan_digest {
        return Err("阅读包预览已过期，请重新预览并核对选择".into());
    }
    super::site::render_package(prepared, progress)
}

pub(super) fn check_budget(
    pages: &[super::PublicPage],
    attachments: &[super::PublicAttachment],
) -> Result<(), String> {
    let mut bytes = 0usize;
    for size in pages
        .iter()
        .map(|page| {
            page.body_html
                .len()
                .saturating_add(page.searchable_text.len())
                .saturating_add(page.title.len())
        })
        .chain(attachments.iter().map(|attachment| attachment.bytes.len()))
    {
        bytes = bytes.checked_add(size).ok_or("阅读包大小超过限制")?;
        if bytes > super::MAX_PACKAGE_BYTES {
            return Err("阅读包超过 128 MiB 限制".into());
        }
    }
    if pages.len().saturating_add(attachments.len()) > super::MAX_OUTPUT_FILES {
        return Err("阅读包超过 10000 个文件限制".into());
    }
    Ok(())
}

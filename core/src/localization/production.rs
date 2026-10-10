//! 制作范围复用同一份已编译来源和已有 sidecar/token 校验；不执行 runtime。
use super::*;
use crate::{project::Project, CompileResult};
use std::collections::BTreeSet;

pub(crate) fn production_catalog(
    project: &Project,
    compiled: &CompileResult,
    locale: Option<&str>,
    source_key: &str,
) -> Result<Vec<LocalizationCatalogEntry>, LocalizationError> {
    limits::project(project).map_err(LocalizationError::from)?;
    let records =
        source::collect(&compiled.program, &project.root).map_err(LocalizationError::from)?;
    let mut ids = BTreeSet::new();
    if records
        .iter()
        .filter_map(|record| record.id.as_ref())
        .any(|id| !ids.insert(id))
    {
        return Err(LocalizationError::new(
            "DUPLICATE_ID",
            "全工程存在重复稳定行 ID，不能以筛选隐藏完整性错误",
        ));
    }
    let available = catalog::load_locale(project, None)
        .map_err(LocalizationError::from)?
        .available;
    if locale.is_some_and(|value| !available.iter().any(|known| known == value)) {
        return Err(LocalizationError::new(
            "UNKNOWN_LOCALE",
            "目标 locale 未在当前工程注册",
        ));
    }
    // 注册完整性先于选中单元状态；缺译/过期/保护 token 错误仍由逐行状态处理。
    for name in &available {
        let loaded = catalog::load_locale(project, Some(name)).map_err(LocalizationError::from)?;
        if let Some(error) = loaded.error {
            return Err(LocalizationError::new("SIDECAR_INVALID", error));
        }
        if loaded.read_only || loaded.entries.values().any(|entry| entry.invalid) {
            return Err(LocalizationError::new(
                "SIDECAR_INVALID",
                "注册 locale 文档结构无效或含未知必需能力",
            ));
        }
    }
    let loaded = catalog::load_locale(project, locale).map_err(LocalizationError::from)?;
    catalog::catalog_entries(&records, &loaded, source_key).map_err(LocalizationError::from)
}

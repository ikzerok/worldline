//! 制作范围复用同一份已编译来源和已有 sidecar/token 校验；不执行 runtime。
use super::*;
use crate::{project::Project, CompileResult};
use std::collections::{BTreeMap, BTreeSet};

/// 来自正式 provenance 的物理身份；公开展示路径不能替代该键。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct SourceIdentity {
    pub file: u32,
    pub line: u32,
    pub kind: String,
}
impl SourceIdentity {
    pub(crate) fn new(file: u32, line: u32, kind: &str) -> Self {
        Self {
            file,
            line,
            kind: kind.into(),
        }
    }
}

pub(crate) fn production_catalog(
    project: &Project,
    compiled: &CompileResult,
    locale: Option<&str>,
    source_key: &str,
    source_ids: &BTreeMap<PathBuf, u32>,
) -> Result<BTreeMap<SourceIdentity, LocalizationCatalogEntry>, LocalizationError> {
    limits::project(project).map_err(LocalizationError::from)?;
    let records = source::collect_identified(&compiled.program, &project.root, source_ids)
        .map_err(LocalizationError::from)?;
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
    let entries =
        catalog::catalog_entries(&records, &loaded, source_key).map_err(LocalizationError::from)?;
    index_catalog(records, entries)
}

fn index_catalog(
    records: Vec<source::Record>,
    entries: Vec<LocalizationCatalogEntry>,
) -> Result<BTreeMap<SourceIdentity, LocalizationCatalogEntry>, LocalizationError> {
    let invalid = || LocalizationError::new("INVALID_SOURCE", "制作台本来源身份重复或无法一一确认");
    let mut entries = entries.into_iter();
    let mut out = BTreeMap::new();
    let mut bytes = 0usize;
    // catalog_entries 按正式 records 原顺序输出来源条目，之后才附加孤立译文。
    // 逐条验证来源、内容和版本，不通过显示 file 重新关联。
    for record in records {
        let entry = entries.next().ok_or_else(invalid)?;
        if entry.source.as_ref() != Some(&record.unit.source)
            || entry.id != record.id
            || entry.source_revision.as_ref() != Some(&record.unit.source_revision)
            || entry.source_parts != record.unit.parts
        {
            return Err(invalid());
        }
        let identity = record.identity.ok_or_else(invalid)?;
        if identity.line != record.unit.source.line || identity.kind != record.unit.source.kind {
            return Err(invalid());
        }
        limits::reserve(&(&identity, &entry), &mut bytes, "制作台本原始来源索引")
            .map_err(LocalizationError::from)?;
        if out.insert(identity, entry).is_some() {
            return Err(invalid());
        }
    }
    if entries.any(|entry| entry.source.is_some()) {
        return Err(invalid());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(file: u32, text: &str) -> source::Record {
        source::Record {
            identity: Some(SourceIdentity::new(file, 2, "say")),
            id: None,
            unit: SourceUnit {
                source: LocalizationSource {
                    file: "a/b.wl".into(),
                    line: 2,
                    kind: "say".into(),
                },
                parts: vec![LocalizationPart::Text { text: text.into() }],
                source_revision: "revision".into(),
            },
        }
    }
    #[test]
    fn production_identity_index_rejects_duplicate_raw_keys_without_last_write_wins() {
        let record = record(0, "first");
        let records = vec![record.clone(), record];
        let entries =
            catalog::catalog_entries(&records, &catalog::Locale::default(), "key").unwrap();
        assert_eq!(
            index_catalog(records, entries).unwrap_err().code,
            "INVALID_SOURCE"
        );
    }
    #[test]
    fn production_identity_index_never_joins_distinct_source_ids_by_display_text() {
        let records = vec![record(0, "A"), record(1, "B")];
        let entries =
            catalog::catalog_entries(&records, &catalog::Locale::default(), "key").unwrap();
        let indexed = index_catalog(records, entries).unwrap();
        assert_eq!(indexed.len(), 2);
        for (file, expected) in [(0, "A"), (1, "B")] {
            let key = SourceIdentity::new(file, 2, "say");
            assert_eq!(
                indexed[&key].source_parts,
                vec![LocalizationPart::Text {
                    text: expected.into()
                }]
            );
        }
    }
    #[test]
    fn production_identity_index_rejects_reordered_entries_and_accepts_orphan_suffix() {
        let records = vec![record(0, "A"), record(1, "B")];
        let mut locale = catalog::Locale::default();
        locale.entries.insert(
            "orphan".into(),
            catalog::StoredEntry {
                revision: Some("old".into()),
                parts: Some(vec![]),
                invalid: false,
            },
        );
        let entries = catalog::catalog_entries(&records, &locale, "key").unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.last().unwrap().source.is_none());
        assert_eq!(
            index_catalog(records.clone(), entries.clone())
                .unwrap()
                .len(),
            2
        );
        let mut reordered = entries;
        reordered.swap(0, 1);
        assert_eq!(
            index_catalog(records, reordered).unwrap_err().code,
            "INVALID_SOURCE"
        );
    }
}

use super::*;
use crate::project::Project;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Locale {
    pub available: Vec<String>,
    pub path: Option<String>,
    pub source_locale: Option<String>,
    pub entries: BTreeMap<String, StoredEntry>,
    pub error: Option<String>,
    pub read_only: bool,
}

pub(super) struct StoredEntry {
    pub revision: Option<String>,
    pub parts: Option<Vec<LocalizationPart>>,
    pub invalid: bool,
}

impl Project {
    pub fn query_localization_catalog(
        &self,
        query: &LocalizationCatalogQuery,
    ) -> Result<LocalizationCatalogPage, LocalizationError> {
        query_catalog(self, query).map_err(LocalizationError::from)
    }
}

fn query_catalog(
    project: &Project,
    query: &LocalizationCatalogQuery,
) -> Result<LocalizationCatalogPage, String> {
    validate_query(query)?;
    limits::project(project)?;
    let content_baseline = project.content_baseline();
    let source_baseline = export::source_baseline(project)?;
    if query
        .expected_content_baseline
        .as_ref()
        .is_some_and(|b| b != &content_baseline)
        || query
            .expected_source_baseline
            .as_ref()
            .is_some_and(|b| b != &source_baseline)
        || (query.offset > 0
            && (query.expected_content_baseline.is_none()
                || query.expected_source_baseline.is_none()))
    {
        return Err("STALE_QUERY：本地化目录基线已变化或续页缺少基线，请重新查询".into());
    }
    let (_, records) = source::current(project)?;
    let locale = load_locale(project, query.target_locale.as_deref())?;
    let mut entries = catalog_entries(&records, &locale, &source_baseline)?;
    let all_total = entries.len();
    let mut status_counts = BTreeMap::new();
    for entry in &entries {
        *status_counts.entry(entry.status).or_insert(0) += 1;
    }
    let ids: BTreeSet<_> = query.string_ids.iter().map(String::as_str).collect();
    entries.retain(|entry| matches_query(entry, query, &ids));
    let total = entries.len();
    let end = query.offset.saturating_add(query.limit).min(total);
    let next_offset = (end < total).then_some(end);
    let entries = entries
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .collect();
    let mut diagnostics = Vec::new();
    if let Some(message) = &locale.error {
        diagnostics.push(LocalizationDiagnostic {
            code: "SIDECAR_INVALID".into(),
            id: None,
            source: None,
            message: message.clone(),
        });
    }
    let page = LocalizationCatalogPage {
        schema_version: 1,
        content_baseline,
        source_baseline,
        source_locale: locale.source_locale,
        target_locale: query.target_locale.clone(),
        available_locales: locale.available,
        all_total,
        total,
        status_counts,
        entries,
        next_offset,
        read_only: locale.read_only || !project.authoring_diagnostics().is_empty(),
        diagnostics,
    };
    limits::serialized(&page, limits::MAX_OUTPUT_BYTES, "目录页输出")?;
    Ok(page)
}

fn validate_query(query: &LocalizationCatalogQuery) -> Result<(), String> {
    if query.schema_version != 1 {
        return Err("UNSUPPORTED_VERSION：不支持的本地化查询版本".into());
    }
    limits::budget(
        (1..=MAX_LOCALIZATION_PAGE_SIZE).contains(&query.limit),
        "分页数量",
    )?;
    limits::budget(query.search.len() <= 1024, "查询文本")?;
    limits::budget(query.statuses.len() <= 7, "状态筛选数")?;
    if query
        .statuses
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .len()
        != query.statuses.len()
    {
        return Err("INVALID_QUERY：状态筛选集合不能重复".into());
    }
    limits::budget(
        query.string_ids.len() <= MAX_LOCALIZATION_BATCH,
        "精确筛选 ID 数",
    )?;
    if let Some(locale) = &query.target_locale {
        limits::id(locale)?;
    }
    for id in &query.string_ids {
        limits::id(id)?;
    }
    if query
        .kind
        .as_deref()
        .is_some_and(|kind| !["text", "say", "choice"].contains(&kind))
    {
        return Err("INVALID_QUERY：kind 必须是 text、say 或 choice".into());
    }
    if let Some(prefix) = &query.source_prefix {
        limits::budget(prefix.len() <= 1024, "来源前缀")?;
        if prefix.contains('\\')
            || prefix.contains(':')
            || prefix.starts_with('/')
            || prefix.chars().any(char::is_control)
            || prefix.split('/').any(|part| part == ".." || part == ".")
        {
            return Err("INVALID_QUERY：来源前缀必须是工作区内相对路径".into());
        }
    }
    Ok(())
}

fn matches_query(
    entry: &LocalizationCatalogEntry,
    query: &LocalizationCatalogQuery,
    ids: &BTreeSet<&str>,
) -> bool {
    if !query.statuses.is_empty() && !query.statuses.contains(&entry.status) {
        return false;
    }
    if !ids.is_empty() && !entry.id.as_deref().is_some_and(|id| ids.contains(id)) {
        return false;
    }
    if query.source_prefix.as_ref().is_some_and(|prefix| {
        !entry
            .source
            .as_ref()
            .is_some_and(|source| source.file.starts_with(prefix))
    }) {
        return false;
    }
    if query.kind.as_ref().is_some_and(|kind| {
        !entry
            .source
            .as_ref()
            .is_some_and(|source| &source.kind == kind)
    }) {
        return false;
    }
    if query.search.is_empty() {
        return true;
    }
    if entry
        .id
        .as_ref()
        .is_some_and(|id| id.contains(&query.search))
    {
        return true;
    }
    entry
        .source_parts
        .iter()
        .chain(entry.translation_parts.iter().flatten())
        .any(|part| match part {
            LocalizationPart::Text { text } => text.contains(&query.search),
            LocalizationPart::Link { label, .. } => label.contains(&query.search),
            LocalizationPart::Placeholder { .. } => false,
        })
}

pub(super) fn load_locale(project: &Project, locale: Option<&str>) -> Result<Locale, String> {
    let mut loaded = Locale::default();
    let manifest = crate::workspace_documents::manifest_path(&project.root);
    let Some(document) = project
        .authoring_documents
        .get(&manifest)
        .filter(|d| !d.is_deleted())
    else {
        return Ok(loaded);
    };
    let registry = crate::workspace_documents::parse_registry(&project.root, document.bytes());
    loaded.available = registry.localizations.keys().cloned().collect();
    let Some(locale) = locale else {
        return Ok(loaded);
    };
    let Some(path) = registry.localizations.get(locale) else {
        return Ok(loaded);
    };
    loaded.path = Some(
        path.strip_prefix(&project.root)
            .map_err(|_| "locale 路径越界")?
            .to_string_lossy()
            .replace('\\', "/"),
    );
    let Some(document) = project
        .authoring_documents
        .get(path)
        .filter(|d| !d.is_deleted())
    else {
        loaded.error = Some("注册的 locale sidecar 不存在".into());
        return Ok(loaded);
    };
    loaded.read_only = document.is_read_only();
    limits::budget(
        document.bytes().len() <= MAX_LOCALIZATION_JSON_BYTES,
        "sidecar JSON 字节",
    )?;
    let value = match crate::parse_unique_json(document.bytes()) {
        Ok(value) => value,
        Err(error) => {
            // Raw Project editing can repair damaged bytes, but typed locale operations cannot merge them safely.
            loaded.read_only = true;
            loaded.error = Some(format!("locale sidecar JSON 无效：{error}"));
            return Ok(loaded);
        }
    };
    if let Err(error) = document::validate_header(&value, locale, None) {
        loaded.read_only = true;
        loaded.error = Some(error);
        return Ok(loaded);
    }
    loaded.source_locale = value["source_locale"].as_str().map(str::to_owned);
    if let Some(source_locale) = &loaded.source_locale {
        limits::id(source_locale)?;
    }
    let entries = value["entries"].as_object().ok_or("locale entries 无效")?;
    limits::budget(entries.len() <= MAX_LOCALIZATION_UNITS, "sidecar 条目数")?;
    for (id, entry) in entries {
        limits::budget(id.len() <= 128, "sidecar ID 长度")?;
        let revision = entry
            .get("source_revision")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let parsed = entry
            .get("translation_parts")
            .cloned()
            .map(serde_json::from_value::<Option<Vec<LocalizationPart>>>);
        let valid_parts = matches!(&parsed, Some(Ok(_)));
        let parts = parsed.and_then(Result::ok).flatten();
        if let Some(parts) = &parts {
            limits::parts(parts)?;
        }
        loaded.entries.insert(
            id.clone(),
            StoredEntry {
                invalid: !crate::workspace_documents::valid_id(id)
                    || revision.is_none()
                    || !valid_parts,
                revision,
                parts,
            },
        );
    }
    Ok(loaded)
}

#[derive(Serialize)]
struct EntryView<'a> {
    unit_key: &'a str,
    id: Option<&'a str>,
    source: Option<&'a LocalizationSource>,
    source_revision: Option<&'a str>,
    source_parts: &'a [LocalizationPart],
    translation_parts: Option<&'a [LocalizationPart]>,
    status: LocalizationStatus,
    sidecar_path: Option<&'a str>,
    translation_pointer: Option<&'a str>,
}

fn push_entry(
    out: &mut Vec<LocalizationCatalogEntry>,
    used: &mut usize,
    entry: EntryView<'_>,
) -> Result<(), String> {
    // Every public field is counted while still borrowed, before large paths/parts are cloned.
    limits::reserve(&entry, used, "完整目录条目元数据")?;
    out.push(LocalizationCatalogEntry {
        unit_key: entry.unit_key.into(),
        id: entry.id.map(str::to_owned),
        source: entry.source.cloned(),
        source_revision: entry.source_revision.map(str::to_owned),
        source_parts: entry.source_parts.to_vec(),
        translation_parts: entry.translation_parts.map(<[_]>::to_vec),
        status: entry.status,
        sidecar_path: entry.sidecar_path.map(str::to_owned),
        translation_pointer: entry.translation_pointer.map(str::to_owned),
    });
    Ok(())
}

pub(super) fn catalog_entries(
    records: &[source::Record],
    locale: &Locale,
    source_baseline: &str,
) -> Result<Vec<LocalizationCatalogEntry>, String> {
    let mut counts = BTreeMap::new();
    for id in records.iter().filter_map(|record| record.id.as_ref()) {
        *counts.entry(id).or_insert(0usize) += 1;
    }
    let mut entries = Vec::with_capacity(records.len());
    let mut payload_bytes = 0usize;
    for record in records {
        let source = &record.unit.source;
        let stored = record.id.as_ref().and_then(|id| locale.entries.get(id));
        let status = status(record, &counts, locale, stored);
        let key = serde_json::to_vec(&(
            source_baseline,
            source,
            &record.id,
            &record.unit.source_revision,
        ))
        .map_err(|e| e.to_string())?;
        let key = export::digest("worldline-localization-location-v1", &key);
        let pointer = record.id.as_deref().map(pointer);
        push_entry(
            &mut entries,
            &mut payload_bytes,
            EntryView {
                unit_key: &key,
                id: record.id.as_deref(),
                source: Some(source),
                source_revision: Some(&record.unit.source_revision),
                source_parts: &record.unit.parts,
                translation_parts: stored.and_then(|entry| entry.parts.as_deref()),
                status,
                sidecar_path: locale.path.as_deref(),
                translation_pointer: pointer.as_deref(),
            },
        )?;
    }
    let ids: BTreeSet<_> = counts.keys().copied().collect();
    for (id, stored) in &locale.entries {
        if ids.contains(id) {
            continue;
        }
        let key = export::digest("worldline-localization-orphan-v1", id.as_bytes());
        let pointer = pointer(id);
        push_entry(
            &mut entries,
            &mut payload_bytes,
            EntryView {
                unit_key: &key,
                id: Some(id),
                source: None,
                source_revision: None,
                source_parts: &[],
                translation_parts: stored.parts.as_deref(),
                status: LocalizationStatus::OrphanTranslation,
                sidecar_path: locale.path.as_deref(),
                translation_pointer: Some(&pointer),
            },
        )?;
    }
    Ok(entries)
}

fn status(
    record: &source::Record,
    counts: &BTreeMap<&String, usize>,
    locale: &Locale,
    stored: Option<&StoredEntry>,
) -> LocalizationStatus {
    use LocalizationStatus::*;
    let Some(id) = &record.id else {
        return MissingId;
    };
    if counts.get(id).is_some_and(|count| *count > 1) {
        return DuplicateId;
    }
    if locale.error.is_some() {
        return InvalidTranslation;
    }
    let Some(stored) = stored else {
        return MissingTranslation;
    };
    if stored.invalid
        || stored
            .parts
            .as_ref()
            .is_some_and(|parts| !import::protected_tokens_match(&record.unit.parts, parts))
    {
        return InvalidTranslation;
    }
    if stored.revision.as_ref() != Some(&record.unit.source_revision) {
        return StaleSource;
    }
    if stored.parts.is_none() {
        return MissingTranslation;
    }
    Translated
}

fn pointer(id: &str) -> String {
    format!(
        "/entries/{}/translation_parts",
        id.replace('~', "~0").replace('/', "~1")
    )
}

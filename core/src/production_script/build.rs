use super::*;
use crate::localization::{LocalizationCatalogEntry, LocalizationPart, LocalizationStatus};
use std::collections::BTreeSet;

impl Project {
    pub fn production_script_snapshot(
        &self,
        buffers: &[WritingBuffer],
        drafts: &[ManuscriptQueryDraft],
        request: &ProductionScriptRequest,
    ) -> Result<ProductionScriptSnapshot, ProductionError> {
        request.validate()?;
        preflight(self, buffers, request)?;
        let input_key = self.manuscript_query_key(buffers, drafts);
        let query = Arc::new(
            self.manuscript_query_snapshot(buffers, drafts)
                .map_err(|e| ProductionError::new(e.code, e.message))?,
        );
        if !query.complete() {
            return Err(ProductionError::new(
                "INCOMPLETE_SNAPSHOT",
                "当前稿编译、编排或工作区观察不完整；未使用旧台本",
            ));
        }
        let compiled = query.compiled();
        guards::ast_envelope(compiled)?;
        if compiled.sources.len() > request.limits.source_files
            || compiled
                .sources
                .values()
                .fold(0usize, |sum, text| sum.saturating_add(text.len()))
                > request.limits.source_bytes
        {
            return Err(ProductionError::budget());
        }
        if request
            .speaker
            .as_ref()
            .is_some_and(|speaker| compiled.analysis.catalog.object(speaker).is_none())
        {
            return Err(ProductionError::new(
                "UNKNOWN_SPEAKER",
                "正式说话者不在当前稿角色目录中",
            ));
        }
        let normalized = scope::normalize(request);
        let key = digest(&("production-v1", query.key(), &input_key, &normalized));
        if request
            .expected_snapshot_key
            .as_ref()
            .is_some_and(|expected| expected != &key)
        {
            return Err(ProductionError::new(
                "STALE_SNAPSHOT",
                "台本快照已变化，请重新查询完整范围",
            ));
        }
        let locale = crate::localization::production_catalog(
            self,
            compiled,
            request.target_locale.as_deref(),
            &key,
        )
        .map_err(|e| ProductionError::new(&e.code, e.message))?;
        let entries: BTreeMap<_, _> = locale
            .into_iter()
            .filter_map(|entry| {
                let source = entry.source.as_ref()?;
                let key = (source.file.clone(), source.line, source.kind.clone());
                Some((key, entry))
            })
            .collect();
        let (roots, chapter_occurrences) = scope::roots(&query, request)?;
        let collected = collect::collect(compiled, &self.root, &roots, request)?;
        let mut rows = Vec::new();
        let mut directions = BTreeMap::new();
        let mut bytes = bounded_size(
            &(
                &collected.definitions,
                &chapter_occurrences,
                &collected.calls,
            ),
            request.limits.result_bytes,
        )?;
        let mut call_uses: BTreeMap<&TargetRef, Vec<&ProductionCallUse>> = BTreeMap::new();
        for call in &collected.calls {
            call_uses.entry(&call.callee).or_default().push(call);
        }
        for unit in collected.units {
            let kind = kind_name(unit.kind);
            let entry = entries
                .get(&(unit.source.file.clone(), unit.source.line, kind.into()))
                .ok_or_else(ProductionError::source)?;
            let status = status(entry, request.target_locale.is_some());
            if !request.statuses.is_empty() && !request.statuses.contains(&status) {
                continue;
            }
            let fallback = request.target_locale.is_some()
                && status != ProductionStatus::Translated
                && request.locale_policy == ProductionLocalePolicy::SourceFallback;
            let selected = if status == ProductionStatus::Source || fallback {
                entry.source_parts.as_slice()
            } else if status == ProductionStatus::Translated {
                entry
                    .translation_parts
                    .as_deref()
                    .ok_or_else(ProductionError::source)?
            } else {
                &[]
            };
            if !request.search.is_empty()
                && !parts_match(&entry.source_parts, &request.search)
                && !parts_match(selected, &request.search)
                && !entry
                    .id
                    .as_ref()
                    .is_some_and(|id| id.contains(&request.search))
            {
                continue;
            }
            let uses = call_uses.get(&unit.root).map(Vec::as_slice).unwrap_or(&[]);
            // 先按借用字段计量，外部调用元数据不因逐行复制越过预算。
            let size = bounded_size(
                &(
                    &unit,
                    entry.id.as_ref(),
                    &entry.source_revision,
                    &entry.source_parts,
                    selected,
                    &uses,
                ),
                request.limits.result_bytes.saturating_sub(bytes),
            )?;
            bytes = bytes
                .checked_add(size)
                .ok_or_else(ProductionError::budget)?;
            let row_key = digest(&(&key, &unit.source, unit.kind));
            if let Some(direction) = unit.direction {
                directions.insert(row_key.clone(), direction);
            }
            rows.push(ProductionRow {
                row_key,
                kind: unit.kind,
                declaration: unit.declaration,
                speaker: unit.speaker,
                source: unit.source,
                stable_line_id: entry.id.clone(),
                source_revision: entry
                    .source_revision
                    .clone()
                    .ok_or_else(ProductionError::source)?,
                source_parts: entry.source_parts.clone(),
                selected_parts: selected.to_vec(),
                status,
                target_locale: request.target_locale.clone(),
                used_source_fallback: fallback,
                control_ancestry: unit.controls,
                external_call_uses: uses.iter().map(|call| (**call).clone()).collect(),
                direction: None,
            });
        }
        let source_files = collected
            .definitions
            .iter()
            .map(|definition| definition.source.file.as_str())
            .chain(collected.calls.iter().map(|call| call.source.file.as_str()))
            .chain(rows.iter().map(|row| row.source.file.as_str()))
            .collect::<BTreeSet<_>>()
            .len();
        let summary = ProductionScopeSummary {
            selected_chapter_occurrences: chapter_occurrences.len(),
            root_targets: roots.len(),
            definition_count: collected.definitions.len(),
            added_fragment_definitions: collected.added_fragments,
            call_sites: collected.calls.len(),
            source_files,
            matching_rows: rows.len(),
            source_only: request.target_locale.is_none(),
            includes_fragment_closure: request.include_fragments
                || matches!(request.scope, ProductionScope::Project),
            source_fallback_rows: rows.iter().filter(|row| row.used_source_fallback).count(),
            complete: true,
        };
        bounded_size(
            &(
                &key,
                &summary,
                &collected.definitions,
                &chapter_occurrences,
                &collected.calls,
                &rows,
                &directions,
            ),
            request.limits.result_bytes,
        )?;
        let snapshot = ProductionScriptSnapshot {
            key,
            input_key,
            root: self.root.clone(),
            request: normalized,
            query,
            summary,
            definitions: collected.definitions,
            chapter_occurrences,
            call_sites: collected.calls,
            rows,
            directions,
        };
        self.validate_production_script(buffers, drafts, &snapshot)?;
        Ok(snapshot)
    }
}
fn preflight(
    project: &Project,
    buffers: &[WritingBuffer],
    request: &ProductionScriptRequest,
) -> Result<(), ProductionError> {
    if project
        .documents
        .len()
        .saturating_add(project.authoring_documents.len())
        > 4096
        || buffers.len() > 4096
    {
        return Err(ProductionError::budget());
    }
    let mut seen = BTreeSet::new();
    let baseline = project.content_baseline();
    for buffer in buffers {
        if !seen.insert(buffer.path()) {
            return Err(ProductionError::new(
                "DUPLICATE_DRAFT",
                "同一文件只允许一个 WritingBuffer；相同内容重复项也不被静默合并",
            ));
        }
        if buffer.is_changed() && buffer.baseline() != baseline {
            return Err(ProductionError::new(
                "STALE_DRAFT",
                "正文草稿基线已变化，输入保留",
            ));
        }
    }
    let all_bytes = project
        .documents
        .values()
        .fold(0usize, |sum, doc| {
            sum.saturating_add(doc.text.len())
                .saturating_add(doc.saved_byte_len())
        })
        .saturating_add(
            project
                .authoring_documents
                .values()
                .fold(0usize, |sum, doc| {
                    sum.saturating_add(doc.bytes().len())
                        .saturating_add(doc.saved.as_ref().map_or(0, Vec::len))
                }),
        );
    if all_bytes > 256 * 1024 * 1024 {
        return Err(ProductionError::budget());
    }
    let mut bytes = 0usize;
    let mut files = 0usize;
    for (path, doc) in &project.documents {
        if doc.is_deleted()
            || project
                .source_selection()
                .is_some_and(|selection| !selection.is_active(path))
        {
            continue;
        }
        files += 1;
        let length = buffers
            .iter()
            .find(|buffer| buffer.path() == path && buffer.is_changed())
            .map_or(doc.text.len(), |buffer| buffer.source().len());
        bytes = bytes.saturating_add(length);
        if bytes > request.limits.source_bytes || files > request.limits.source_files {
            return Err(ProductionError::budget());
        }
    }
    Ok(())
}
fn status(entry: &LocalizationCatalogEntry, localized: bool) -> ProductionStatus {
    if !localized {
        return ProductionStatus::Source;
    }
    match entry.status {
        LocalizationStatus::Translated => ProductionStatus::Translated,
        LocalizationStatus::MissingId | LocalizationStatus::MissingTranslation => {
            ProductionStatus::Missing
        }
        LocalizationStatus::StaleSource => ProductionStatus::Stale,
        _ => ProductionStatus::Invalid,
    }
}
fn parts_match(parts: &[LocalizationPart], search: &str) -> bool {
    parts.iter().any(|part| match part {
        LocalizationPart::Text { text } => text.contains(search),
        LocalizationPart::Link { label, .. } => label.contains(search),
        LocalizationPart::Placeholder { .. } => false,
    })
}
pub(super) fn kind_name(kind: ProductionKind) -> &'static str {
    match kind {
        ProductionKind::Say => "say",
        ProductionKind::Text => "text",
        ProductionKind::Choice => "choice",
    }
}

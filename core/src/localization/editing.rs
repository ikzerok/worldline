use super::*;
use crate::project::Project;

impl Project {
    /// Preview an explicit exchange update against current, possibly unsaved buffers.
    pub fn preview_localization_import_candidate(
        &self,
        selection: &LocalizationSelection,
        exchange: &LocalizationExchange,
    ) -> Result<LocalizationImportPlan, LocalizationError> {
        prepare(self, selection, exchange, None)
            .map(|prepared| prepared.plan)
            .map_err(LocalizationError::from)
    }

    /// One atomic in-memory replacement. The mounted WASM files and native disk stay unchanged.
    pub fn apply_localization_import_candidate(
        &mut self,
        selection: &LocalizationSelection,
        exchange: &LocalizationExchange,
        expected_plan_digest: &str,
    ) -> Result<LocalizationImportResult, LocalizationError> {
        apply(self, selection, exchange, expected_plan_digest, None)
            .map_err(LocalizationError::from)
    }

    pub fn preview_localization_edit(
        &self,
        draft: &LocalizationEditDraft,
    ) -> Result<LocalizationImportPlan, LocalizationError> {
        let (selection, exchange, records) =
            edit_exchange(self, draft).map_err(LocalizationError::from)?;
        prepare(self, &selection, &exchange, Some(&records))
            .map(|prepared| prepared.plan)
            .map_err(LocalizationError::from)
    }

    pub fn apply_localization_edit(
        &mut self,
        draft: &LocalizationEditDraft,
        expected_plan_digest: &str,
    ) -> Result<LocalizationImportResult, LocalizationError> {
        let (selection, exchange, records) =
            edit_exchange(self, draft).map_err(LocalizationError::from)?;
        apply(
            self,
            &selection,
            &exchange,
            expected_plan_digest,
            Some(&records),
        )
        .map_err(LocalizationError::from)
    }
}

pub(super) fn guard(project: &Project) -> Result<(), String> {
    limits::project(project)?;
    if !project.authoring_diagnostics().is_empty() {
        return Err("READ_ONLY：工作区清单含不支持的格式或能力，不能编辑译文".into());
    }
    project
        .verify_review_navigation()
        .map_err(|error| format!("EXTERNAL_CONFLICT：{error}"))
}

fn prepare(
    project: &Project,
    selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    records: Option<&[source::Record]>,
) -> Result<import::PreparedImport, String> {
    limits::exchange(exchange)?;
    limits::budget(
        selection.string_ids.len() <= MAX_LOCALIZATION_BATCH,
        "选择 ID 数",
    )?;
    limits::id(&selection.source_locale)?;
    limits::id(&selection.target_locale)?;
    for id in &selection.string_ids {
        limits::id(id)?;
    }
    guard(project)?;
    if !project.compile_options().localization_ids {
        return Err("FEATURE_REQUIRED：请先显式启用 content.localization.v1".into());
    }
    // Inspect all active source units and the selected locale before legacy validation can clone them.
    let collected;
    let records = match records {
        Some(records) => records,
        None => {
            collected = source::current(project)?.1;
            &collected
        }
    };
    let locale = catalog::load_locale(project, Some(&selection.target_locale))?;
    let additions: std::collections::BTreeSet<_> = exchange
        .entries
        .iter()
        .map(|entry| &entry.id)
        .filter(|id| !locale.entries.contains_key(*id))
        .collect();
    limits::budget(
        locale.entries.len().saturating_add(additions.len()) <= MAX_LOCALIZATION_UNITS,
        "候选 sidecar 条目数",
    )?;
    let mut prepared = import::prepare_import(project, selection, exchange, Some(records))?;
    if let Some(bytes) = &prepared.sidecar_bytes {
        limits::budget(
            bytes.len() <= MAX_LOCALIZATION_JSON_BYTES,
            "候选 sidecar JSON 字节",
        )?;
    }
    if let Some(bytes) = &prepared.manifest_bytes {
        limits::budget(
            bytes.len() <= MAX_LOCALIZATION_JSON_BYTES,
            "候选清单 JSON 字节",
        )?;
    }
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let mut updates = Vec::with_capacity(2);
    if let Some(bytes) = &prepared.manifest_bytes {
        updates.push(limits::AuthoringProjection {
            path: &manifest_path,
            bytes,
            create: false,
        });
    }
    if let Some(bytes) = &prepared.sidecar_bytes {
        updates.push(limits::AuthoringProjection {
            path: &prepared.sidecar_path,
            bytes,
            create: prepared.create_sidecar,
        });
    }
    limits::projected_authoring(project, &updates)?;
    let manifest_unchanged = prepared.manifest_bytes.as_ref().is_none_or(|bytes| {
        project
            .authoring_documents
            .get(&crate::workspace_documents::manifest_path(&project.root))
            .is_some_and(|document| document.bytes() == bytes)
    });
    let sidecar_unchanged = prepared.sidecar_bytes.as_ref().is_some_and(|bytes| {
        project
            .authoring_documents
            .get(&prepared.sidecar_path)
            .is_some_and(|document| !document.is_deleted() && document.bytes() == bytes)
    });
    if prepared.plan.can_apply && manifest_unchanged && sidecar_unchanged {
        prepared.plan.can_apply = false;
        prepared.plan.diagnostics.push(LocalizationDiagnostic {
            code: "NO_CHANGE".into(),
            id: None,
            source: None,
            message: "所选译文已与当前缓冲一致，无需重复应用".into(),
        });
    }
    let identity = serde_json::to_vec(&(&prepared.plan, project.search_refresh_generation()))
        .map_err(|e| e.to_string())?;
    prepared.plan.plan_digest =
        export::digest("worldline-localization-candidate-plan-v1", &identity);
    limits::serialized(&prepared.plan, limits::MAX_OUTPUT_BYTES, "导入计划输出")?;
    Ok(prepared)
}

fn apply(
    project: &mut Project,
    selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    expected: &str,
    records: Option<&[source::Record]>,
) -> Result<LocalizationImportResult, String> {
    let prepared = prepare(project, selection, exchange, records)?;
    if prepared.plan.plan_digest != expected {
        return Err("STALE_PLAN：本地化候选预览已过期，请重新预览".into());
    }
    if !prepared.plan.can_apply {
        let diagnostic = prepared
            .plan
            .diagnostics
            .first()
            .ok_or("本地化候选未通过校验")?;
        return Err(format!("{}：{}", diagnostic.code, diagnostic.message));
    }
    let baseline = project.content_baseline();
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let mut candidate = project.clone();
    let mut changed_files = Vec::new();
    if let Some(bytes) = prepared.manifest_bytes {
        crate::source_lifecycle::safety::writable_path(&manifest_path)?;
        candidate.set_authoring_document(&manifest_path, bytes)?;
        changed_files.push(manifest_path);
    }
    crate::source_lifecycle::safety::writable_path(&prepared.sidecar_path)?;
    let bytes = prepared.sidecar_bytes.ok_or("本地化候选没有生成译文文档")?;
    if prepared.create_sidecar {
        candidate.create_authoring_document(&prepared.sidecar_path, bytes)?;
    } else {
        candidate.set_authoring_document(&prepared.sidecar_path, bytes)?;
    }
    changed_files.push(prepared.sidecar_path);
    changed_files.sort();
    crate::source_lifecycle::safety::buffer_budget(&candidate)?;
    limits::project(&candidate)?;
    guard(project)?;
    let new_baseline = candidate.content_baseline();
    *project = candidate;
    Ok(LocalizationImportResult {
        plan: prepared.plan,
        changed_files,
        baseline,
        new_baseline,
    })
}

fn edit_exchange(
    project: &Project,
    draft: &LocalizationEditDraft,
) -> Result<
    (
        LocalizationSelection,
        LocalizationExchange,
        Vec<source::Record>,
    ),
    String,
> {
    if draft.schema_version != 1 {
        return Err("UNSUPPORTED_VERSION：不支持的译文编辑版本".into());
    }
    limits::budget(
        draft.edits.len() <= MAX_LOCALIZATION_BATCH,
        "译文编辑条目数",
    )?;
    limits::id(&draft.source_locale)?;
    limits::id(&draft.target_locale)?;
    let mut seen = std::collections::BTreeSet::new();
    for edit in &draft.edits {
        limits::id(&edit.id)?;
        limits::parts(&edit.translation_parts)?;
        if !seen.insert(&edit.id) {
            return Err("DUPLICATE_ID：译文编辑不能重复选择同一稳定 ID".into());
        }
    }
    limits::serialized(draft, MAX_LOCALIZATION_JSON_BYTES, "译文编辑输入")?;
    let selection = LocalizationSelection {
        schema_version: 1,
        source_locale: draft.source_locale.clone(),
        target_locale: draft.target_locale.clone(),
        string_ids: draft.edits.iter().map(|edit| edit.id.clone()).collect(),
    };
    let selection = export::normalize_selection(&selection)?;
    guard(project)?;
    if !project.compile_options().localization_ids {
        return Err("FEATURE_REQUIRED：请先显式启用 content.localization.v1".into());
    }
    let (_, records) = source::current(project)?;
    let mut by_id = std::collections::BTreeMap::<&str, Vec<&SourceUnit>>::new();
    for record in &records {
        if let Some(id) = &record.id {
            by_id.entry(id).or_default().push(&record.unit);
        }
    }
    let edits: std::collections::BTreeMap<_, _> = draft
        .edits
        .iter()
        .map(|edit| (edit.id.as_str(), edit))
        .collect();
    let mut exchange = LocalizationExchange {
        schema_version: 1,
        source_locale: selection.source_locale.clone(),
        target_locale: selection.target_locale.clone(),
        source_baseline: draft.source_baseline.clone(),
        string_ids: selection.string_ids.clone(),
        entries: Vec::new(),
    };
    // The protected payload comes from core's exact applied-source snapshot, never from caller text.
    for id in &selection.string_ids {
        let matches = by_id
            .get(id.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        if matches.is_empty() {
            return Err(format!("UNKNOWN_ID：当前活动源码没有稳定 ID `{id}`"));
        }
        if matches.len() != 1 {
            return Err(format!("DUPLICATE_ID：当前活动源码的稳定 ID `{id}` 不唯一"));
        }
        let unit = matches[0];
        let edit = edits[id.as_str()];
        exchange.entries.push(LocalizationExchangeEntry {
            id: id.clone(),
            source_revision: edit.source_revision.clone(),
            source: unit.source.clone(),
            source_parts: unit.parts.clone(),
            translation_parts: Some(edit.translation_parts.clone()),
        });
    }
    Ok((selection, exchange, records))
}

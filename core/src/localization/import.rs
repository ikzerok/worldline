use super::export::{collect_program_units, digest, normalize_selection, source_baseline};
use super::*;
use crate::project::Project;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct PreparedImport {
    pub plan: LocalizationImportPlan,
    pub sidecar_path: PathBuf,
    pub manifest_bytes: Option<Vec<u8>>,
    pub sidecar_bytes: Option<Vec<u8>>,
    pub create_sidecar: bool,
}

impl Project {
    /// Validate a translator-edited exchange package without changing Project buffers.
    pub fn preview_localization_import(
        &self,
        selection: &LocalizationSelection,
        exchange: &LocalizationExchange,
    ) -> Result<LocalizationImportPlan, String> {
        Ok(prepare_import(self, selection, exchange, None)?.plan)
    }

    /// Revalidate and persist the complete locale update before replacing this Project buffer.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_localization_import(
        &mut self,
        selection: &LocalizationSelection,
        exchange: &LocalizationExchange,
        expected_plan_digest: &str,
    ) -> Result<LocalizationImportResult, String> {
        if self.is_dirty() {
            return Err("请先保存或撤销工程修改，再应用本地化译文".into());
        }
        let mut refreshed = self.clone();
        let conflicts = refreshed.refresh()?;
        if !conflicts.is_empty() {
            return Err("工程存在外部刷新冲突，不能应用本地化译文".into());
        }
        let prepared = prepare_import(&refreshed, selection, exchange, None)?;
        if prepared.plan.plan_digest != expected_plan_digest {
            return Err("本地化导入预览已过期，请重新预览".into());
        }
        if !prepared.plan.can_apply {
            return Err(prepared
                .plan
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
                .unwrap_or_else(|| "本地化导入未通过校验".into()));
        }

        let baseline = refreshed.content_baseline();
        let manifest_path = crate::workspace_documents::manifest_path(&refreshed.root);
        let mut candidate = refreshed;
        let manifest_changed = prepared.manifest_bytes.is_some();
        if let Some(bytes) = prepared.manifest_bytes {
            candidate.set_authoring_document(&manifest_path, bytes)?;
        }
        let sidecar_bytes = prepared
            .sidecar_bytes
            .ok_or("本地化导入没有生成 sidecar 内容")?;
        if prepared.create_sidecar {
            candidate.create_authoring_document(&prepared.sidecar_path, sidecar_bytes)?;
        } else {
            candidate.set_authoring_document(&prepared.sidecar_path, sidecar_bytes)?;
        }
        candidate.save()?;
        let new_baseline = candidate.content_baseline();
        let mut changed_files = Vec::with_capacity(2);
        if manifest_changed {
            changed_files.push(manifest_path);
        }
        changed_files.push(prepared.sidecar_path);
        changed_files.sort();
        let plan = prepared.plan;
        *self = candidate;
        Ok(LocalizationImportResult {
            plan,
            changed_files,
            baseline,
            new_baseline,
        })
    }
}

pub(super) fn prepare_import(
    project: &Project,
    requested_selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    records: Option<&[source::Record]>,
) -> Result<PreparedImport, String> {
    let candidate = records.is_some();
    let selection = normalize_selection(requested_selection)?;
    let content_baseline = project.content_baseline();
    let source_baseline = source_baseline(project)?;
    let (sidecar_path, sidecar_registered) =
        localization_sidecar_path(project, &selection.target_locale)?;
    let sidecar_relative = sidecar_path
        .strip_prefix(&project.root)
        .map_err(|_| "本地化 sidecar 路径越出工作区")?
        .to_str()
        .ok_or("本地化 sidecar 路径不是 UTF-8")?
        .replace('\\', "/");
    let mut diagnostics = Vec::new();
    let mut current_units = BTreeMap::<String, Vec<SourceUnit>>::new();

    if exchange.schema_version != LOCALIZATION_SCHEMA_VERSION {
        diagnostic(
            &mut diagnostics,
            "UNSUPPORTED_VERSION",
            None,
            None,
            "不支持的本地化交换格式版本",
        );
    }
    if exchange.source_locale != selection.source_locale
        || exchange.target_locale != selection.target_locale
        || exchange.string_ids != selection.string_ids
    {
        diagnostic(
            &mut diagnostics,
            "SELECTION_MISMATCH",
            None,
            None,
            "导入 selection 必须与交换包的 locale 和 ID 白名单完全一致",
        );
    }
    if !project.compile_options().localization_ids {
        diagnostic(
            &mut diagnostics,
            "FEATURE_REQUIRED",
            None,
            None,
            format!("工程清单必须声明 required_features {LOCALIZATION_REQUIRED_FEATURE}"),
        );
    }
    if !project.authoring_diagnostics().is_empty() {
        diagnostic(
            &mut diagnostics,
            "READ_ONLY",
            None,
            None,
            "工程存在工作区诊断，只能只读查看",
        );
    }
    if !candidate && project.is_dirty() {
        diagnostic(
            &mut diagnostics,
            "DIRTY_PROJECT",
            None,
            None,
            "请先保存或撤销工程修改，再预览本地化导入",
        );
    }

    if let Some(records) = records {
        // New candidate callers already checked this exact source snapshot and every unit budget.
        // Keep only the explicit batch here; unselected units never expand the import scope.
        for record in records {
            if let Some(id) = &record.id {
                if selection.string_ids.binary_search(id).is_ok() {
                    current_units
                        .entry(id.clone())
                        .or_default()
                        .push(record.unit.clone());
                }
            }
        }
    } else {
        let mut compile_project = project.clone();
        #[cfg(all(test, not(target_arch = "wasm32")))]
        source::record_compilation();
        let compiled = compile_project.compile();
        if compiled.has_errors() {
            diagnostic(
                &mut diagnostics,
                "COMPILE_ERROR",
                None,
                None,
                "工程存在编译错误，不能验证本地化导入",
            );
        } else {
            current_units = collect_program_units(&compiled.program, &project.root)?;
        }
    }
    if exchange.source_baseline != source_baseline {
        diagnostic(
            &mut diagnostics,
            "BASELINE_MISMATCH",
            None,
            None,
            "交换包的源码基线已过期，请重新导出",
        );
    }

    let selected: BTreeSet<_> = selection.string_ids.iter().map(String::as_str).collect();
    let mut package_entries = BTreeMap::<&str, Vec<&LocalizationExchangeEntry>>::new();
    for entry in &exchange.entries {
        package_entries.entry(&entry.id).or_default().push(entry);
        if !selected.contains(entry.id.as_str()) {
            diagnostic(
                &mut diagnostics,
                "UNKNOWN_ID",
                Some(&entry.id),
                None,
                "交换包包含未授权的本地化字符串 ID",
            );
        }
    }
    for id in &selection.string_ids {
        let source_units = current_units.get(id).map(Vec::as_slice).unwrap_or_default();
        let Some(package_matches) = package_entries.get(id.as_str()).map(Vec::as_slice) else {
            diagnostic(
                &mut diagnostics,
                "MISSING_ENTRY",
                Some(id),
                source_units.first().map(|unit| &unit.source),
                "交换包缺少 selection 中的字符串",
            );
            continue;
        };
        if package_matches.len() != 1 {
            diagnostic(
                &mut diagnostics,
                "DUPLICATE_ID",
                Some(id),
                source_units.first().map(|unit| &unit.source),
                "交换包中的本地化字符串 ID 重复",
            );
            continue;
        }
        if source_units.len() != 1 {
            diagnostic(
                &mut diagnostics,
                if source_units.is_empty() {
                    "UNKNOWN_ID"
                } else {
                    "DUPLICATE_ID"
                },
                Some(id),
                source_units.first().map(|unit| &unit.source),
                "当前活动源码中的本地化字符串 ID 缺失或重复",
            );
            continue;
        }
        let unit = &source_units[0];
        let entry = package_matches[0];
        if entry.source_revision != unit.source_revision {
            diagnostic(
                &mut diagnostics,
                "STALE_SOURCE",
                Some(id),
                Some(&unit.source),
                "源文已改变，此译文需要复核并重新导出",
            );
        }
        if entry.source != unit.source || entry.source_parts != unit.parts {
            diagnostic(
                &mut diagnostics,
                "SOURCE_MISMATCH",
                Some(id),
                Some(&unit.source),
                "交换包的源引用或受保护源片段与当前工程不匹配；请按诊断中的真实来源重新导出交换包，原输入未修改",
            );
        }
        match &entry.translation_parts {
            None => diagnostic(
                &mut diagnostics,
                "MISSING_TRANSLATION",
                Some(id),
                Some(&unit.source),
                "选中的字符串缺少译文",
            ),
            Some(parts) if !protected_tokens_match(&unit.parts, parts) => diagnostic(
                &mut diagnostics,
                "INVALID_TOKEN",
                Some(id),
                Some(&unit.source),
                "译文添加、丢失、重复或改写了受保护占位符/链接 token",
            ),
            Some(_) => {}
        }
    }

    let mut manifest_bytes = None;
    let mut sidecar_bytes = None;
    let mut create_sidecar = false;
    if diagnostics.is_empty() {
        match super::sidecar::prepare_sidecar(
            project,
            &sidecar_path,
            sidecar_registered,
            exchange,
            candidate,
        ) {
            Ok(sidecar) => {
                manifest_bytes = sidecar.manifest_bytes;
                sidecar_bytes = Some(sidecar.bytes);
                create_sidecar = sidecar.create;
            }
            Err(message) => diagnostic(&mut diagnostics, "SIDECAR_INVALID", None, None, message),
        }
    }
    if candidate {
        limits::budget(diagnostics.len() <= limits::MAX_DIAGNOSTICS, "诊断数")?;
    }
    let can_apply = diagnostics.is_empty();
    let plan_digest = import_plan_digest(
        &selection,
        exchange,
        &content_baseline,
        &source_baseline,
        &sidecar_relative,
        &diagnostics,
    )?;
    Ok(PreparedImport {
        plan: LocalizationImportPlan {
            schema_version: LOCALIZATION_SCHEMA_VERSION,
            plan_digest,
            content_baseline,
            source_baseline,
            target_locale: selection.target_locale,
            affected_ids: selection.string_ids.clone(),
            sidecar_path: sidecar_relative,
            diagnostics,
            can_apply,
        },
        sidecar_path,
        manifest_bytes,
        sidecar_bytes,
        create_sidecar,
    })
}

fn diagnostic(
    diagnostics: &mut Vec<LocalizationDiagnostic>,
    code: &str,
    id: Option<&str>,
    source: Option<&LocalizationSource>,
    message: impl Into<String>,
) {
    diagnostics.push(LocalizationDiagnostic {
        code: code.into(),
        id: id.map(str::to_string),
        source: source.cloned(),
        message: message.into(),
    });
}

pub(super) fn protected_tokens_match(
    source: &[LocalizationPart],
    translation: &[LocalizationPart],
) -> bool {
    fn tokens(parts: &[LocalizationPart]) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
        let mut placeholders = BTreeMap::new();
        let mut links = BTreeMap::new();
        for part in parts {
            match part {
                LocalizationPart::Placeholder { token } => {
                    *placeholders.entry(token.clone()).or_insert(0) += 1;
                }
                LocalizationPart::Link { token, .. } => {
                    *links.entry(token.clone()).or_insert(0) += 1;
                }
                LocalizationPart::Text { .. } => {}
            }
        }
        (placeholders, links)
    }
    tokens(source) == tokens(translation)
}

fn localization_sidecar_path(project: &Project, locale: &str) -> Result<(PathBuf, bool), String> {
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let manifest = project
        .authoring_documents
        .get(&manifest_path)
        .filter(|document| !document.is_deleted())
        .ok_or("本地化需要已载入的工程清单")?;
    let registry = crate::workspace_documents::parse_registry(&project.root, manifest.bytes());
    if let Some(path) = registry.localizations.get(locale) {
        return Ok((path.clone(), true));
    }
    let relative = format!(".world/localization/{locale}.json");
    let path = crate::workspace_documents::registered_path(&project.root, &relative)?;
    Ok((path, false))
}

fn import_plan_digest(
    selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    content_baseline: &str,
    source_baseline: &str,
    sidecar_path: &str,
    diagnostics: &[LocalizationDiagnostic],
) -> Result<String, String> {
    let bytes = serde_json::to_vec(&(
        selection,
        exchange,
        content_baseline,
        source_baseline,
        sidecar_path,
        diagnostics,
    ))
    .map_err(|error| format!("无法序列化本地化导入计划：{error}"))?;
    Ok(digest("worldline-localization-import-plan-v1", &bytes))
}

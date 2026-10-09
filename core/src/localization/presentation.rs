use super::*;
use crate::{ast::Program, project::Project, Analysis};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalizationPresentationPolicy {
    Strict,
    SourceFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LocalizationPresentationRequest {
    pub schema_version: u32,
    pub target_locale: String,
    pub policy: LocalizationPresentationPolicy,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalizationPresentationEntry {
    pub id: Option<String>,
    pub source: LocalizationSource,
    pub source_revision: String,
    pub source_parts: Vec<LocalizationPart>,
    pub translation_parts: Option<Vec<LocalizationPart>>,
    pub status: LocalizationStatus,
    pub sidecar_path: Option<String>,
    pub translation_pointer: Option<String>,
}

/// Only core can construct this proof. JSON describes it but cannot reconstruct a trusted snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct LocalizationPresentationSnapshot {
    target_locale: String,
    policy: LocalizationPresentationPolicy,
    program_fingerprint: String,
    source_baseline: String,
    presentation_digest: String,
    entries: Vec<LocalizationPresentationEntry>,
    #[serde(skip)]
    root: PathBuf,
}

impl LocalizationPresentationSnapshot {
    pub fn target_locale(&self) -> &str {
        &self.target_locale
    }
    pub fn policy(&self) -> LocalizationPresentationPolicy {
        self.policy
    }
    pub fn program_fingerprint(&self) -> &str {
        &self.program_fingerprint
    }
    pub fn source_baseline(&self) -> &str {
        &self.source_baseline
    }
    pub fn presentation_digest(&self) -> &str {
        &self.presentation_digest
    }
    pub fn entries(&self) -> &[LocalizationPresentationEntry] {
        &self.entries
    }

    pub fn find_entry(
        &self,
        source_file: &str,
        line: u32,
        kind: &str,
    ) -> Option<&LocalizationPresentationEntry> {
        let file = Path::new(source_file)
            .strip_prefix(&self.root)
            .ok()?
            .to_str()?
            .replace('\\', "/");
        self.entries
            .binary_search_by(|entry| {
                (
                    entry.source.file.as_str(),
                    entry.source.line,
                    entry.source.kind.as_str(),
                )
                    .cmp(&(file.as_str(), line, kind))
            })
            .ok()
            .and_then(|index| self.entries.get(index))
    }

    /// Source IDs and coordinates are presentation identity even when runtime fingerprint stays equal.
    pub fn validate_program(
        &self,
        program: &Program,
        analysis: &Analysis,
    ) -> Result<(), LocalizationError> {
        self.validate(program, analysis)
            .map_err(LocalizationError::from)
    }

    fn validate(&self, program: &Program, analysis: &Analysis) -> Result<(), String> {
        let fingerprint = crate::fingerprint_program(program).to_string();
        if fingerprint != self.program_fingerprint
            || analysis.fingerprint.to_string() != fingerprint
        {
            return Err("INVALID_PRESENTATION：展示快照不属于当前运行程序".into());
        }
        let records = source::collect(program, &self.root)?;
        if records.len() != self.entries.len() {
            return Err("INVALID_PRESENTATION：展示快照的正文单元集合不匹配".into());
        }
        for (record, entry) in records.iter().zip(&self.entries) {
            if record.id != entry.id
                || record.unit.source != entry.source
                || record.unit.source_revision != entry.source_revision
                || record.unit.parts != entry.source_parts
            {
                return Err("INVALID_PRESENTATION：展示快照的正文身份、位置或源修订已变化".into());
            }
        }
        Ok(())
    }
}

impl Project {
    pub fn prepare_localization_presentation(
        &self,
        request: &LocalizationPresentationRequest,
    ) -> Result<LocalizationPresentationSnapshot, LocalizationError> {
        prepare(self, request).map_err(LocalizationError::from)
    }

    /// Uses the actual compiled draft sources; never applies them to the author's Project.
    pub fn prepare_draft_localization_presentation(
        &self,
        draft: &crate::draft_rehearsal::DraftRehearsalSnapshot,
        request: &LocalizationPresentationRequest,
    ) -> Result<LocalizationPresentationSnapshot, LocalizationError> {
        self.check_localization_budget()?;
        draft
            .verify_localization_project(self)
            .map_err(|e| LocalizationError::new("STALE_SOURCE", e))?;
        let mut candidate = self.clone();
        for (path, source) in &draft.compiled().sources {
            candidate
                .set_text(path, source.clone())
                .map_err(LocalizationError::from)?;
        }
        let presentation = candidate.prepare_localization_presentation(request)?;
        presentation.validate_program(&draft.compiled().program, &draft.compiled().analysis)?;
        Ok(presentation)
    }
}

fn prepare(
    project: &Project,
    request: &LocalizationPresentationRequest,
) -> Result<LocalizationPresentationSnapshot, String> {
    if request.schema_version != 1 {
        return Err("UNSUPPORTED_VERSION：不支持的 locale 展示请求版本".into());
    }
    limits::id(&request.target_locale)?;
    limits::project(project)?;
    if !project.authoring_diagnostics().is_empty() {
        return Err("READ_ONLY：工作区有不支持的格式或能力，不能准备运行展示".into());
    }
    if !project.compile_options().localization_ids {
        return Err("FEATURE_REQUIRED：locale 展示需要 content.localization.v1".into());
    }
    let (compiled, records) = source::current(project)?;
    let locale = catalog::load_locale(project, Some(&request.target_locale))?;
    if locale.path.is_none() {
        return Err("UNKNOWN_LOCALE：工程没有登记此 locale".into());
    }
    if let Some(error) = &locale.error {
        return Err(format!("SIDECAR_INVALID：{error}"));
    }
    let source_baseline = export::source_baseline(project)?;
    let catalog = catalog::catalog_entries(&records, &locale, &source_baseline)?;
    let mut entries: Vec<LocalizationPresentationEntry> = Vec::with_capacity(records.len());
    for entry in catalog.into_iter().filter(|entry| entry.source.is_some()) {
        if request.policy == LocalizationPresentationPolicy::Strict
            && entry.status != LocalizationStatus::Translated
        {
            return Err(format!("INVALID_PRESENTATION：严格 locale 展示需要每个正文单元都有当前有效译文（{}：{:?}）", entry.id.as_deref().unwrap_or("无稳定 ID"), entry.status));
        }
        let source = entry.source.ok_or("正文来源缺失")?;
        if entries.last().is_some_and(|entry| entry.source == source) {
            return Err("INVALID_PRESENTATION：同一正文来源对应多个声明".into());
        }
        let translation_parts = (entry.status == LocalizationStatus::Translated)
            .then_some(entry.translation_parts)
            .flatten();
        entries.push(LocalizationPresentationEntry {
            id: entry.id,
            source,
            source_revision: entry.source_revision.ok_or("正文源修订缺失")?,
            source_parts: entry.source_parts,
            translation_parts,
            status: entry.status,
            sidecar_path: entry.sidecar_path,
            translation_pointer: entry.translation_pointer,
        });
    }
    // Presentation content and policy have their own identity, separate from executable semantics.
    let mut content = Vec::with_capacity(entries.len());
    let mut content_size = 0usize;
    for entry in &entries {
        let canonical = serde_json::to_string(&(
            &entry.id,
            &entry.source_revision,
            &entry.source_parts,
            &entry.translation_parts,
            entry.status,
        ))
        .map_err(|e| e.to_string())?;
        content_size = content_size.saturating_add(canonical.len());
        limits::budget(content_size <= 64 * 1024 * 1024, "展示快照内容")?;
        content.push(canonical);
    }
    // Sort the exact content tuples, including unassigned fallback units; source movement is not display content.
    content.sort();
    limits::serialized(&content, 64 * 1024 * 1024, "展示快照内容")?;
    let bytes = serde_json::to_vec(&(&request.target_locale, request.policy, content))
        .map_err(|e| e.to_string())?;
    let snapshot = LocalizationPresentationSnapshot {
        target_locale: request.target_locale.clone(),
        policy: request.policy,
        program_fingerprint: compiled.analysis.fingerprint.to_string(),
        source_baseline,
        presentation_digest: export::digest("worldline-localization-presentation-v1", &bytes),
        entries,
        root: project.root.clone(),
    };
    limits::serialized(&snapshot, limits::MAX_SNAPSHOT_BYTES, "完整展示快照 DTO")?;
    Ok(snapshot)
}

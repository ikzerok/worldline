use super::*;
use crate::project::Project;
use serde::de::DeserializeOwned;
use std::path::Path;

pub const MAX_LOCALIZATION_JSON_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_LOCALIZATION_UNITS: usize = 50_000;
pub const MAX_LOCALIZATION_BATCH: usize = 1_000;
pub const MAX_LOCALIZATION_PARTS: usize = 4_096;
pub const MAX_LOCALIZATION_UNIT_BYTES: usize = 64 * 1024;
pub const MAX_LOCALIZATION_PAGE_SIZE: usize = 200;
pub const MAX_LOCALIZATION_SOURCE_LINES: usize = 200_000;
pub const MAX_LOCALIZATION_TRACKED_FILES: usize = 4_096;
pub const MAX_LOCALIZATION_CLONE_BYTES: usize = 256 * 1024 * 1024;
pub(super) const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
pub(super) const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub(super) const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_DIAGNOSTICS: usize = 200;

pub(super) fn budget(allowed: bool, what: &str) -> Result<(), String> {
    if allowed {
        Ok(())
    } else {
        Err(format!("BUDGET_EXCEEDED：本地化{what}超过预算"))
    }
}

pub(super) fn id(value: &str) -> Result<(), String> {
    budget(value.len() <= 128, " ID/locale 长度")?;
    if !crate::workspace_documents::valid_id(value) {
        return Err("INVALID_ID：本地化 ID/locale 必须是有效 ASCII 标识符".into());
    }
    Ok(())
}

pub(super) fn parts(parts: &[LocalizationPart]) -> Result<(), String> {
    budget(parts.len() <= MAX_LOCALIZATION_PARTS, " typed parts 数")?;
    let bytes = parts.iter().fold(0usize, |size, part| {
        size.saturating_add(match part {
            LocalizationPart::Text { text } => text.len(),
            LocalizationPart::Placeholder { token } => token.len(),
            LocalizationPart::Link { token, label } => token.len().saturating_add(label.len()),
        })
    });
    budget(bytes <= MAX_LOCALIZATION_UNIT_BYTES, "单元 UTF-8 字节")
}

pub(super) fn project(project: &Project) -> Result<(), String> {
    clone_envelope(project)?;
    let manifest = crate::workspace_documents::manifest_path(&project.root);
    if let Some(document) = project.authoring_documents.get(&manifest) {
        budget(
            document.bytes().len() <= MAX_LOCALIZATION_JSON_BYTES,
            "工程清单 JSON 字节",
        )?;
    }
    let mut size = 0usize;
    let mut lines = 0usize;
    for (path, document) in &project.documents {
        if document.is_deleted()
            || project
                .source_selection()
                .is_some_and(|s| !s.is_active(path))
        {
            continue;
        }
        size = size.saturating_add(document.text.len());
        budget(size <= MAX_SOURCE_BYTES, "活动源码总字节")?;
        lines = lines.saturating_add(physical_lines(&document.text));
        budget(lines <= MAX_LOCALIZATION_SOURCE_LINES, "活动源码物理行数")?;
    }
    Ok(())
}

pub(super) fn physical_lines(text: &str) -> usize {
    text.lines().count()
}

fn clone_envelope(project: &Project) -> Result<(), String> {
    clone_envelope_with_limit(project, MAX_LOCALIZATION_CLONE_BYTES).map(|_| ())
}

pub(super) struct AuthoringProjection<'a> {
    pub path: &'a Path,
    pub bytes: &'a [u8],
    pub create: bool,
}

pub(super) fn projected_authoring(
    project: &Project,
    updates: &[AuthoringProjection<'_>],
) -> Result<(), String> {
    projected_envelope_with_limit(project, updates, MAX_LOCALIZATION_CLONE_BYTES).map(|_| ())
}

pub(super) fn clone_envelope_with_limit(project: &Project, limit: usize) -> Result<usize, String> {
    projected_envelope_with_limit(project, &[], limit)
}

pub(super) fn projected_envelope_with_limit(
    project: &Project,
    updates: &[AuthoringProjection<'_>],
    limit: usize,
) -> Result<usize, String> {
    let additions = updates
        .iter()
        .filter(|update| !project.authoring_documents.contains_key(update.path))
        .count();
    budget(
        project
            .documents
            .len()
            .saturating_add(project.authoring_documents.len())
            .saturating_add(additions)
            <= MAX_LOCALIZATION_TRACKED_FILES,
        "完整工作区文档数",
    )?;
    let mut bytes = project
        .root
        .as_os_str()
        .as_encoded_bytes()
        .len()
        .saturating_add(project.entry.as_os_str().as_encoded_bytes().len());
    budget(bytes <= limit, "完整工作区 current/saved/path 字节")?;
    for (path, document) in &project.documents {
        bytes = bytes
            .saturating_add(path.as_os_str().as_encoded_bytes().len())
            .saturating_add(document.text.len())
            .saturating_add(document.saved_byte_len());
        budget(bytes <= limit, "完整工作区 current/saved/path 字节")?;
    }
    for (path, document) in &project.authoring_documents {
        let update = updates.iter().find(|update| update.path == path.as_path());
        let current = update.map_or(document.bytes().len(), |update| update.bytes.len());
        let saved = if update.is_some_and(|update| update.create) {
            0
        } else {
            document.saved.as_ref().map_or(0, Vec::len)
        };
        bytes = bytes
            .saturating_add(path.as_os_str().as_encoded_bytes().len())
            .saturating_add(current)
            .saturating_add(saved);
        budget(bytes <= limit, "完整工作区 current/saved/path 字节")?;
    }
    for update in updates
        .iter()
        .filter(|update| !project.authoring_documents.contains_key(update.path))
    {
        bytes = bytes
            .saturating_add(update.path.as_os_str().as_encoded_bytes().len())
            .saturating_add(update.bytes.len());
        budget(bytes <= limit, "完整工作区 current/saved/path 字节")?;
    }
    budget(
        project.authoring_diagnostics().len() <= MAX_DIAGNOSTICS,
        "工作区诊断数",
    )?;
    Ok(bytes)
}

impl Project {
    /// Cheap borrowed-size preflight before a native/worker host clones this Project.
    /// Does not compile, parse JSON, clone document contents, refresh or access storage.
    pub fn check_localization_budget(&self) -> Result<(), LocalizationError> {
        project(self).map_err(LocalizationError::from)
    }
}

pub(super) fn exchange(exchange: &LocalizationExchange) -> Result<(), String> {
    budget(
        exchange.entries.len() <= MAX_LOCALIZATION_BATCH,
        "导入条目数",
    )?;
    budget(
        exchange.string_ids.len() <= MAX_LOCALIZATION_BATCH,
        "导入 ID 数",
    )?;
    id(&exchange.source_locale)?;
    id(&exchange.target_locale)?;
    for value in &exchange.string_ids {
        id(value)?;
    }
    for entry in &exchange.entries {
        id(&entry.id)?;
        parts(&entry.source_parts)?;
        if let Some(value) = &entry.translation_parts {
            parts(value)?;
        }
    }
    serialized(exchange, MAX_LOCALIZATION_JSON_BYTES, "交换 JSON 字节")
}

pub(super) fn serialized<T: Serialize>(value: &T, max: usize, what: &str) -> Result<(), String> {
    serialized_len(value, max, what).map(|_| ())
}

pub(super) fn reserve<T: Serialize>(value: &T, used: &mut usize, what: &str) -> Result<(), String> {
    let remaining = MAX_SNAPSHOT_BYTES.saturating_sub(*used);
    let bytes = serialized_len(value, remaining, what)?;
    *used = used.saturating_add(bytes).saturating_add(1);
    budget(*used <= MAX_SNAPSHOT_BYTES, what)
}

pub(super) fn serialized_len<T: Serialize>(
    value: &T,
    max: usize,
    what: &str,
) -> Result<usize, String> {
    struct Counter {
        size: usize,
        max: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.size = self.size.saturating_add(bytes.len());
            if self.size > self.max {
                return Err(std::io::Error::other("预算已满"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { size: 0, max };
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| format!("BUDGET_EXCEEDED：本地化{what}超过预算"))?;
    Ok(counter.size)
}

pub(super) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    budget(bytes.len() <= MAX_LOCALIZATION_JSON_BYTES, "输入 JSON 字节")?;
    let value = crate::parse_unique_json(bytes).map_err(|e| format!("INVALID_JSON：{e}"))?;
    serde_json::from_value(value).map_err(|e| format!("INVALID_JSON：{e}"))
}

impl LocalizationExchange {
    /// New candidate workflow only. The legacy parser's compatibility is unchanged.
    pub fn from_json_bytes_limited(bytes: &[u8]) -> Result<Self, LocalizationError> {
        let value = decode(bytes).map_err(LocalizationError::from)?;
        exchange(&value).map_err(LocalizationError::from)?;
        Ok(value)
    }
}

impl LocalizationEditDraft {
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, LocalizationError> {
        decode(bytes).map_err(LocalizationError::from)
    }
}

impl LocalizationIdDraft {
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, LocalizationError> {
        decode(bytes).map_err(LocalizationError::from)
    }
}

impl LocalizationImportDraft {
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, LocalizationError> {
        let value: Self = decode(bytes).map_err(LocalizationError::from)?;
        exchange(&value.exchange).map_err(LocalizationError::from)?;
        Ok(value)
    }
}

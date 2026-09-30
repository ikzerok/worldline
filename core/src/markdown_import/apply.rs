use super::{
    CandidateBuild, MarkdownImportConflict, MarkdownImportFilePreview, MarkdownImportOptions,
    MarkdownImportPlan, MarkdownLinkMapping, Project, ResolvedAttachment, SourcePage,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) fn ensure_plan_applicable(
    plan: &MarkdownImportPlan,
    options: &MarkdownImportOptions,
) -> Result<(), String> {
    if !plan.conflicts.is_empty() {
        return Err("Markdown 迁移仍有未解决的冲突".into());
    }
    if !plan.losses.is_empty() && !options.accept_losses {
        return Err("Markdown 迁移缺少损失确认".into());
    }
    if plan.requires_language_upgrade && !options.allow_language_upgrade {
        return Err("Markdown 迁移缺少语言升级确认".into());
    }
    Ok(())
}

pub(super) fn insert_candidate_file(
    files: &mut crate::workspace_snapshot::Files,
    path: PathBuf,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let target = path
        .to_str()
        .ok_or_else(|| format!("Markdown 候选路径不是 UTF-8：{}", path.display()))?
        .replace('\\', "/")
        .to_lowercase();
    for existing in files.keys() {
        let existing = existing
            .to_str()
            .ok_or_else(|| format!("工程路径不是 UTF-8：{}", existing.display()))?
            .replace('\\', "/")
            .to_lowercase();
        if existing == target
            || existing.starts_with(&format!("{target}/"))
            || target.starts_with(&format!("{existing}/"))
        {
            return Err(format!(
                "Markdown 候选目标已存在，拒绝覆盖：{}",
                path.display()
            ));
        }
    }
    files.insert(path, bytes);
    Ok(())
}

pub(super) fn build_candidate(
    project: &Project,
    current: &crate::CompileResult,
    pages: &[SourcePage],
    links: &[MarkdownLinkMapping],
    attachments: &[ResolvedAttachment<'_>],
    namespace: &str,
    requires_language_upgrade: bool,
) -> Result<CandidateBuild, String> {
    let mut candidate = project.clone();
    let output_root = format!(".world/markdown-imports/{namespace}");
    let relative_source = PathBuf::from(&output_root).join("import.wl");
    let absolute_source = candidate.root.join(&relative_source);
    let import_source_root = format!("{output_root}/sources");
    let mut generated = String::new();

    for page in pages {
        let source_copy = format!("{import_source_root}/{}", page.relative);
        generated.push_str(&format!(
            "entity {} kind {} as {}\n  description {}\n  property markdown_source = {}\n\n",
            page.id,
            page.entity_type,
            crate::authoring::quote(&page.title),
            crate::authoring::quote(&page.description),
            crate::authoring::quote(&source_copy),
        ));
        for heading in &page.headings {
            generated.push_str(&format!(
                "anchor_def {} as {}\n  description {}\nanchor_link {} entity {}\n\n",
                heading.id,
                crate::authoring::quote(&heading.title),
                crate::authoring::quote(&format!("{}:{}", page.relative, heading.line)),
                heading.id,
                page.id,
            ));
        }
    }

    let relation_type = format!("markdown_link_{namespace}");
    if !links.is_empty() {
        generated.push_str(&format!(
            "relation_type {relation_type} as {}\n  direction directed\n\n",
            crate::authoring::quote("Markdown 链接"),
        ));
        for link in links {
            let from = pages
                .iter()
                .find(|page| page.relative == link.source)
                .map(|page| page.id.as_str())
                .ok_or("Markdown 关系来源页面不存在")?;
            generated.push_str(&format!(
                "relation_def {} type {} from entity {} to {} {}\n  description {}\n  source_note {}\n\n",
                link.relation_id,
                relation_type,
                from,
                link.target.kind,
                link.target.id,
                crate::authoring::quote(&link.label),
                crate::authoring::quote(&format!("{}:{}", link.source, link.line)),
            ));
        }
    }

    let mut additional_by_path = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut seen_assets = BTreeSet::new();
    let mut seen_asset_links = BTreeSet::new();
    for attachment in attachments {
        match additional_by_path.get(&PathBuf::from(&attachment.output_path)) {
            Some(existing) if existing != attachment.bytes.as_ref() => {
                return Err(format!(
                    "相同附件目标对应不同来源字节：{}",
                    attachment.output_path
                ));
            }
            Some(_) => {}
            None => {
                additional_by_path.insert(
                    PathBuf::from(&attachment.output_path),
                    attachment.bytes.as_ref().clone(),
                );
            }
        }
        if !attachment.quarantined {
            let page_id = pages
                .iter()
                .find(|page| page.relative == attachment.source_page)
                .map(|page| page.id.as_str())
                .ok_or("Markdown 附件来源页面不存在")?;
            let relative_asset = attachment
                .output_path
                .strip_prefix(&format!("{output_root}/"))
                .ok_or("Markdown 附件目标路径不在导入目录内")?;
            let extension = safe_extension(&attachment.input.relative);
            if seen_assets.insert(attachment.id.clone()) {
                generated.push_str(&format!(
                    "asset {} {} {} as {}\n",
                    attachment.id,
                    asset_kind(&extension),
                    crate::authoring::quote(relative_asset),
                    crate::authoring::quote(&attachment.reference.label),
                ));
            }
            if seen_asset_links.insert((page_id.to_string(), attachment.id.clone())) {
                generated.push_str(&format!(
                    "attach entity {} with {}\n",
                    page_id, attachment.id
                ));
            }
            generated.push('\n');
        }
    }
    for page in pages {
        additional_by_path.insert(
            PathBuf::from(&import_source_root).join(&page.relative),
            page.bytes.clone(),
        );
    }

    if !candidate.documents.contains_key(&absolute_source) {
        candidate.add_file(&relative_source)?;
    }
    candidate.set_text(&absolute_source, generated.clone())?;
    let manifest_relative = PathBuf::from(".world/project.json");
    let manifest_was_present = candidate
        .authoring_documents
        .contains_key(&candidate.root.join(&manifest_relative));
    update_import_manifest(&mut candidate, &relative_source)?;

    // Compile the complete proposal once so cross-file IDs and references are
    // validated against the same candidate that would be saved.
    let candidate_result = candidate.compile_current();
    let baseline_errors = diagnostic_error_counts(current);
    let candidate_errors = diagnostic_error_counts(&candidate_result);
    let mut conflicts = Vec::new();
    for (error, count) in candidate_errors {
        let previous = baseline_errors.get(&error).copied().unwrap_or_default();
        for _ in previous..count {
            conflicts.push(MarkdownImportConflict {
                code: "CANDIDATE_COMPILE_ERROR".into(),
                source: None,
                preferred_id: None,
                candidates: Vec::new(),
                message: format!(
                    "候选源码产生新编译错误 {}：{}:{} {}",
                    error.0, error.1, error.2, error.3
                ),
            });
        }
    }

    let mut files = Vec::new();
    for (path, document) in &candidate.documents {
        if document.is_dirty() {
            let relative = path
                .strip_prefix(&candidate.root)
                .map_err(|_| format!("写入路径不在工程目录内：{}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(MarkdownImportFilePreview {
                path: relative,
                kind: if document.is_deleted() {
                    "project_source_delete".into()
                } else if path == &absolute_source {
                    "generated_source".into()
                } else {
                    "project_source_update".into()
                },
                source: None,
                bytes: if document.is_deleted() {
                    0
                } else {
                    document.text.len()
                },
            });
        }
    }
    for (path, document) in &candidate.authoring_documents {
        if document.is_dirty() {
            let relative = path
                .strip_prefix(&candidate.root)
                .map_err(|_| format!("写入路径不在工程目录内：{}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push(MarkdownImportFilePreview {
                path: relative,
                kind: if path == &candidate.root.join(&manifest_relative) {
                    if manifest_was_present && requires_language_upgrade {
                        "manifest_language_upgrade".into()
                    } else if manifest_was_present {
                        "manifest_preserving_update".into()
                    } else {
                        "manifest_create".into()
                    }
                } else if document.is_deleted() {
                    "authoring_document_delete".into()
                } else {
                    "authoring_document_update".into()
                },
                source: None,
                bytes: if document.is_deleted() {
                    0
                } else {
                    document.bytes.len()
                },
            });
        }
    }
    for page in pages {
        files.push(MarkdownImportFilePreview {
            path: format!("{import_source_root}/{}", page.relative),
            kind: "source_copy".into(),
            source: Some(page.relative.clone()),
            bytes: page.bytes.len(),
        });
    }
    for (relative, bytes) in &additional_by_path {
        let path = relative.to_string_lossy().replace('\\', "/");
        let attachment = attachments
            .iter()
            .find(|attachment| attachment.output_path == path);
        files.push(MarkdownImportFilePreview {
            path,
            kind: attachment.map_or_else(
                || "source_copy".into(),
                |attachment| {
                    if attachment.quarantined {
                        "quarantine_copy".into()
                    } else {
                        "attachment".into()
                    }
                },
            ),
            source: attachment
                .map(|attachment| attachment.input.relative.clone())
                .or_else(|| {
                    pages
                        .iter()
                        .find(|page| relative.ends_with(Path::new(&page.relative)))
                        .map(|page| page.relative.clone())
                }),
            bytes: bytes.len(),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files.dedup_by(|left, right| left.path == right.path);
    let additional_files = additional_by_path.into_iter().collect();
    Ok((candidate, additional_files, files, conflicts))
}

fn diagnostic_error_counts(
    result: &crate::CompileResult,
) -> BTreeMap<(String, String, u32, String), usize> {
    let mut counts = BTreeMap::new();
    for diagnostic in result
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == crate::Severity::Error)
    {
        *counts
            .entry((
                diagnostic.code.to_string(),
                diagnostic.file.clone(),
                diagnostic.span.line,
                diagnostic.message.clone(),
            ))
            .or_default() += 1;
    }
    counts
}

fn update_import_manifest(project: &mut Project, import_source: &Path) -> Result<(), String> {
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let relative_source = import_source.to_string_lossy().replace('\\', "/");
    let exists = project.authoring_documents.contains_key(&manifest_path);
    let mut value = if exists {
        let document = project.authoring_document(&manifest_path)?;
        crate::workspace_documents::parse_unique_json(&document.bytes)?
    } else {
        let entry = project
            .entry
            .strip_prefix(&project.root)
            .map_err(|_| "工程入口不在工作区内")?
            .to_string_lossy()
            .replace('\\', "/");
        serde_json::json!({
            "schema_version": 1,
            "language_version": "1.10",
            "entry": entry,
            "required_features": []
        })
    };
    let object = value
        .as_object_mut()
        .ok_or("工作区清单顶层必须是 JSON 对象")?;
    let mut changed = !exists;
    if !matches!(
        object
            .get("language_version")
            .and_then(serde_json::Value::as_str),
        Some("1.10" | "1.11")
    ) {
        object.insert(
            "language_version".into(),
            serde_json::Value::String("1.10".into()),
        );
        changed = true;
    }
    let mut needs_source_set_feature = false;
    for required in ["content.entities.v1", "content.relations.v1"] {
        let required_features = object
            .entry("required_features")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or("required_features 必须是数组")?;
        if !required_features
            .iter()
            .any(|feature| feature.as_str() == Some(required))
        {
            required_features.push(serde_json::Value::String(required.into()));
            changed = true;
        }
    }
    if let Some(source_config) = object.get_mut("source_config") {
        let source_config = source_config
            .as_object_mut()
            .ok_or("source_config 必须是对象")?;
        let active = source_config
            .get_mut("active")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("source_config.active 必须是数组")?;
        if !active
            .iter()
            .any(|path| path.as_str() == Some(&relative_source))
        {
            active.push(serde_json::Value::String(relative_source));
            changed = true;
        }
        active.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
        needs_source_set_feature = true;
    }
    if needs_source_set_feature {
        let required_features = object
            .get_mut("required_features")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("required_features 必须是数组")?;
        if !required_features
            .iter()
            .any(|feature| feature.as_str() == Some("workspace.source_sets.v1"))
        {
            required_features.push(serde_json::Value::String("workspace.source_sets.v1".into()));
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    if exists {
        project.set_authoring_document(&manifest_path, bytes)?;
    } else {
        project.create_authoring_document(&manifest_path, bytes)?;
    }
    Ok(())
}

fn asset_kind(extension: &str) -> &'static str {
    match extension {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => "image",
        "wav" | "mp3" | "ogg" | "flac" | "m4a" | "aac" => "audio",
        _ => "file",
    }
}

pub(super) fn safe_extension(relative: &str) -> String {
    let extension = Path::new(relative)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.is_empty()
        && extension.len() <= 16
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        extension.to_ascii_lowercase()
    } else {
        "bin".into()
    }
}

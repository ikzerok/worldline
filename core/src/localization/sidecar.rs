use super::*;
use crate::project::Project;
use std::path::Path;

pub(super) struct PreparedSidecar {
    pub manifest_bytes: Option<Vec<u8>>,
    pub bytes: Vec<u8>,
    pub create: bool,
}

pub(super) fn prepare_sidecar(
    project: &Project,
    path: &Path,
    registered: bool,
    exchange: &LocalizationExchange,
    memory_candidate: bool,
) -> Result<PreparedSidecar, String> {
    let manifest_path = crate::workspace_documents::manifest_path(&project.root);
    let manifest_bytes = if registered {
        None
    } else {
        Some(register_localization_path(
            project,
            &manifest_path,
            path,
            &exchange.target_locale,
        )?)
    };

    let current = project
        .authoring_documents
        .get(path)
        .filter(|document| !document.is_deleted());
    if current.is_some_and(|document| document.is_read_only()) {
        return Err("目标 locale sidecar 是只读文档".into());
    }
    let create_sidecar = current.is_none();
    let mut sidecar = if let Some(document) = current {
        crate::workspace_documents::parse_unique_json(document.bytes())
            .map_err(|error| format!("locale sidecar JSON 无法解析：{error}"))?
    } else {
        let files = match crate::file_access::workspace_files(&project.root) {
            Ok(files) => files,
            Err(error) if memory_candidate && error.kind() == std::io::ErrorKind::NotFound => {
                Vec::new()
            }
            Err(error) => return Err(format!("无法读取工作区文件清单：{error}")),
        };
        if files.iter().any(|file| file.as_path() == path) {
            return Err("目标 locale sidecar 已存在但未注册，拒绝覆盖".into());
        }
        serde_json::json!({
            "schema_version": 1,
            "required_features": [LOCALIZATION_REQUIRED_FEATURE],
            "source_locale": exchange.source_locale,
            "target_locale": exchange.target_locale,
            "entries": {}
        })
    };
    super::document::validate_header(
        &sidecar,
        &exchange.target_locale,
        Some(&exchange.source_locale),
    )?;
    let object = sidecar
        .as_object_mut()
        .ok_or("locale sidecar 顶层必须是 JSON 对象")?;
    let entries = object
        .get_mut("entries")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("locale sidecar entries 必须是对象")?;
    for entry in &exchange.entries {
        let value = entries
            .entry(entry.id.clone())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let fields = value
            .as_object_mut()
            .ok_or_else(|| format!("sidecar 条目 `{}` 不是对象", entry.id))?;
        fields.insert(
            "source_revision".into(),
            serde_json::Value::String(entry.source_revision.clone()),
        );
        fields.insert(
            "translation_parts".into(),
            serde_json::to_value(&entry.translation_parts)
                .map_err(|error| format!("无法序列化译文：{error}"))?,
        );
    }
    let bytes = serde_json::to_vec(&sidecar)
        .map_err(|error| format!("无法序列化 locale sidecar：{error}"))?;
    Ok(PreparedSidecar {
        manifest_bytes,
        bytes,
        create: create_sidecar,
    })
}

fn register_localization_path(
    project: &Project,
    manifest_path: &Path,
    sidecar_path: &Path,
    locale: &str,
) -> Result<Vec<u8>, String> {
    let document = project
        .authoring_documents
        .get(manifest_path)
        .filter(|document| !document.is_deleted())
        .ok_or("本地化需要已载入的工程清单")?;
    let mut manifest = crate::workspace_documents::parse_unique_json(document.bytes())
        .map_err(|error| format!("工程清单 JSON 无法解析：{error}"))?;
    let object = manifest
        .as_object_mut()
        .ok_or("工程清单顶层必须是 JSON 对象")?;
    let relative = sidecar_path
        .strip_prefix(&project.root)
        .map_err(|_| "locale sidecar 路径越出工作区")?
        .to_str()
        .ok_or("locale sidecar 路径不是 UTF-8")?
        .replace('\\', "/");
    let localizations = object
        .entry("localizations")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .ok_or("工程清单 localizations 必须是对象")?;
    if let Some(existing) = localizations
        .get(locale)
        .and_then(serde_json::Value::as_str)
    {
        if existing.replace('\\', "/") != relative {
            return Err("目标 locale 已登记到其他 sidecar 路径".into());
        }
    } else {
        localizations.insert(locale.into(), serde_json::Value::String(relative));
    }
    serde_json::to_vec(&manifest).map_err(|error| format!("无法序列化工程清单：{error}"))
}

//! locale sidecar 的共享只读结构校验，不选择字符串或执行导入。
use super::{LocalizationPart, LOCALIZATION_REQUIRED_FEATURE};
use crate::project::Project;
use serde_json::Value;
use std::path::Path;

pub(super) fn validate_header(
    value: &Value,
    target_locale: &str,
    source_locale: Option<&str>,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or("locale sidecar 顶层必须是 JSON 对象")?;
    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("locale sidecar schema_version 不受支持".into());
    }
    let features = object
        .get("required_features")
        .and_then(Value::as_array)
        .ok_or("locale sidecar 缺少 required_features 数组")?;
    if !features
        .iter()
        .any(|feature| feature.as_str() == Some(LOCALIZATION_REQUIRED_FEATURE))
    {
        return Err("locale sidecar 缺少 content.localization.v1".into());
    }
    if features
        .iter()
        .any(|feature| feature.as_str() != Some(LOCALIZATION_REQUIRED_FEATURE))
    {
        return Err("locale sidecar 含未知必需能力，只读保留原文".into());
    }
    let source = object
        .get("source_locale")
        .and_then(Value::as_str)
        .ok_or("locale sidecar 缺少 source_locale")?;
    if source.trim().is_empty()
        || source_locale.is_some_and(|expected| source != expected)
        || object.get("target_locale").and_then(Value::as_str) != Some(target_locale)
    {
        return Err("locale sidecar 的 source/target locale 不匹配".into());
    }
    if object.get("entries").and_then(Value::as_object).is_none() {
        return Err("locale sidecar entries 必须是对象".into());
    }
    Ok(())
}

impl Project {
    /// 只读取一个已注册 locale；一份坏文档不影响其他注册文档。
    pub fn read_localization_document(&self, locale: &str, path: &Path) -> Result<Value, String> {
        let manifest = crate::workspace_documents::manifest_path(&self.root);
        let registry = self.authoring_document(&manifest)?;
        if registry.is_deleted() {
            return Err("本地化清单已删除".into());
        }
        let registered = crate::workspace_documents::parse_registry(&self.root, registry.bytes());
        if registered.localizations.get(locale).map(Path::new) != Some(path) {
            return Err("本地化文档不属于此注册 locale".into());
        }
        let document = self.authoring_document(path)?;
        if document.is_deleted() {
            return Err("注册的本地化文档已删除或不存在".into());
        }
        let value = crate::parse_unique_json(document.bytes())
            .map_err(|e| format!("locale sidecar JSON 无法解析：{e}"))?;
        validate_header(&value, locale, None)?;
        let entries = value["entries"]
            .as_object()
            .ok_or("locale sidecar entries 必须是对象")?;
        for (id, entry) in entries {
            let fields = entry
                .as_object()
                .ok_or_else(|| format!("sidecar 条目 `{id}` 不是对象"))?;
            if id.trim().is_empty()
                || fields
                    .get("source_revision")
                    .and_then(Value::as_str)
                    .is_none()
            {
                return Err(format!("sidecar 条目 `{id}` 缺少 source_revision"));
            }
            let parts = fields
                .get("translation_parts")
                .ok_or_else(|| format!("sidecar 条目 `{id}` 缺少 translation_parts"))?;
            // None means untranslated; missing translations need an explicit operation selection.
            serde_json::from_value::<Option<Vec<LocalizationPart>>>(parts.clone())
                .map_err(|e| format!("sidecar 条目 `{id}` 的译文结构无效：{e}"))?;
        }
        Ok(value)
    }
}

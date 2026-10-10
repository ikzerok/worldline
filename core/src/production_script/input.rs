use super::*;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTarget {
    kind: String,
    id: String,
}
pub(super) fn target<'de, D: Deserializer<'de>>(d: D) -> Result<TargetRef, D::Error> {
    let value = StrictTarget::deserialize(d)?;
    Ok(TargetRef::new(&value.kind, &value.id))
}
pub(super) fn optional_target<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<TargetRef>, D::Error> {
    Ok(Option::<StrictTarget>::deserialize(d)?.map(|value| TargetRef::new(&value.kind, &value.id)))
}
impl ProductionScriptRequest {
    pub fn validate(&self) -> Result<(), ProductionError> {
        if self.schema_version != 1 {
            return Err(ProductionError::new(
                "UNSUPPORTED_VERSION",
                "不支持的制作台本请求版本",
            ));
        }
        let max = ProductionLimits::default();
        let limits = &self.limits;
        if [
            (limits.chapters, max.chapters),
            (limits.definitions, max.definitions),
            (limits.call_sites, max.call_sites),
            (limits.rows, max.rows),
            (limits.source_files, max.source_files),
            (limits.source_bytes, max.source_bytes),
            (limits.result_bytes, max.result_bytes),
            (limits.export_bytes, max.export_bytes),
        ]
        .iter()
        .any(|(value, cap)| *value == 0 || value > cap)
        {
            return Err(ProductionError::new(
                "INVALID_LIMIT",
                "预算必须为正且不得超过制作台本硬上限；未钳制",
            ));
        }
        if self.search.len() > 4096
            || self
                .expected_snapshot_key
                .as_ref()
                .is_some_and(|k| k.len() > 128)
        {
            return Err(ProductionError::budget());
        }
        if self.statuses.len() > 5
            || self.statuses.iter().collect::<BTreeSet<_>>().len() != self.statuses.len()
        {
            return Err(ProductionError::new(
                "INVALID_QUERY",
                "台词状态过滤不得重复或超过五类",
            ));
        }
        if let Some(target) = &self.speaker {
            valid_target(target)?;
            if target.kind != "character" {
                return Err(ProductionError::new(
                    "INVALID_SPEAKER",
                    "说话者必须为正式 character 身份",
                ));
            }
        }
        if let Some(locale) = &self.target_locale {
            if locale.len() > 128 || !crate::workspace_documents::valid_id(locale) {
                return Err(ProductionError::new(
                    "INVALID_QUERY",
                    "目标 locale 必须是有效标识符",
                ));
            }
        }
        match &self.scope {
            ProductionScope::CurrentTarget { target } => valid_target(target)?,
            ProductionScope::Manuscript {
                query,
                chapter_ids,
                expected_query_key,
            } => {
                query
                    .validate()
                    .map_err(|e| ProductionError::new(e.code, e.message))?;
                if chapter_ids.as_ref().is_some_and(|ids| {
                    ids.len() > limits.chapters
                        || ids.iter().any(|id| id.is_empty() || id.len() > 256)
                }) || expected_query_key
                    .as_ref()
                    .is_some_and(|key| key.len() > 128)
                {
                    return Err(ProductionError::budget());
                }
            }
            ProductionScope::Project => {}
        }
        bounded_size(self, 4 * 1024 * 1024)?;
        Ok(())
    }
}
fn valid_target(target: &TargetRef) -> Result<(), ProductionError> {
    if target.id.is_empty() || target.id.len() > 256 || target.kind.len() > 32 {
        return Err(ProductionError::new(
            "INVALID_QUERY",
            "对象身份为空或超过预算",
        ));
    }
    Ok(())
}
pub fn parse_production_script_request(raw: &str) -> Result<ProductionScriptRequest, String> {
    let request: ProductionScriptRequest = parse(raw)?;
    request.validate().map_err(|e| e.to_string())?;
    Ok(request)
}
pub fn parse_production_export_options(raw: &str) -> Result<ProductionExportOptions, String> {
    let value: ProductionExportOptions = parse(raw)?;
    value.validate().map_err(|e| e.to_string())?;
    Ok(value)
}
impl ProductionExportOptions {
    pub fn validate(&self) -> Result<(), ProductionError> {
        if self.schema_version != 1 {
            return Err(ProductionError::new(
                "UNSUPPORTED_VERSION",
                "不支持的制作台本导出版本",
            ));
        }
        Ok(())
    }
}
fn parse<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, String> {
    if raw.len() > 4 * 1024 * 1024 {
        return Err("制作台本请求超过4MiB预算".into());
    }
    serde_json::from_value(crate::parse_unique_json(raw.as_bytes())?).map_err(|e| e.to_string())
}

use super::*;
use crate::{project::Project, CompileResult, TargetRef};
use serde::{Deserialize, Deserializer, Serialize};
use std::io::{self, Write};

pub(super) fn target<'de, D: Deserializer<'de>>(d: D) -> Result<TargetRef, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StrictTarget {
        kind: String,
        id: String,
    }
    let value = StrictTarget::deserialize(d)?;
    Ok(TargetRef::new(&value.kind, &value.id))
}
pub fn parse_condition_selector(text: &str) -> Result<ConditionSelector, String> {
    let value = parse(text)?;
    validate_selector(&value)?;
    Ok(value)
}
pub fn parse_condition_authoring_request(text: &str) -> Result<ConditionAuthoringRequest, String> {
    let value = parse(text)?;
    validate_request(&value)?;
    Ok(value)
}
fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    if text.len() > MAX_CONDITION_REQUEST_BYTES {
        return Err("条件作者请求超过 256 KiB 预算".into());
    }
    serde_json::from_value(crate::parse_unique_json(text.as_bytes())?)
        .map_err(|e| format!("条件作者协议格式错误：{e}"))
}
pub(super) fn validate_selector(value: &ConditionSelector) -> Result<(), String> {
    version(value.schema_version)?;
    if value.owner.target.kind != "event" {
        return Err("条件作者 owner 必须为完整 event 身份".into());
    }
    if value.owner.target.id.len() > 512 {
        return Err("条件事件身份超过 512 字节预算".into());
    }
    for part in value.owner.target.id.split('.') {
        identifier(part)?;
    }
    match (value.slot, value.owner.choice_line) {
        (ConditionSlot::EventAfter, None) => Ok(()),
        (ConditionSlot::ChoiceDisplay | ConditionSlot::ChoiceEnable, Some(line)) if line > 0 => {
            Ok(())
        }
        _ => Err("条件槽与 choice_line 不匹配；不能猜测目标".into()),
    }
}
pub(super) fn validate_request(value: &ConditionAuthoringRequest) -> Result<(), String> {
    version(value.schema_version)?;
    validate_selector(&value.selector)?;
    if value.expected_baseline.is_empty() || value.expected_baseline.len() > 512 {
        return Err("条件作者请求缺少有效内容基线".into());
    }
    super::tree::validate_budget(&value.replacement)?;
    measure(value, MAX_CONDITION_REQUEST_BYTES)
}
pub(super) fn version(version: u32) -> Result<(), String> {
    if version != 1 {
        Err("不支持的条件作者 DTO 版本".into())
    } else {
        Ok(())
    }
}
pub(super) fn identifier(value: &str) -> Result<(), String> {
    if value.len() > 512 {
        return Err("条件身份超过 512 字节预算".into());
    }
    crate::authoring::identifier(value)
}
pub(super) fn check_workspace(project: &Project) -> Result<(), String> {
    if project
        .documents
        .len()
        .saturating_add(project.authoring_documents.len())
        > 4096
    {
        return Err("条件作者工作区超过 4096 文档预算".into());
    }
    let mut total = 0usize;
    for (path, doc) in &project.documents {
        if doc.is_deleted()
            || project
                .source_selection()
                .is_some_and(|s| !s.is_active(path))
        {
            continue;
        }
        if doc.text.len() > 4 * 1024 * 1024 {
            return Err("条件作者单源超过 4 MiB 预算".into());
        }
        total = total.saturating_add(doc.text.len());
        if total > 32 * 1024 * 1024 {
            return Err("条件作者活动源码超过 32 MiB 预算".into());
        }
    }
    Ok(())
}
pub(super) fn snapshot(project: &Project, compiled: &CompileResult) -> Result<(), String> {
    check_workspace(project)?;
    let sources = project.documents.iter().filter(|(path, doc)| {
        !doc.is_deleted() && project.source_selection().is_none_or(|s| s.is_active(path))
    });
    if project.compile_options() != compiled.options
        || sources.clone().count() != compiled.sources.len()
        || !sources
            .into_iter()
            .all(|(path, doc)| compiled.sources.get(path) == Some(&doc.text))
    {
        return Err("条件目录编译快照已过期；请保留输入并刷新".into());
    }
    Ok(())
}
pub(super) fn guard(project: &Project) -> Result<String, String> {
    project.ensure_workspace_writable()?;
    let inventory = crate::source_lifecycle::safety::inventory(project).map_err(|e| e.message)?;
    project
        .source_disk_baselines_match_inventory(&inventory)
        .map_err(|e| e.message)?;
    project
        .source_lifecycle_guard(&inventory)
        .map_err(|e| e.message)
}
pub(super) fn measure(value: &impl Serialize, limit: usize) -> Result<(), String> {
    struct Counter {
        used: usize,
        limit: usize,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.used) {
                return Err(io::Error::other("条件作者数据超出预算"));
            }
            self.used += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter { used: 0, limit }, value)
        .map_err(|e| format!("条件作者数据超过 {limit} 字节或无法序列化：{e}"))
}
pub(super) fn digest(value: &impl Serialize) -> Result<String, String> {
    measure(value, MAX_CONDITION_OUTPUT_BYTES)?;
    Ok(crate::presentation_commands::document_hash(
        &serde_json::to_vec(value).map_err(|e| e.to_string())?,
    ))
}

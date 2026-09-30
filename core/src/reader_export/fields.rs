use super::site::{html_escape, relative_url};
use super::*;
use crate::ast::{Property, PropertyValue};

fn properties<'a>(compiled: &'a CompileResult, target: &TargetRef) -> Option<&'a [Property]> {
    match target.kind.as_str() {
        "character" => compiled
            .program
            .characters
            .iter()
            .find(|v| v.name == target.id)
            .map(|v| v.properties.as_slice()),
        "entity" => compiled
            .program
            .entities
            .iter()
            .find(|v| v.name == target.id)
            .map(|v| v.properties.as_slice()),
        "world" => compiled
            .program
            .worlds
            .iter()
            .find(|v| v.name == target.id)
            .map(|v| v.properties.as_slice()),
        "relation" => compiled
            .program
            .relations
            .iter()
            .find(|v| v.id == target.id)
            .map(|v| v.properties.as_slice()),
        _ => None,
    }
}

/// 从同一 AST 列举作者候选；引用值只显示类型提示，避免候选误当公开值。
pub fn candidates(compiled: &CompileResult, target: &TargetRef) -> Vec<ReaderFieldCandidate> {
    properties(compiled, target)
        .unwrap_or_default()
        .iter()
        .map(|property| ReaderFieldCandidate {
            key: property.name.clone(),
            preview: match &property.value {
                PropertyValue::Str(value) if value.is_empty() => "（空字符串）".into(),
                PropertyValue::Str(value) => value.clone(),
                PropertyValue::Num(value) => value.to_string(),
                PropertyValue::Bool(value) => value.to_string(),
                PropertyValue::Ref(_) => "对象引用（仅当目标明确公开才显示）".into(),
            },
        })
        .collect()
}

impl CompileResult {
    pub fn reader_field_candidates(&self, target: &TargetRef) -> Vec<ReaderFieldCandidate> {
        candidates(self, target)
    }
}

pub(super) fn validate_version(selection: &ReaderExportSelection) -> Result<(), String> {
    if selection.schema_version == READER_EXPORT_SCHEMA_VERSION {
        if !selection.fields.is_empty() || !selection.required_features.is_empty() {
            return Err("字段公开需要选择 DTO v2 与 reader.fields.v1 能力".into());
        }
    } else if selection.required_features != [READER_FIELDS_FEATURE] {
        return Err("阅读包 v2 必须且只能声明 reader.fields.v1 能力".into());
    }
    Ok(())
}

pub(super) fn validate_fields(
    compiled: &CompileResult,
    selection: &ReaderExportSelection,
) -> Result<(), String> {
    let mut targets = BTreeSet::new();
    let mut total = 0usize;
    for field in &selection.fields {
        if !targets.insert(&field.target) || !selection.objects.contains(&field.target) {
            return Err("公开字段目标重复或未单独选择对象".into());
        }
        let properties = properties(compiled, &field.target).ok_or("此对象不支持字段公开")?;
        let mut keys = BTreeSet::new();
        total = total.saturating_add(field.keys.len());
        if field.keys.len() > 256 || total > 5000 {
            return Err("公开字段数量超过限制".into());
        }
        for key in &field.keys {
            if !keys.insert(key) || !properties.iter().any(|p| p.name == *key) {
                return Err(format!("公开字段不存在或重复：{key}"));
            }
        }
    }
    Ok(())
}

pub(super) fn append_fields(
    compiled: &CompileResult,
    target: &TargetRef,
    routes: &BTreeMap<TargetRef, String>,
    selection: &ReaderExportSelection,
    html: &mut String,
    plain: &mut String,
) -> Result<(), String> {
    let Some(selected) = selection.fields.iter().find(|f| f.target == *target) else {
        return Ok(());
    };
    let properties = properties(compiled, target).ok_or("此对象不支持字段公开")?;
    for key in &selected.keys {
        let property = properties
            .iter()
            .find(|p| p.name == *key)
            .ok_or("公开字段已失效")?;
        let (value_html, value_text) = match &property.value {
            PropertyValue::Str(value) if value.is_empty() => {
                ("（空字符串）".into(), "（空字符串）".into())
            }
            PropertyValue::Str(value) => (html_escape(value), value.clone()),
            PropertyValue::Num(value) if value.is_finite() => {
                (value.to_string(), value.to_string())
            }
            PropertyValue::Num(_) => return Err("公开数值字段必须有限".into()),
            PropertyValue::Bool(value) => (value.to_string(), value.to_string()),
            PropertyValue::Ref(reference) => {
                if let Some(route) = routes.get(reference) {
                    let object = compiled
                        .analysis
                        .catalog
                        .object(reference)
                        .ok_or("公开引用目标已失效")?;
                    (
                        format!(
                            "<a href=\"{}\">{}</a>",
                            html_escape(&relative_url(&routes[target], route)),
                            html_escape(&object.display)
                        ),
                        object.display.clone(),
                    )
                } else {
                    ("未公开内容".into(), "未公开内容".into())
                }
            }
        };
        html.push_str(&format!(
            "<dl><dt>{}</dt><dd style=\"white-space:pre-wrap\">{value_html}</dd></dl>",
            html_escape(key)
        ));
        plain.push_str(&format!("\n{key}：{value_text}\n"));
    }
    Ok(())
}

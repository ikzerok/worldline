use super::entities::property_value;
use super::relation_types::nullable_string;
use super::*;
pub(super) fn relation_id(
    params: &Value,
    operation: RelationOperation,
) -> Result<String, ProtoError> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| {
            params
                .get("relation")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| ProtoError::new(-32602, "关系参数需要字符串 `id`"))?;
    if id.is_empty() {
        return Err(ProtoError::new(-32602, "关系 `id` 不能为空"));
    }
    if operation != RelationOperation::Delete
        && params.get("relation").and_then(Value::as_object).is_none()
    {
        return Err(ProtoError::new(-32602, "需要对象参数 `relation`"));
    }
    Ok(id.to_string())
}

pub(super) fn relation_target_value(value: &Value, key: &str) -> Result<TargetRef, ProtoError> {
    let (kind, id) = if let Some(text) = value.as_str() {
        text.split_once(':')
            .ok_or_else(|| ProtoError::new(-32602, format!("`{key}` 字符串格式必须为 KIND:ID")))?
    } else {
        let object = value.as_object().ok_or_else(|| {
            ProtoError::new(-32602, format!("`{key}` 必须是 KIND:ID 字符串或对象"))
        })?;
        (
            object
                .get("kind")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtoError::new(-32602, format!("`{key}.kind` 必须是字符串")))?,
            object
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| ProtoError::new(-32602, format!("`{key}.id` 必须是字符串")))?,
        )
    };
    let kind = kind.trim();
    let id = id.trim();
    if kind.is_empty() || id.is_empty() || (kind != "file" && id.contains(':')) {
        return Err(ProtoError::new(
            -32602,
            format!("`{key}` 必须包含非空 KIND 和 ID"),
        ));
    }
    Ok(TargetRef::new(kind, id))
}

pub(super) fn relation_draft(
    params: &Value,
    existing: Option<&worldline_core::SemanticRelationInfo>,
) -> Result<RelationDraft, ProtoError> {
    let object = params
        .get("relation")
        .and_then(Value::as_object)
        .ok_or_else(|| ProtoError::new(-32602, "需要对象参数 `relation`"))?;
    relation_draft_object(object, existing, "relation")
}

pub(super) fn relation_draft_object(
    object: &serde_json::Map<String, Value>,
    existing: Option<&worldline_core::SemanticRelationInfo>,
    label: &str,
) -> Result<RelationDraft, ProtoError> {
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.id.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系参数需要字符串 `id`"))?;
    let relation_type = object
        .get("relation_type")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.relation_type.as_str()))
        .ok_or_else(|| ProtoError::new(-32602, "关系参数需要字符串 `relation_type`"))?;
    let from = if let Some(value) = object.get("from") {
        relation_target_value(value, &format!("{label}.from"))?
    } else {
        existing
            .map(|value| value.from_ref.clone())
            .ok_or_else(|| ProtoError::new(-32602, "创建关系需要 `from`"))?
    };
    let to = if let Some(value) = object.get("to") {
        relation_target_value(value, &format!("{label}.to"))?
    } else {
        existing
            .map(|value| value.to_ref.clone())
            .ok_or_else(|| ProtoError::new(-32602, "创建关系需要 `to`"))?
    };
    let description = object
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| existing.map(|value| value.description.as_str()))
        .unwrap_or_default();
    let source_note = if object.contains_key("source_note") {
        nullable_string(object.get("source_note"), "source_note")?
    } else {
        existing.and_then(|value| value.source_note.clone())
    };
    let scope_refs = if let Some(value) = object.get("scope_refs") {
        let values = value
            .as_array()
            .ok_or_else(|| ProtoError::new(-32602, format!("`{label}.scope_refs` 必须是数组")))?;
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                relation_target_value(value, &format!("{label}.scope_refs[{index}]"))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        existing.map_or_else(Vec::new, |value| value.scope_refs.clone())
    };
    let properties = if let Some(value) = object.get("properties") {
        relation_properties_value(value, label)?
    } else {
        existing.map_or_else(Vec::new, |value| {
            value
                .properties
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
    };
    Ok(RelationDraft {
        id: id.to_string(),
        relation_type: relation_type.to_string(),
        from,
        to,
        description: description.to_string(),
        source_note,
        scope_refs,
        properties,
    })
}

fn relation_properties_value(
    value: &Value,
    label: &str,
) -> Result<Vec<(String, PropertyValue)>, ProtoError> {
    if let Some(values) = value.as_object() {
        return values
            .iter()
            .map(|(key, value)| Ok((key.clone(), property_value(value)?)))
            .collect::<Result<Vec<_>, ProtoError>>();
    }
    let Some(values) = value.as_array() else {
        return Err(ProtoError::new(
            -32602,
            format!("`{label}.properties` 必须是对象或键值数组"),
        ));
    };
    values
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let pair = item.as_array().ok_or_else(|| {
                ProtoError::new(
                    -32602,
                    format!("`{label}.properties[{index}]` 必须是 [name, value]"),
                )
            })?;
            if pair.len() != 2 {
                return Err(ProtoError::new(
                    -32602,
                    format!("`{label}.properties[{index}]` 必须是 [name, value]"),
                ));
            }
            let name = pair[0].as_str().ok_or_else(|| {
                ProtoError::new(
                    -32602,
                    format!("`{label}.properties[{index}][0]` 必须是字符串"),
                )
            })?;
            Ok((name.to_string(), property_value(&pair[1])?))
        })
        .collect()
}

use super::model::*;
use crate::{ast::PropertyValue, catalog::TargetRef};
use serde::{Deserialize, Deserializer, Serialize};
use std::io::{self, Write};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTarget {
    kind: String,
    id: String,
}
impl From<StrictTarget> for TargetRef {
    fn from(value: StrictTarget) -> Self {
        Self {
            kind: value.kind,
            id: value.id,
        }
    }
}
pub(super) fn deserialize_binding<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<SchemaBindingMutation, D::Error> {
    // serde 内部标签的 unit variant 会忽略多余键，空 struct variant 才严格拒绝。
    #[derive(Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
    enum Binding {
        Keep {},
        Bind { schema_id: String },
        Change { schema_id: String },
        Unbind {},
    }
    Ok(match Binding::deserialize(d)? {
        Binding::Keep {} => SchemaBindingMutation::Keep,
        Binding::Bind { schema_id } => SchemaBindingMutation::Bind { schema_id },
        Binding::Change { schema_id } => SchemaBindingMutation::Change { schema_id },
        Binding::Unbind {} => SchemaBindingMutation::Unbind,
    })
}

pub(super) fn deserialize_target<'de, D: Deserializer<'de>>(d: D) -> Result<TargetRef, D::Error> {
    StrictTarget::deserialize(d).map(Into::into)
}
pub(super) fn deserialize_value<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<PropertyValue, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Value {
        Text(String),
        Number(f64),
        Boolean(bool),
        Ref(StrictTarget),
    }
    Ok(match Value::deserialize(d)? {
        Value::Text(value) => PropertyValue::Str(value),
        Value::Number(value) => PropertyValue::Num(value),
        Value::Boolean(value) => PropertyValue::Bool(value),
        Value::Ref(value) => PropertyValue::Ref(value.into()),
    })
}

pub fn parse_schema_authoring_target(text: &str) -> Result<TargetRef, String> {
    if text.len() > MAX_SCHEMA_AUTHORING_REQUEST_BYTES {
        return Err("schema 作者请求超过 256 KiB 预算".into());
    }
    let value = crate::parse_unique_json(text.as_bytes())?;
    let target: TargetRef = serde_json::from_value::<StrictTarget>(value)
        .map_err(|e| format!("无效的完整对象身份：{e}"))?
        .into();
    validate_target(&target)?;
    Ok(target)
}

pub fn parse_schema_authoring_request(text: &str) -> Result<SchemaAuthoringRequest, String> {
    if text.len() > MAX_SCHEMA_AUTHORING_REQUEST_BYTES {
        return Err("schema 作者请求超过 256 KiB 预算".into());
    }
    let value = crate::parse_unique_json(text.as_bytes())?;
    let request =
        serde_json::from_value(value).map_err(|e| format!("无效的 schema 作者请求：{e}"))?;
    validate_request(&request)?;
    Ok(request)
}

pub(super) fn validate_target(target: &TargetRef) -> Result<(), String> {
    if !super::super::parse::INSTANCE_KINDS.contains(&target.kind.as_str()) {
        return Err("schema 作者目标仅支持 world/character/entity/relation".into());
    }
    identifier(&target.id)
}
fn identifier(value: &str) -> Result<(), String> {
    if value.len() > MAX_SCHEMA_AUTHORING_ID_BYTES {
        return Err("schema 作者身份或字段键超过 512 字节预算".into());
    }
    crate::authoring::identifier(value)
}

pub(super) fn validate_request(request: &SchemaAuthoringRequest) -> Result<(), String> {
    if request.schema_version != 1 {
        return Err("不支持的 schema 作者 DTO 版本".into());
    }
    validate_target(&request.target)?;
    if request.expected_baseline.len() > MAX_SCHEMA_AUTHORING_ID_BYTES {
        return Err("schema 作者基线超过 512 字节预算".into());
    }
    if request.fields.len() > MAX_SCHEMA_AUTHORING_FIELDS {
        return Err("schema 字段操作超过 256 项预算".into());
    }
    match &request.binding {
        SchemaBindingMutation::Bind { schema_id } | SchemaBindingMutation::Change { schema_id } => {
            identifier(schema_id)?
        }
        _ => {}
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut keys = std::collections::BTreeSet::new();
    for mutation in &request.fields {
        let (field_id, key) = match mutation {
            SchemaFieldMutation::Set {
                field_id,
                key,
                value,
            } => {
                if matches!(value, PropertyValue::Num(n) if !n.is_finite()) {
                    return Err("schema 数字必须为有限数值".into());
                }
                if matches!(value, PropertyValue::Str(s) if s.contains('\r')) {
                    return Err("属性字符串不能无损表达回车字符；请保留原稿或改用换行".into());
                }
                if let PropertyValue::Ref(target) = value {
                    identifier(&target.kind)?;
                    identifier(&target.id)?;
                }
                measure(value, MAX_SCHEMA_AUTHORING_VALUE_BYTES)?;
                (field_id, key)
            }
            SchemaFieldMutation::Remove { field_id, key } => (field_id, key),
        };
        identifier(field_id)?;
        identifier(key)?;
        if !ids.insert(field_id) || !keys.insert(key) {
            return Err("同一 schema 字段或属性键不能重复操作".into());
        }
    }
    measure(request, MAX_SCHEMA_AUTHORING_REQUEST_BYTES)
}

pub(super) fn measure(value: &impl Serialize, limit: usize) -> Result<(), String> {
    encoded_size(value, limit).map(|_| ())
}

/// 在复制任何可变长投影内容前计量借用值，预留容器/字段名开销。
pub(super) struct Budget {
    remaining: usize,
}
impl Budget {
    pub fn new() -> Self {
        Self {
            remaining: MAX_SCHEMA_AUTHORING_OUTPUT_BYTES - 4096,
        }
    }
    pub fn charge(&mut self, value: &impl Serialize) -> Result<(), String> {
        let size = encoded_size(value, self.remaining)?;
        self.remaining = self
            .remaining
            .checked_sub(size.saturating_add(64))
            .ok_or("schema 作者投影构建超过 8 MiB 保守预算")?;
        Ok(())
    }
}

fn encoded_size(value: &impl Serialize, limit: usize) -> Result<usize, String> {
    struct Counter {
        used: usize,
        limit: usize,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.used) {
                return Err(io::Error::other("schema 作者数据超出序列化预算"));
            }
            self.used += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { used: 0, limit };
    serde_json::to_writer(&mut counter, value)
        .map_err(|e| format!("schema 作者数据超过 {limit} 字节预算或无法序列化：{e}"))?;
    Ok(counter.used)
}

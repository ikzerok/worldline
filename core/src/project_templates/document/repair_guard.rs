//! 救援资格检查保留所有重复成员，仅检查保护边界，不产生模板或转换草稿。
use super::*;
use serde::de::{MapAccess, SeqAccess, Visitor};

/// 不接受无法完整读取的旧文；每一处保护元数据均须可确认为受支持。
pub(super) fn validate(
    bytes: &[u8],
    features: &BTreeSet<String>,
    language: crate::LanguageVersion,
) -> Result<(), String> {
    let root: ProtectedJson = serde_json::from_slice(bytes).map_err(|_| refusal())?;
    let ProtectedJson::Object(members) = &root else {
        return Err(refusal());
    };
    let mut schema_found = false;
    let mut feature_sets = Vec::new();
    for (key, value) in members {
        match key.as_str() {
            "schema_version" => {
                schema_found = true;
                if !matches!(value, ProtectedJson::Unsigned(1) | ProtectedJson::Signed(1)) {
                    return Err(refusal());
                }
            }
            "required_features" => {
                let ProtectedJson::Array(values) = value else {
                    return Err(refusal());
                };
                let mut declared = BTreeSet::new();
                for value in values {
                    let ProtectedJson::String(feature) = value else {
                        return Err(refusal());
                    };
                    if !supported_template_feature(feature) {
                        return Err(refusal());
                    }
                    declared.insert(feature.as_str());
                }
                feature_sets.push(declared);
            }
            _ => {}
        }
    }
    if !schema_found {
        return Err(refusal());
    }
    let mut references = ReferenceCapabilities::default();
    inspect_fields(&root, &mut references);
    if references.object_ref
        && (!language.supports_entities() || !features.contains(OBJECT_REFS_REQUIRED_FEATURE))
    {
        return Err("旧文包含受语言或能力保护的对象引用，不能通过完整替换绕过只读保护".into());
    }
    if references.character_ref
        && (!language.supports_language_113()
            || !features.contains(CHARACTER_REFS_REQUIRED_FEATURE)
            || feature_sets.is_empty()
            || feature_sets
                .iter()
                .any(|set| !set.contains(CHARACTER_REFS_REQUIRED_FEATURE)))
    {
        return Err("旧文包含受语言或双能力保护的人物引用，不能通过完整替换绕过只读保护".into());
    }
    Ok(())
}

fn refusal() -> String {
    "旧原文保护元数据无法完整确认为受支持，不能原地修复；请先复制原文，再显式按新身份导入".into()
}

#[derive(Default)]
struct ReferenceCapabilities {
    object_ref: bool,
    character_ref: bool,
}

fn inspect_fields(value: &ProtectedJson, references: &mut ReferenceCapabilities) {
    let ProtectedJson::Object(members) = value else {
        return;
    };
    for (key, value) in members {
        if key != "fields" {
            continue;
        }
        let ProtectedJson::Array(fields) = value else {
            continue;
        };
        for field in fields {
            let ProtectedJson::Object(members) = field else {
                continue;
            };
            let object_ref = members.iter().any(|(key, value)| {
                key == "type"
                    && matches!(value, ProtectedJson::String(kind) if kind == "object_ref")
            });
            references.object_ref |= object_ref;
            if object_ref {
                references.character_ref |= members.iter().any(|(key, value)| key == "target"
                    && matches!(value, ProtectedJson::Object(target) if target.iter().any(|(key, value)| key == "kind"
                        && matches!(value, ProtectedJson::String(kind) if kind == "character"))));
            }
            inspect_fields(field, references);
        }
    }
}

enum ProtectedJson {
    Object(Vec<(String, ProtectedJson)>),
    Array(Vec<ProtectedJson>),
    String(String),
    Unsigned(u64),
    Signed(i64),
    Other,
}

impl<'de> Deserialize<'de> for ProtectedJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ProtectedVisitor)
    }
}

struct ProtectedVisitor;

impl<'de> Visitor<'de> for ProtectedVisitor {
    type Value = ProtectedJson;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("可完整结构化读取的 JSON")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut members = Vec::new();
        while let Some(member) = map.next_entry()? {
            members.push(member);
        }
        Ok(ProtectedJson::Object(members))
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Self::Value, S::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(ProtectedJson::Array(values))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(ProtectedJson::String(value.into()))
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(ProtectedJson::String(value))
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(ProtectedJson::Unsigned(value))
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(ProtectedJson::Signed(value))
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(ProtectedJson::Other)
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(ProtectedJson::Other)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(ProtectedJson::Other)
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(ProtectedJson::Other)
    }
}

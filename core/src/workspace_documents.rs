use crate::compiler::LanguageVersion;
use serde::de::{self, Deserializer as _, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub(crate) const MANIFEST_RELATIVE: &str = ".world/project.json";

/// 已注册的展示文档即使 JSON 无法解码也保留原始字节。
/// 保存基线和删除状态由 Project 持有，此类型只提供公开读取视图。
#[derive(Clone)]
pub struct AuthoringDocument {
    pub(crate) bytes: Vec<u8>,
    pub(crate) saved: Option<Vec<u8>>,
    pub(crate) deleted: bool,
    pub(crate) read_only: bool,
}

impl AuthoringDocument {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn is_dirty(&self) -> bool {
        if self.deleted {
            self.saved.is_some()
        } else {
            self.saved.as_ref() != Some(&self.bytes)
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn from_disk(bytes: Vec<u8>, read_only: bool) -> Self {
        Self {
            saved: Some(bytes.clone()),
            bytes,
            deleted: false,
            read_only,
        }
    }

    pub(crate) fn missing(read_only: bool) -> Self {
        Self {
            bytes: Vec::new(),
            saved: None,
            deleted: true,
            read_only,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Registry {
    pub(crate) documents: BTreeMap<PathBuf, bool>,
    pub(crate) maps: BTreeMap<String, PathBuf>,
    pub(crate) graph_views: BTreeMap<String, PathBuf>,
    pub(crate) source_selection: Option<crate::source_config::SourceSelection>,
    pub(crate) diagnostics: Vec<crate::Diagnostic>,
    pub(crate) language_version: LanguageVersion,
}

impl Registry {
    fn report(&mut self, root: &Path, code: &'static str, message: impl Into<String>) {
        self.diagnostics.push(crate::Diagnostic::error(
            code,
            &manifest_path(root).to_string_lossy(),
            crate::Span::new(1, 1, 1),
            message,
        ));
    }
    pub(crate) fn is_registered(&self, path: &Path) -> bool {
        self.documents.contains_key(path)
    }

    pub(crate) fn read_only(&self, path: &Path) -> bool {
        self.documents.get(path).copied().unwrap_or(false)
    }
}

pub(crate) fn manifest_path(root: &Path) -> PathBuf {
    crate::compiler::source_path(&root.join(MANIFEST_RELATIVE))
}

/// 只读取明确指定的清单和清单声明的路径，不递归发现 JSON。
pub(crate) fn parse_registry(root: &Path, manifest: &[u8]) -> Registry {
    let mut registry = Registry::default();
    let mut manifest_read_only = manifest_capability_is_read_only(manifest);
    registry
        .documents
        .insert(manifest_path(root), manifest_read_only);
    let value = match parse_unique_json(manifest) {
        Ok(value) => value,
        Err(error) => {
            registry.report(root, "WS001", format!("清单 JSON 无法解析:{error}"));
            return registry;
        }
    };
    let Some(object) = value.as_object() else {
        registry.report(root, "WS001", "清单 JSON 顶层必须是对象");
        return registry;
    };

    registry.language_version = match object.get("language_version") {
        None => LanguageVersion::V1_9,
        Some(Value::String(version)) => match version.as_str() {
            "1.9" => LanguageVersion::V1_9,
            "1.10" => LanguageVersion::V1_10,
            _ => {
                manifest_read_only = true;
                registry.report(
                    root,
                    "WS003",
                    format!("清单 language_version `{version}` 不受支持，按只读处理"),
                );
                LanguageVersion::V1_9
            }
        },
        _ => {
            manifest_read_only = true;
            registry.report(
                root,
                "WS003",
                "清单 language_version 必须是 \"1.9\" 或 \"1.10\"，按只读处理",
            );
            LanguageVersion::V1_9
        }
    };

    if object.get("schema_version").and_then(Value::as_u64) != Some(1) {
        registry.report(root, "WS002", "清单 schema_version 不受支持，按只读处理");
    }
    if let Some(features) = object.get("required_features") {
        match features.as_array() {
            Some(features) => {
                for feature in features {
                    if !feature.as_str().is_some_and(supported_feature) {
                        registry.report(
                            root,
                            "WS003",
                            format!("清单包含未知 required_feature，按只读处理:{feature}"),
                        );
                    }
                }
            }
            None => registry.report(
                root,
                "WS003",
                "清单 required_features 必须是数组，按只读处理",
            ),
        }
    }

    if let Some(config) = object.get("source_config") {
        match parse_source_selection(root, object, config) {
            Ok(selection) => registry.source_selection = Some(selection),
            Err(message) => registry.report(root, "WS005", message),
        }
    } else if required_feature(object, "workspace.source_sets.v1") {
        registry.report(
            root,
            "WS005",
            "清单声明 workspace.source_sets.v1，但缺少 source_config",
        );
    }

    let mut paths = BTreeSet::new();
    for key in ["maps", "graph_views"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let Some(entries) = value.as_object() else {
            registry.report(
                root,
                "WS004",
                format!("清单 {key} 必须是对象，已跳过其注册项"),
            );
            continue;
        };
        for (id, value) in entries {
            if !valid_id(id) {
                registry.report(root, "WS004", format!("清单 {key}.{id} 的 ID 无效，已跳过"));
                continue;
            }
            let Some(relative) = value.as_str() else {
                registry.report(
                    root,
                    "WS004",
                    format!("清单 {key}.{id} 必须是相对 JSON 路径，已跳过"),
                );
                continue;
            };
            if let Ok(path) = registered_path(root, relative) {
                paths.insert(path.clone());
                if key == "maps" {
                    // registry.maps 只保存已经通过路径边界检查的注册项；重复
                    // JSON key 已在 parse_unique_json 阶段拒绝。
                    registry.maps.insert(id.clone(), path);
                } else {
                    registry.graph_views.insert(id.clone(), path);
                }
            } else {
                registry.report(
                    root,
                    "WS004",
                    format!("清单 {key}.{id} 的路径无效，已跳过:{relative}"),
                );
            }
        }
    }
    for path in paths {
        registry.documents.insert(path, manifest_read_only);
    }
    registry
}

fn required_feature(object: &Map<String, Value>, feature: &str) -> bool {
    object
        .get("required_features")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(feature)))
}

fn parse_source_selection(
    root: &Path,
    manifest: &Map<String, Value>,
    value: &Value,
) -> Result<crate::source_config::SourceSelection, String> {
    if !required_feature(manifest, "workspace.source_sets.v1") {
        return Err("source_config 需要 required_feature workspace.source_sets.v1".into());
    }
    let object = value.as_object().ok_or("source_config 必须是对象")?;
    if object.get("mode").and_then(Value::as_str) != Some("explicit") {
        return Err("source_config.mode 目前只支持 explicit".into());
    }
    let active = source_list(root, object.get("active"), "active")?;
    if active.is_empty() {
        return Err("source_config.active 不能为空".into());
    }
    let archived = source_list(root, object.get("archived"), "archived")?;
    if active.iter().any(|path| archived.contains(path)) {
        return Err("同一源码不能同时属于 active 与 archived".into());
    }
    let entry = manifest
        .get("entry")
        .and_then(Value::as_str)
        .unwrap_or("world.wl");
    let entry = source_file_path(root, entry)?;
    if !active.contains(&entry) {
        return Err("工程 entry 必须属于 source_config.active".into());
    }
    let mut selection = crate::source_config::SourceSelection { active, archived };
    selection.normalize();
    Ok(selection)
}

fn source_list(root: &Path, value: Option<&Value>, field: &str) -> Result<Vec<PathBuf>, String> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| format!("source_config.{field} 必须是数组"))?;
    values
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| format!("source_config.{field} 必须只含字符串路径"))?;
            source_file_path(root, value)
        })
        .collect()
}

fn source_file_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path.as_os_str().is_empty() || relative_path.is_absolute() {
        return Err("源码配置必须使用工作区内相对路径".into());
    }
    let path = crate::compiler::source_path(&root.join(relative_path));
    let root = crate::compiler::source_path(root);
    if !path.starts_with(&root) || path == root {
        return Err("源码配置路径不得越过工作区边界".into());
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("wl") {
        return Err("源码配置路径必须使用 .wl 扩展名".into());
    }
    Ok(path)
}

pub(crate) fn registered_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path.as_os_str().is_empty() || relative_path.is_absolute() {
        return Err("展示文档路径必须是工作区内的相对路径".into());
    }
    let path = crate::compiler::source_path(&root.join(relative_path));
    let root = crate::compiler::source_path(root);
    if !path.starts_with(&root) || path == root {
        return Err("展示文档路径必须位于工作区目录内".into());
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
        return Err("展示文档路径必须使用 .json 扩展名".into());
    }
    Ok(path)
}

pub(crate) fn document_read_only(bytes: &[u8], inherited: bool) -> bool {
    if inherited {
        return true;
    }
    let Ok(value) = parse_unique_json(bytes) else {
        // 格式损坏时允许原始字节修复，不先丢弃作者的原文。
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    if object
        .get("schema_version")
        .and_then(Value::as_u64)
        .is_some_and(|version| version != 1)
    {
        return true;
    }
    object
        .get("required_features")
        .is_some_and(|features| !features_supported(features))
}

fn manifest_capability_is_read_only(bytes: &[u8]) -> bool {
    let Ok(value) = parse_unique_json(bytes) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return true;
    };
    let version_ok = object
        .get("schema_version")
        .and_then(Value::as_u64)
        .is_some_and(|version| version == 1);
    let language_ok = object
        .get("language_version")
        .is_none_or(|version| matches!(version.as_str(), Some("1.9") | Some("1.10")));
    let features_ok = object
        .get("required_features")
        .is_none_or(features_supported);
    !(version_ok && language_ok && features_ok)
}

pub(crate) fn features_supported(features: &Value) -> bool {
    features.as_array().is_some_and(|features| {
        features
            .iter()
            .all(|feature| feature.as_str().is_some_and(supported_feature))
    })
}

fn supported_feature(feature: &str) -> bool {
    matches!(
        feature,
        "presentation.maps.v1"
            | "content.entities.v1"
            | "content.relations.v1"
            | "presentation.geometry.line_area.v1"
            | "presentation.graph_views.v1"
            | "workspace.source_sets.v1"
    )
}

pub(crate) fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// serde_json 默认对重复键采用后者覆盖前者的语义。
/// 清单路径决定载入哪些文件，因此必须在注册前拒绝重复键。
pub(crate) fn parse_unique_json(bytes: &[u8]) -> Result<Value, String> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = deserializer
        .deserialize_any(UniqueValueVisitor)
        .map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(value.0)
}

struct UniqueValueVisitor;

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueValueVisitor)
    }
}

impl<'de> Visitor<'de> for UniqueValueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("对象键不重复的 JSON 值")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let number =
            serde_json::Number::from_f64(value).ok_or_else(|| E::custom("JSON 数字必须有限"))?;
        Ok(UniqueValue(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::String(value.into())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            values.push(value.0);
        }
        Ok(UniqueValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("JSON对象包含重复键:{key}")));
            }
            values.insert(key, value.0);
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}

//! 共享展示文档的读取 DTO 与派生索引。
//!
//! 地图 JSON 是 Project 的展示文档，不是语言源码。这个模块只读取已经由
//! `Project` 注册的地图，使用内容分析得到的 1.9 `TargetRef` 与素材目录做
//! 引用校验，不把地图内容并入 `Program` 或运行指纹。

mod measurement;
mod parse;
pub use measurement::{
    measurement_distance, validate_measurement, MapMeasurement, MEASUREMENT_FEATURE,
};
mod placement_parse;
mod scene_parse;

use crate::catalog::{AssetInfo, Catalog, TargetRef};
use crate::diagnostic::{sort_diagnostics, Diagnostic, Span};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, valid_id};
use crate::CompileResult;
use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// MapDocument 当前支持的格式版本。
pub const MAP_SCHEMA_VERSION: u64 = 1;

pub const TEXT_GEOMETRY_FEATURE: &str = "presentation.geometry.text.v1";

/// 独立标签的纯文本、字号与颜色约束；解析与结构命令共用。
pub fn valid_map_text(text: &str, font_size: f64, color: &str) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= 160
        && text.split('\n').count() <= 4
        && !text.chars().any(|c| c.is_control() && c != '\n')
        && font_size.is_finite()
        && (12.0..=64.0).contains(&font_size)
        && color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

/// 地图展示文档解析结果中的稳定几何类型。
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum MapGeometry {
    #[serde(rename = "point")]
    Point { position: [f64; 2] },
    #[serde(rename = "text")]
    Text {
        position: [f64; 2],
        text: String,
        font_size: f64,
        color: String,
    },
    #[serde(rename = "polyline")]
    Polyline { points: Vec<[f64; 2]> },
    #[serde(rename = "polygon")]
    Polygon { points: Vec<[f64; 2]> },
}

impl MapGeometry {
    pub fn point(position: [f64; 2]) -> Self {
        Self::Point { position }
    }

    pub fn points(&self) -> &[[f64; 2]] {
        match self {
            Self::Point { position } | Self::Text { position, .. } => {
                std::slice::from_ref(position)
            }
            Self::Polyline { points } | Self::Polygon { points } => points,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapCanvas {
    pub width: u32,
    pub height: u32,
    pub unit: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapLayer {
    pub id: String,
    pub title: String,
    pub visible_default: bool,
    pub locked: bool,
    pub style: Option<Map<String, Value>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapRasterLayer {
    pub id: String,
    /// 地图文档中的素材 TargetRef。`asset_info` 缺失时仍保留此引用，供
    /// UI 显示“素材未声明/不可用”的占位状态。
    pub asset: TargetRef,
    pub asset_info: Option<AssetInfo>,
    pub rect: [f64; 4],
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapNavigation {
    pub map_id: String,
    /// 只表示注册表中是否有这个地图；循环导航是合法的。
    pub available: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapPlacement {
    pub id: String,
    pub layer_id: String,
    pub target_ref: Option<TargetRef>,
    pub geometry: MapGeometry,
    pub annotation: String,
    pub role: String,
    pub label_override: Option<String>,
    pub navigation: Option<MapNavigation>,
    pub scope_refs: Vec<TargetRef>,
    pub style: Option<Map<String, Value>>,
    pub extensions: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapDocument {
    /// 完整原始 JSON 值，保留嵌套未知字段。此 DTO 为只读查询；写入须通过 Project。
    pub source: Value,
    pub schema_version: u64,
    pub id: String,
    pub title: String,
    pub raster_layers: Vec<MapRasterLayer>,
    pub canvas: MapCanvas,
    pub measurement: Option<MapMeasurement>,
    pub layer_order: Vec<String>,
    pub layers: BTreeMap<String, MapLayer>,
    pub placements: BTreeMap<String, MapPlacement>,
    pub scene: Option<crate::vector_scene::MapScene>,
    pub extensions: Map<String, Value>,
    /// 根层未知可选字段；完整原文（含所有嵌套字段）见 source。
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct MapPlacementRef {
    pub map_id: String,
    pub placement_id: String,
}

/// Project 当前注册地图的稳定快照。
#[derive(Debug, Clone, Default)]
pub struct MapIndex {
    pub maps: BTreeMap<String, MapDocument>,
    pub placements_by_target: BTreeMap<TargetRef, Vec<MapPlacementRef>>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Serialize for MapIndex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("MapIndex", 3)?;
        state.serialize_field("maps", &self.maps)?;
        let references: Vec<_> = self
            .placements_by_target
            .iter()
            .map(|(target, placements)| PlacementIndexEntry { target, placements })
            .collect();
        state.serialize_field("placements_by_target", &references)?;
        state.serialize_field("diagnostics", &self.diagnostics)?;
        state.end()
    }
}

#[derive(Serialize)]
struct PlacementIndexEntry<'a> {
    target: &'a TargetRef,
    placements: &'a [MapPlacementRef],
}

impl MapIndex {
    /// 返回一个稳定排序的对象反查结果。
    pub fn placements_for(&self, target: &TargetRef) -> Vec<MapPlacementRef> {
        self.placements_by_target
            .get(target)
            .cloned()
            .unwrap_or_default()
    }

    pub fn map(&self, id: &str) -> Option<&MapDocument> {
        self.maps.get(id)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MapParseResult {
    pub(crate) document: Option<MapDocument>,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

/// 从 Project 当前缓冲构造地图与 placement 反查索引。
pub(crate) fn build_map_index(project: &Project, content: &CompileResult) -> MapIndex {
    let mut index = MapIndex::default();
    let manifest = manifest_path(&project.root);
    let Some(manifest_document) = project
        .authoring_documents
        .get(&manifest)
        .filter(|document| !document.deleted)
    else {
        return index;
    };
    let registry = parse_registry(&project.root, &manifest_document.bytes);
    index
        .diagnostics
        .extend(registry.diagnostics.iter().cloned());
    let map_ids: BTreeSet<_> = registry.maps.keys().cloned().collect();

    for (registered_id, path) in registry.maps {
        let Some(document) = project
            .authoring_documents
            .get(&path)
            .filter(|document| !document.deleted)
        else {
            index.diagnostics.push(map_error(
                &path.to_string_lossy(),
                "MAP001",
                format!("注册的地图 `{registered_id}` 文件不存在"),
            ));
            continue;
        };
        let parsed = parse_map_document(
            &document.bytes,
            &path,
            &registered_id,
            &content.analysis.catalog,
            &map_ids,
            content.options,
        );
        index.diagnostics.extend(parsed.diagnostics);
        let Some(map) = parsed.document else {
            continue;
        };
        if index.maps.contains_key(&map.id) {
            index.diagnostics.push(map_error(
                &path.to_string_lossy(),
                "MAP003",
                format!("地图 ID `{}` 在注册地图中重复", map.id),
            ));
            continue;
        }
        for placement in map.placements.values() {
            if let Some(target) = &placement.target_ref {
                index
                    .placements_by_target
                    .entry(target.clone())
                    .or_default()
                    .push(MapPlacementRef {
                        map_id: map.id.clone(),
                        placement_id: placement.id.clone(),
                    });
            }
        }
        if let Some(scene) = &map.scene {
            for node in scene.nodes.values() {
                if let Some(target) = &node.target_ref {
                    index
                        .placements_by_target
                        .entry(target.clone())
                        .or_default()
                        .push(MapPlacementRef {
                            map_id: map.id.clone(),
                            placement_id: node.id.clone(),
                        });
                }
            }
        }
        index.maps.insert(map.id.clone(), map);
    }
    for placements in index.placements_by_target.values_mut() {
        placements.sort();
    }
    sort_diagnostics(&mut index.diagnostics);
    index
}

/// Project 暴露的展示地图入口。
impl Project {
    pub fn map_index(&self) -> MapIndex {
        build_map_index(self, &self.compile_current())
    }
}
pub(crate) fn parse_map_document(
    bytes: &[u8],
    file: &Path,
    registered_id: &str,
    catalog: &Catalog,
    map_ids: &BTreeSet<String>,
    options: crate::CompileOptions,
) -> MapParseResult {
    parse::parse_map_document(bytes, file, registered_id, catalog, map_ids, options)
}

fn required_object<'a>(
    value: Option<&'a Value>,
    name: &str,
    file: &str,
    code: &'static str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a Map<String, Value>> {
    let Some(value) = value else {
        diagnostics.push(map_error(file, code, format!("缺少 `{name}` 对象")));
        return None;
    };
    let Some(object) = value.as_object() else {
        diagnostics.push(map_error(file, code, format!("`{name}` 必须是对象")));
        return None;
    };
    Some(object)
}

fn required_string(
    value: Option<&Value>,
    key: &str,
    file: &str,
    code: &'static str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let Some(object) = value.and_then(Value::as_object) else {
        diagnostics.push(map_error(file, code, format!("`{key}` 必须是字符串")));
        return None;
    };
    required_string_in(object, key, file, code, diagnostics)
}

fn required_string_in(
    object: &Map<String, Value>,
    key: &str,
    file: &str,
    code: &'static str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let Some(string) = object.get(key).and_then(Value::as_str) else {
        diagnostics.push(map_error(file, code, format!("`{key}` 必须是字符串")));
        return None;
    };
    Some(string.into())
}

fn parse_optional_string(
    value: Option<&Value>,
    key: &str,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    let Some(value) = value.as_str() else {
        diagnostics.push(map_error(
            file,
            "MAP006",
            format!("`{key}` 必须是字符串或 null"),
        ));
        return None;
    };
    Some(value.into())
}

fn parse_optional_object(
    value: Option<&Value>,
    key: &str,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Map<String, Value>> {
    let value = value?;
    let Some(object) = value.as_object() else {
        diagnostics.push(map_error(file, "MAP001", format!("`{key}` 必须是对象")));
        return None;
    };
    Some(object.clone())
}

fn parse_extensions(
    value: Option<&Value>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Map<String, Value>> {
    parse_optional_object(value, "extensions", file, diagnostics)
        .or_else(|| value.is_none().then(Map::new))
}

fn extras(object: &Map<String, Value>, known: &[&str]) -> Map<String, Value> {
    object
        .iter()
        .filter(|(key, _)| !known.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn finite_unit(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
}

fn map_error(file: &str, code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, file, Span::new(1, 1, 1), message)
}

fn map_warning(file: &str, code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::warning(code, file, Span::new(1, 1, 1), message)
}

fn parse_navigation(
    value: Option<&Value>,
    file: &str,
    map_ids: &BTreeSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<MapNavigation>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(object) = value.as_object() else {
        diagnostics.push(map_error(file, "MAP009", "navigation 必须是对象或 null"));
        return Err(());
    };
    let Some(map_id) = object.get("map_id").and_then(Value::as_str) else {
        diagnostics.push(map_error(file, "MAP009", "navigation.map_id 必须是字符串"));
        return Err(());
    };
    if !valid_id(map_id) {
        diagnostics.push(map_error(
            file,
            "MAP009",
            format!("navigation.map_id `{map_id}` 无效"),
        ));
        return Err(());
    }
    let available = map_ids.contains(map_id);
    if !available {
        diagnostics.push(map_warning(
            file,
            "MAP012",
            format!("navigation 指向未注册地图 `{map_id}`"),
        ));
    }
    Ok(Some(MapNavigation {
        map_id: map_id.into(),
        available,
        extra: extras(object, &["map_id"]),
    }))
}

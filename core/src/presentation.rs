//! 共享展示文档的读取 DTO 与派生索引。
//!
//! 地图 JSON 是 Project 的展示文档，不是语言源码。这个模块只读取已经由
//! `Project` 注册的地图，使用内容分析得到的 1.9 `TargetRef` 与素材目录做
//! 引用校验，不把地图内容并入 `Program` 或运行指纹。

use crate::catalog::{AssetInfo, Catalog, TargetRef};
use crate::diagnostic::{sort_diagnostics, Diagnostic, Span};
use crate::project::Project;
use crate::workspace_documents::{manifest_path, parse_registry, parse_unique_json, valid_id};
use crate::CompileResult;
use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// MapDocument 当前支持的格式版本。
pub const MAP_SCHEMA_VERSION: u64 = 1;

/// 地图展示文档解析结果中的稳定几何类型。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum MapGeometry {
    #[serde(rename = "point")]
    Point { position: [f64; 2] },
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
            Self::Point { position } => std::slice::from_ref(position),
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
    pub layer_order: Vec<String>,
    pub layers: BTreeMap<String, MapLayer>,
    pub placements: BTreeMap<String, MapPlacement>,
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
    let file = file.to_string_lossy();
    let mut diagnostics = Vec::new();
    let value = match parse_unique_json(bytes) {
        Ok(value) => value,
        Err(error) => {
            diagnostics.push(map_error(
                &file,
                "MAP001",
                format!("地图 JSON 无法解析:{error}"),
            ));
            return MapParseResult {
                document: None,
                diagnostics,
            };
        }
    };
    let Some(object) = value.as_object() else {
        diagnostics.push(map_error(&file, "MAP001", "地图 JSON 顶层必须是对象"));
        return MapParseResult {
            document: None,
            diagnostics,
        };
    };

    let version = object.get("schema_version").and_then(Value::as_u64);
    if version != Some(MAP_SCHEMA_VERSION) {
        diagnostics.push(map_error(
            &file,
            "MAP002",
            "地图 schema_version 不受支持，只能只读查看",
        ));
        return MapParseResult {
            document: None,
            diagnostics,
        };
    }
    if let Some(features) = object.get("required_features") {
        if !crate::workspace_documents::features_supported(features) {
            diagnostics.push(map_error(
                &file,
                "MAP002",
                "地图包含当前工具不支持的必需能力，只能只读查看",
            ));
            return MapParseResult {
                document: None,
                diagnostics,
            };
        }
    }

    let id = match required_string_in(object, "id", &file, "MAP003", &mut diagnostics) {
        Some(id) if valid_id(&id) => id,
        Some(id) => {
            diagnostics.push(map_error(
                &file,
                "MAP003",
                format!("地图 ID `{id}` 不符合标识符格式"),
            ));
            id
        }
        None => String::new(),
    };
    if !id.is_empty() && id != registered_id {
        diagnostics.push(map_error(
            &file,
            "MAP003",
            format!("清单注册 ID `{registered_id}` 与地图 ID `{id}` 不一致"),
        ));
    }
    let title = required_string_in(object, "title", &file, "MAP001", &mut diagnostics)
        .filter(|title| !title.is_empty())
        .unwrap_or_default();

    let title_valid = object
        .get("title")
        .and_then(Value::as_str)
        .is_some_and(|title| !title.is_empty());
    if object.get("title").and_then(Value::as_str) == Some("") {
        diagnostics.push(map_error(&file, "MAP001", "地图 title 不能为空"));
    }
    let canvas = parse_canvas(object.get("canvas"), &file, &mut diagnostics);
    let layers = parse_layers(object.get("layers"), &file, &mut diagnostics);
    let layer_order = parse_layer_order(object.get("layer_order"), &file, &mut diagnostics);
    let mut structural_error =
        !id.is_empty() && id != registered_id || id.is_empty() || !title_valid;
    structural_error |= canvas.is_none() || layers.is_none() || layer_order.is_none();
    let layers = layers.unwrap_or_default();
    let layer_order = layer_order.unwrap_or_default();
    if layers.len() != layer_order.len()
        || layer_order
            .iter()
            .any(|layer_id| !layers.contains_key(layer_id))
    {
        diagnostics.push(map_error(
            &file,
            "MAP005",
            "layer_order 必须恰好包含 layers 中的每个图层",
        ));
        structural_error = true;
    }

    let raster_value = object
        .get("raster_layers")
        .or_else(|| object.get("background"));
    let raster_layers = if object
        .get("raster_layers")
        .is_some_and(|value| !value.is_array())
    {
        diagnostics.push(map_error(&file, "MAP008", "raster_layers 必须是数组"));
        None
    } else {
        parse_raster_layers(raster_value, &file, catalog, &mut diagnostics)
    };
    structural_error |= raster_layers.is_none();

    let placements = parse_placements(
        object.get("placements"),
        &file,
        &layers,
        catalog,
        map_ids,
        options,
        &mut diagnostics,
    );
    structural_error |= placements.is_none();
    let extensions = parse_extensions(object.get("extensions"), &file, &mut diagnostics);
    structural_error |= extensions.is_none();

    let known = [
        "schema_version",
        "id",
        "title",
        "background",
        "raster_layers",
        "canvas",
        "layer_order",
        "layers",
        "placements",
        "extensions",
        "required_features",
    ];
    let extra = extras(object, &known);
    let document = (!structural_error).then(|| MapDocument {
        source: value,
        schema_version: MAP_SCHEMA_VERSION,
        id,
        title,
        raster_layers: raster_layers.unwrap_or_default(),
        canvas: canvas.unwrap_or_else(|| MapCanvas {
            width: 1,
            height: 1,
            unit: "normalized".into(),
            extra: Map::new(),
        }),
        layer_order,
        layers,
        placements: placements.unwrap_or_default(),
        extensions: extensions.unwrap_or_default(),
        extra,
    });
    MapParseResult {
        document,
        diagnostics,
    }
}

fn parse_canvas(
    value: Option<&Value>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MapCanvas> {
    let object = required_object(value, "canvas", file, "MAP004", diagnostics)?;
    let width = object.get("width").and_then(Value::as_u64);
    let height = object.get("height").and_then(Value::as_u64);
    let unit = object.get("unit").and_then(Value::as_str);
    let mut valid = true;
    if width.is_none_or(|v| v == 0 || v > i32::MAX as u64) {
        diagnostics.push(map_error(
            file,
            "MAP004",
            "canvas.width 必须是正整数且不超过 2^31-1",
        ));
        valid = false;
    }
    if height.is_none_or(|v| v == 0 || v > i32::MAX as u64) {
        diagnostics.push(map_error(
            file,
            "MAP004",
            "canvas.height 必须是正整数且不超过 2^31-1",
        ));
        valid = false;
    }
    if unit != Some("normalized") {
        diagnostics.push(map_error(file, "MAP004", "canvas.unit 必须是 normalized"));
        valid = false;
    }
    if !valid {
        return None;
    }
    let known = ["width", "height", "unit"];
    Some(MapCanvas {
        width: width.unwrap() as u32,
        height: height.unwrap() as u32,
        unit: unit.unwrap().into(),
        extra: extras(object, &known),
    })
}

fn parse_layers(
    value: Option<&Value>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BTreeMap<String, MapLayer>> {
    let object = required_object(value, "layers", file, "MAP005", diagnostics)?;
    let mut layers = BTreeMap::new();
    let mut valid = true;
    for (id, value) in object {
        if !valid_id(id) {
            diagnostics.push(map_error(
                file,
                "MAP005",
                format!("图层 ID `{id}` 不符合标识符格式"),
            ));
            valid = false;
        }
        let Some(layer) = value.as_object() else {
            diagnostics.push(map_error(file, "MAP005", format!("图层 `{id}` 必须是对象")));
            valid = false;
            continue;
        };
        let title = required_string(Some(value), "title", file, "MAP005", diagnostics);
        if title.as_deref().is_none_or(str::is_empty) {
            diagnostics.push(map_error(
                file,
                "MAP005",
                format!("图层 `{id}` 的 title 不能为空"),
            ));
            valid = false;
        }
        let visible = layer.get("visible_default").and_then(Value::as_bool);
        let locked = layer.get("locked").and_then(Value::as_bool);
        if visible.is_none() || locked.is_none() {
            diagnostics.push(map_error(
                file,
                "MAP005",
                format!("图层 `{id}` 的 visible_default 与 locked 必须是布尔值"),
            ));
            valid = false;
        }
        let style = parse_optional_object(layer.get("style"), "style", file, diagnostics);
        if layer.get("style").is_some() && style.is_none() {
            valid = false;
        }
        let known = ["title", "visible_default", "locked", "style"];
        layers.insert(
            id.clone(),
            MapLayer {
                id: id.clone(),
                title: title.unwrap_or_default(),
                visible_default: visible.unwrap_or(false),
                locked: locked.unwrap_or(false),
                style,
                extra: extras(layer, &known),
            },
        );
    }
    valid.then_some(layers)
}

fn parse_layer_order(
    value: Option<&Value>,
    file: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<String>> {
    let Some(array) = value.and_then(Value::as_array) else {
        diagnostics.push(map_error(file, "MAP005", "layer_order 必须是数组"));
        return None;
    };
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut valid = true;
    for value in array {
        let Some(id) = value.as_str() else {
            diagnostics.push(map_error(
                file,
                "MAP005",
                "layer_order 中的图层 ID 必须是字符串",
            ));
            valid = false;
            continue;
        };
        if !valid_id(id) || !seen.insert(id.to_string()) {
            diagnostics.push(map_error(
                file,
                "MAP005",
                format!("layer_order 含有无效或重复图层 ID `{id}`"),
            ));
            valid = false;
        }
        result.push(id.into());
    }
    valid.then_some(result)
}

fn parse_raster_layers(
    value: Option<&Value>,
    file: &str,
    catalog: &Catalog,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<MapRasterLayer>> {
    let Some(value) = value else {
        return Some(Vec::new());
    };
    // `background` 是早期单图层别名；读取时归一成一个 raster layer，
    // 但 Project 仍保存调用方原始 JSON 字节。
    let owned;
    let value = if value.is_array() {
        value
    } else if let Some(object) = value.as_object() {
        let mut layer = object.clone();
        layer
            .entry("id")
            .or_insert_with(|| Value::String("background".into()));
        layer
            .entry("rect")
            .or_insert_with(|| Value::Array(vec![0.into(), 0.into(), 1.into(), 1.into()]));
        owned = Value::Array(vec![Value::Object(layer)]);
        &owned
    } else if let Some(asset_id) = value.as_str() {
        owned = Value::Array(vec![serde_json::json!({
            "id": "background",
            "asset": {"kind": "asset", "id": asset_id},
            "rect": [0, 0, 1, 1]
        })]);
        &owned
    } else {
        value
    };
    let Some(array) = value.as_array() else {
        diagnostics.push(map_error(file, "MAP008", "raster_layers 必须是数组"));
        return None;
    };
    let mut layers = Vec::new();
    let mut ids = BTreeSet::new();
    let mut valid = true;
    for raster in array {
        let Some(object) = raster.as_object() else {
            diagnostics.push(map_error(file, "MAP008", "栅格图层必须是对象"));
            valid = false;
            continue;
        };
        let id = required_string(Some(raster), "id", file, "MAP008", diagnostics);
        let Some(id) = id else {
            valid = false;
            continue;
        };
        if !valid_id(&id) || !ids.insert(id.clone()) {
            diagnostics.push(map_error(
                file,
                "MAP008",
                format!("栅格图层 ID `{id}` 无效或重复"),
            ));
            valid = false;
        }
        let Some(asset_object) = object.get("asset").and_then(Value::as_object) else {
            diagnostics.push(map_error(
                file,
                "MAP008",
                format!("栅格图层 `{id}` 缺少 asset 对象"),
            ));
            valid = false;
            continue;
        };
        let kind = asset_object.get("kind").and_then(Value::as_str);
        let asset_id = asset_object.get("id").and_then(Value::as_str);
        if kind != Some("asset") || asset_id.is_none_or(str::is_empty) {
            diagnostics.push(map_error(
                file,
                "MAP008",
                format!("栅格图层 `{id}` 的 asset 必须是非空 asset 引用"),
            ));
            valid = false;
            continue;
        }
        let asset_id = asset_id.unwrap();
        let asset = TargetRef::new("asset", asset_id);
        let declared_asset = catalog.assets.get(asset_id);
        let asset_info = declared_asset
            .filter(|asset| asset.kind == "image")
            .cloned();
        if declared_asset.is_some_and(|asset| asset.kind != "image") {
            diagnostics.push(map_warning(
                file,
                "MAP010",
                format!("栅格图层引用的素材 `{asset_id}` 不是图片，图层不可用"),
            ));
        } else if asset_info.is_none() {
            diagnostics.push(map_warning(
                file,
                "MAP010",
                format!("栅格图层引用的素材 `{asset_id}` 未声明，图层不可用"),
            ));
        } else if !asset_info.as_ref().is_some_and(|asset| asset.available) {
            diagnostics.push(map_warning(
                file,
                "MAP010",
                format!("栅格图层引用的素材 `{asset_id}` 不可用"),
            ));
        }
        let Some(rect) = parse_rect(
            object.get("rect"),
            file,
            &format!("raster_layers.{id}.rect"),
            diagnostics,
        ) else {
            valid = false;
            continue;
        };
        let known = ["id", "asset", "rect"];
        layers.push(MapRasterLayer {
            id,
            asset,
            asset_info,
            rect,
            extra: extras(object, &known),
        });
    }
    valid.then_some(layers)
}

fn parse_placements(
    value: Option<&Value>,
    file: &str,
    layers: &BTreeMap<String, MapLayer>,
    catalog: &Catalog,
    map_ids: &BTreeSet<String>,
    options: crate::CompileOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BTreeMap<String, MapPlacement>> {
    let object = required_object(value, "placements", file, "MAP006", diagnostics)?;
    let mut placements = BTreeMap::new();
    let mut valid = true;
    for (id, value) in object {
        if !valid_id(id) {
            diagnostics.push(map_error(
                file,
                "MAP006",
                format!("标记 ID `{id}` 不符合标识符格式"),
            ));
            valid = false;
        }
        let Some(placement) = value.as_object() else {
            diagnostics.push(map_error(file, "MAP006", format!("标记 `{id}` 必须是对象")));
            valid = false;
            continue;
        };
        let layer_id = required_string(Some(value), "layer_id", file, "MAP005", diagnostics);
        if layer_id
            .as_deref()
            .is_none_or(|layer| !layers.contains_key(layer))
        {
            diagnostics.push(map_error(
                file,
                "MAP005",
                format!("标记 `{id}` 引用了不存在的图层"),
            ));
            valid = false;
        }
        if !placement.contains_key("target_ref") {
            diagnostics.push(map_error(
                file,
                "MAP007",
                format!("标记 `{id}` 缺少 target_ref"),
            ));
            valid = false;
        }
        let target = match parse_target(
            placement.get("target_ref"),
            true,
            file,
            &format!("placements.{id}.target_ref"),
            options,
            diagnostics,
        ) {
            Ok(target) => target,
            Err(()) => {
                valid = false;
                None
            }
        };
        if let Some(target) = &target {
            warn_unresolved_target(target, &format!("标记 `{id}`"), file, catalog, diagnostics);
        }
        let geometry = match parse_geometry(
            placement.get("geometry"),
            file,
            &format!("placements.{id}.geometry"),
            diagnostics,
        ) {
            Some(geometry) => geometry,
            None => {
                valid = false;
                MapGeometry::point([0.0, 0.0])
            }
        };
        let annotation = required_string(Some(value), "annotation", file, "MAP006", diagnostics);
        let role = required_string(Some(value), "role", file, "MAP006", diagnostics)
            .filter(|role| !role.is_empty());
        if placement.get("role").and_then(Value::as_str) == Some("") {
            diagnostics.push(map_error(
                file,
                "MAP006",
                format!("标记 `{id}` 的 role 不能为空"),
            ));
        }
        if annotation.is_none() || role.is_none() {
            valid = false;
        }
        let label_override = parse_optional_string(
            placement.get("label_override"),
            "label_override",
            file,
            diagnostics,
        );
        if placement.get("label_override").is_some()
            && !placement.get("label_override").is_some_and(Value::is_null)
            && label_override.is_none()
        {
            valid = false;
        }
        let navigation =
            match parse_navigation(placement.get("navigation"), file, map_ids, diagnostics) {
                Ok(navigation) => navigation,
                Err(()) => {
                    valid = false;
                    None
                }
            };
        let scope_refs = match parse_target_array(
            placement.get("scope_refs"),
            file,
            &format!("placements.{id}.scope_refs"),
            options,
            diagnostics,
        ) {
            Ok(scope_refs) => {
                for (index, target) in scope_refs.iter().enumerate() {
                    warn_unresolved_target(
                        target,
                        &format!("placements.{id}.scope_refs.{index}"),
                        file,
                        catalog,
                        diagnostics,
                    );
                }
                scope_refs
            }
            Err(()) => {
                valid = false;
                Vec::new()
            }
        };
        let style = parse_optional_object(placement.get("style"), "style", file, diagnostics);
        if placement.get("style").is_some() && style.is_none() {
            valid = false;
        }
        let extensions = parse_extensions(placement.get("extensions"), file, diagnostics);
        if placement.get("extensions").is_some() && extensions.is_none() {
            valid = false;
        }
        let known = [
            "layer_id",
            "target_ref",
            "geometry",
            "annotation",
            "role",
            "label_override",
            "navigation",
            "scope_refs",
            "style",
            "extensions",
        ];
        placements.insert(
            id.clone(),
            MapPlacement {
                id: id.clone(),
                layer_id: layer_id.unwrap_or_default(),
                target_ref: target,
                geometry,
                annotation: annotation.unwrap_or_default(),
                role: role.unwrap_or_default(),
                label_override,
                navigation,
                scope_refs,
                style,
                extensions: extensions.unwrap_or_default(),
                extra: extras(placement, &known),
            },
        );
    }
    valid.then_some(placements)
}

fn parse_geometry(
    value: Option<&Value>,
    file: &str,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MapGeometry> {
    let object = required_object(value, "geometry", file, "MAP006", diagnostics)?;
    match object.get("kind").and_then(Value::as_str) {
        Some("point") => {
            let position = parse_point(object.get("position"), file, path, diagnostics)?;
            Some(MapGeometry::Point { position })
        }
        Some("polyline") => {
            let points = parse_points(object.get("points"), 2, file, path, diagnostics)?;
            Some(MapGeometry::Polyline { points })
        }
        Some("polygon") => {
            let points = parse_points(object.get("points"), 3, file, path, diagnostics)?;
            if points.first() == points.last() || polygon_has_duplicate_points(&points) {
                diagnostics.push(map_error(
                    file,
                    "MAP006",
                    format!("{path} 多边形最后一点不能重复首点"),
                ));
                return None;
            }
            if polygon_is_degenerate(&points) || polygon_self_intersects(&points) {
                diagnostics.push(map_error(
                    file,
                    "MAP006",
                    format!("{path} 多边形必须是非退化的简单多边形"),
                ));
                return None;
            }
            Some(MapGeometry::Polygon { points })
        }
        Some(kind) => {
            diagnostics.push(map_error(
                file,
                "MAP006",
                format!("{path} 不支持几何类型 `{kind}`"),
            ));
            None
        }
        None => {
            diagnostics.push(map_error(
                file,
                "MAP006",
                format!("{path}.kind 必须是 point、polyline 或 polygon"),
            ));
            None
        }
    }
}

fn parse_point(
    value: Option<&Value>,
    file: &str,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<[f64; 2]> {
    let Some(array) = value.and_then(Value::as_array) else {
        diagnostics.push(map_error(
            file,
            "MAP006",
            format!("{path} 必须是二维坐标数组"),
        ));
        return None;
    };
    if array.len() != 2 {
        diagnostics.push(map_error(
            file,
            "MAP006",
            format!("{path} 必须恰好有两个坐标"),
        ));
        return None;
    }
    let x = finite_unit(&array[0]);
    let y = finite_unit(&array[1]);
    if x.is_none() || y.is_none() {
        diagnostics.push(map_error(
            file,
            "MAP006",
            format!("{path} 坐标必须是有限数且在 [0,1] 内"),
        ));
        return None;
    }
    Some([x.unwrap(), y.unwrap()])
}

fn parse_points(
    value: Option<&Value>,
    minimum: usize,
    file: &str,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<[f64; 2]>> {
    let Some(array) = value.and_then(Value::as_array) else {
        diagnostics.push(map_error(file, "MAP006", format!("{path} 必须是坐标数组")));
        return None;
    };
    if array.len() < minimum {
        diagnostics.push(map_error(
            file,
            "MAP006",
            format!("{path} 至少需要 {minimum} 个点"),
        ));
        return None;
    }
    let mut points = Vec::with_capacity(array.len());
    for point in array {
        let point = parse_point(Some(point), file, path, diagnostics)?;
        points.push(point);
    }
    Some(points)
}

fn parse_rect(
    value: Option<&Value>,
    file: &str,
    path: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<[f64; 4]> {
    let Some(array) = value.and_then(Value::as_array) else {
        diagnostics.push(map_error(
            file,
            "MAP008",
            format!("{path} 必须是四维矩形数组"),
        ));
        return None;
    };
    if array.len() != 4 {
        diagnostics.push(map_error(
            file,
            "MAP008",
            format!("{path} 必须恰好有四个坐标"),
        ));
        return None;
    }
    let values: Option<Vec<_>> = array.iter().map(finite_unit).collect();
    let Some(values) = values else {
        diagnostics.push(map_error(
            file,
            "MAP008",
            format!("{path} 坐标必须是有限数且在 [0,1] 内"),
        ));
        return None;
    };
    if values[0] >= values[2] || values[1] >= values[3] {
        diagnostics.push(map_error(
            file,
            "MAP008",
            format!("{path} 的左上角必须在右下角左上方，宽高须大于零"),
        ));
        return None;
    }
    Some([values[0], values[1], values[2], values[3]])
}

fn parse_target(
    value: Option<&Value>,
    nullable: bool,
    file: &str,
    path: &str,
    options: crate::CompileOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<TargetRef>, ()> {
    if value.is_none() || value.is_some_and(Value::is_null) {
        if nullable {
            return Ok(None);
        }
        diagnostics.push(map_error(file, "MAP007", format!("{path} 不能为空")));
        return Err(());
    }
    let Some(object) = value.and_then(Value::as_object) else {
        diagnostics.push(map_error(
            file,
            "MAP007",
            format!("{path} 必须是对象或 null"),
        ));
        return Err(());
    };
    let kind = object.get("kind").and_then(Value::as_str);
    let id = object.get("id").and_then(Value::as_str);
    if kind.is_none()
        || id.is_none_or(str::is_empty)
        || !crate::catalog::is_target_kind(kind.unwrap(), options)
    {
        diagnostics.push(map_error(
            file,
            "MAP007",
            format!("{path} 必须引用现有 1.9 TargetRef(kind,id)"),
        ));
        return Err(());
    }
    Ok(Some(TargetRef::new(kind.unwrap(), id.unwrap())))
}

fn parse_target_array(
    value: Option<&Value>,
    file: &str,
    path: &str,
    options: crate::CompileOptions,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<TargetRef>, ()> {
    let Some(array) = value else {
        return Ok(Vec::new());
    };
    let Some(array) = array.as_array() else {
        diagnostics.push(map_error(
            file,
            "MAP007",
            format!("{path} 必须是 TargetRef 数组"),
        ));
        return Err(());
    };
    let mut refs = Vec::with_capacity(array.len());
    for (index, value) in array.iter().enumerate() {
        match parse_target(
            Some(value),
            false,
            file,
            &format!("{path}.{index}"),
            options,
            diagnostics,
        ) {
            Ok(Some(target)) => refs.push(target),
            Ok(None) | Err(()) => return Err(()),
        }
    }
    Ok(refs)
}

fn warn_unresolved_target(
    target: &TargetRef,
    label: &str,
    file: &str,
    catalog: &Catalog,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if catalog.object(target).is_none() {
        diagnostics.push(map_warning(
            file,
            "MAP011",
            format!(
                "{label} 引用的对象 {} `{}` 尚未解析",
                target.kind, target.id
            ),
        ));
    }
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

fn polygon_is_degenerate(points: &[[f64; 2]]) -> bool {
    signed_area(points).abs() <= f64::EPSILON
}

fn polygon_has_duplicate_points(points: &[[f64; 2]]) -> bool {
    points
        .iter()
        .enumerate()
        .any(|(index, point)| points.iter().skip(index + 1).any(|other| point == other))
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        * 0.5
}

fn polygon_self_intersects(points: &[[f64; 2]]) -> bool {
    let len = points.len();
    for i in 0..len {
        let a = points[i];
        let b = points[(i + 1) % len];
        for j in (i + 1)..len {
            if j == i + 1 || (i == 0 && j == len - 1) {
                continue;
            }
            let c = points[j];
            let d = points[(j + 1) % len];
            if segments_intersect(a, b, c, d) {
                return true;
            }
        }
    }
    false
}

fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    const EPS: f64 = 1e-12;
    let ab_c = cross(a, b, c);
    let ab_d = cross(a, b, d);
    let cd_a = cross(c, d, a);
    let cd_b = cross(c, d, b);
    if ((ab_c > EPS && ab_d < -EPS) || (ab_c < -EPS && ab_d > EPS))
        && ((cd_a > EPS && cd_b < -EPS) || (cd_a < -EPS && cd_b > EPS))
    {
        return true;
    }
    (ab_c.abs() <= EPS && on_segment(a, b, c))
        || (ab_d.abs() <= EPS && on_segment(a, b, d))
        || (cd_a.abs() <= EPS && on_segment(c, d, a))
        || (cd_b.abs() <= EPS && on_segment(c, d, b))
}

fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn on_segment(a: [f64; 2], b: [f64; 2], point: [f64; 2]) -> bool {
    const EPS: f64 = 1e-12;
    point[0] >= a[0].min(b[0]) - EPS
        && point[0] <= a[0].max(b[0]) + EPS
        && point[1] >= a[1].min(b[1]) - EPS
        && point[1] <= a[1].max(b[1]) + EPS
}

fn map_error(file: &str, code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, file, Span::new(1, 1, 1), message)
}

fn map_warning(file: &str, code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::warning(code, file, Span::new(1, 1, 1), message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concave_polygon_is_valid_but_crossing_polygon_is_rejected() {
        let concave = vec![
            [0.1, 0.1],
            [0.9, 0.1],
            [0.9, 0.4],
            [0.5, 0.4],
            [0.5, 0.9],
            [0.1, 0.9],
        ];
        assert!(!polygon_self_intersects(&concave));
        let crossing = vec![[0.1, 0.1], [0.9, 0.9], [0.1, 0.9], [0.9, 0.1]];
        assert!(polygon_self_intersects(&crossing));
    }
}

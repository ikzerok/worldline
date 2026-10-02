use super::placement_parse::parse_placements;
use super::{
    extras, finite_unit, map_error, map_warning, parse_extensions, parse_optional_object,
    required_object, required_string, required_string_in, MapCanvas, MapDocument, MapLayer,
    MapParseResult, MapRasterLayer, MAP_SCHEMA_VERSION,
};
use crate::catalog::{Catalog, TargetRef};
use crate::diagnostic::Diagnostic;
use crate::workspace_documents::{parse_unique_json, valid_id};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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
    let measurement =
        super::measurement::parse_measurement(object.get("measurement"), canvas.as_ref());
    let measurement_valid = measurement.is_ok();
    if let Err(message) = &measurement {
        diagnostics.push(map_error(&file, "MAP013", message));
    }
    let measurement = measurement.ok().flatten();
    let measurement_declared = object
        .get("required_features")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(super::MEASUREMENT_FEATURE))
        });
    if object.contains_key("measurement") && !measurement_declared {
        diagnostics.push(map_error(
            &file,
            "MAP002",
            "地图校准缺少 presentation.measurement.v1 必需能力声明",
        ));
    }
    let layers = parse_layers(object.get("layers"), &file, &mut diagnostics);
    let layer_order = parse_layer_order(object.get("layer_order"), &file, &mut diagnostics);
    let mut structural_error =
        !id.is_empty() && id != registered_id || id.is_empty() || !title_valid;
    structural_error |=
        !measurement_valid || (object.contains_key("measurement") && !measurement_declared);
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
    if placements.as_ref().is_some_and(|items| {
        items
            .values()
            .any(|item| matches!(item.geometry, super::MapGeometry::Text { .. }))
    }) && !object
        .get("required_features")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(super::TEXT_GEOMETRY_FEATURE))
        })
    {
        diagnostics.push(map_error(
            &file,
            "MAP002",
            "文字标签地图缺少 presentation.geometry.text.v1 必需能力声明",
        ));
        structural_error = true;
    }
    let extensions = parse_extensions(object.get("extensions"), &file, &mut diagnostics);
    structural_error |= extensions.is_none();
    let scene = super::scene_parse::parse_scene(
        object,
        &layers,
        &placements.clone().unwrap_or_default(),
        catalog,
        map_ids,
        options,
        &file,
        &mut diagnostics,
    );
    structural_error |= scene.is_err();

    let known = [
        "schema_version",
        "id",
        "title",
        "background",
        "raster_layers",
        "canvas",
        "measurement",
        "layer_order",
        "layers",
        "placements",
        "scene",
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
        measurement,
        layer_order,
        layers,
        placements: placements.unwrap_or_default(),
        scene: scene.ok().flatten(),
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

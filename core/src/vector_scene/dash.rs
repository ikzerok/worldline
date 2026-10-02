//! 有界虚线样式、能力声明与安全 SVG 编码；不展开路径。
use super::{
    MapScene, SceneError, SceneGeometry, SceneStyle, MAX_DASH_ENTRIES, SCENE_DASH_FEATURE,
    WORLD_LIMIT,
};
use serde_json::{Map, Value};
use std::fmt::Write;

fn error(field: &str, message: &str) -> SceneError {
    let mut error = SceneError::new("SCENE_STYLE", message);
    error.field = Some(field.into());
    error
}

pub(super) fn validate_style(style: &SceneStyle) -> Result<(), SceneError> {
    check_array_limit(style, MAX_DASH_ENTRIES)?;
    if let Some(array) = &style.stroke_dasharray {
        for value in array {
            if !value.is_finite()
                || *value < 0.0
                || *value > WORLD_LIMIT
                || (*value > 0.0 && !(*value as f32).is_normal())
            {
                return Err(error(
                    "stroke-dasharray",
                    "虚线长度必须是有限非负 user unit/px，且不能超过世界预算或在渲染后端下溢",
                ));
            }
        }
    }
    if style.stroke_dashoffset.is_some_and(|value| {
        !value.is_finite() || value.abs() > WORLD_LIMIT || (value != 0.0 && (value as f32) == 0.0)
    }) {
        return Err(error(
            "stroke-dashoffset",
            "虚线偏移必须是有限 user unit/px，且不能超过世界预算或在渲染后端下溢",
        ));
    }
    Ok(())
}

pub(super) fn check_array_limit(style: &SceneStyle, maximum: usize) -> Result<(), SceneError> {
    if style
        .stroke_dasharray
        .as_ref()
        .is_some_and(|a| a.len() > maximum)
    {
        let mut error = super::validate::limit("虚线数列项数");
        error.field = Some("stroke-dasharray".into());
        return Err(error);
    }
    Ok(())
}

fn scalar(source: &str, field: &str) -> Result<f64, SceneError> {
    let text = source.strip_suffix("px").unwrap_or(source);
    let parsed = super::svg_path::numbers(text).map_err(|_| {
        error(
            field,
            "虚线只接受有限十进制 user unit 或 px；不支持百分比、其它单位或表达式",
        )
    })?;
    if parsed.len() != 1 {
        return Err(error(field, "虚线长度或偏移必须是单个数值"));
    }
    // f64 解析也可能将非零十进制静默下溢为 ±0；此时不能按全零实线解释。
    if parsed[0] == 0.0
        && text
            .split(['e', 'E'])
            .next()
            .unwrap_or("")
            .bytes()
            .any(|b| matches!(b, b'1'..=b'9'))
    {
        return Err(error(field, "非零虚线数值发生下溢，不能替换为零或实线"));
    }
    Ok(parsed[0])
}

pub(super) fn parse_array(source: &str) -> Result<Option<Vec<f64>>, SceneError> {
    if source == "inherit" {
        return Ok(None);
    }
    if source == "none" {
        return Ok(Some(Vec::new()));
    }
    let field = "stroke-dasharray";
    if source.is_empty()
        || source
            .bytes()
            .any(|b| b.is_ascii_control() && !matches!(b, b'\t' | b'\r' | b'\n'))
    {
        return Err(error(
            field,
            "虚线数列不能为空或包含非法分隔符；实线请使用 none",
        ));
    }
    let mut array = Vec::new();
    for group in source.split(',') {
        let mut count = 0;
        for token in group.split_ascii_whitespace() {
            if array.len() >= MAX_DASH_ENTRIES {
                let mut error = super::validate::limit("虚线数列项数");
                error.field = Some(field.into());
                return Err(error);
            }
            array.push(scalar(token, field)?);
            count += 1;
        }
        if count == 0 {
            return Err(error(field, "虚线数列不允许空项、连续或首尾逗号"));
        }
    }
    let style = SceneStyle {
        stroke_dasharray: Some(array),
        ..SceneStyle::default()
    };
    validate_style(&style)?;
    Ok(style.stroke_dasharray)
}

pub(super) fn parse_offset(source: &str) -> Result<Option<f64>, SceneError> {
    if source == "inherit" {
        return Ok(None);
    }
    let value = scalar(source, "stroke-dashoffset")?;
    let style = SceneStyle {
        stroke_dashoffset: Some(value),
        ..SceneStyle::default()
    };
    validate_style(&style)?;
    Ok(Some(value))
}

fn explicit(style: &SceneStyle) -> bool {
    style.stroke_dasharray.is_some() || style.stroke_dashoffset.is_some()
}

pub(crate) fn uses_dash(scene: &MapScene) -> bool {
    scene.nodes.values().any(|node| {
        explicit(&node.style) || matches!(&node.geometry, SceneGeometry::Text { runs, .. } if runs.iter().any(|r| explicit(&r.style)))
    })
}

pub(super) fn validate_feature(scene: &MapScene) -> Result<(), SceneError> {
    if uses_dash(scene)
        && !scene
            .extra
            .get("required_features")
            .and_then(Value::as_array)
            .is_some_and(|features| {
                features
                    .iter()
                    .any(|v| v.as_str() == Some(SCENE_DASH_FEATURE))
            })
    {
        return Err(SceneError::new("SCENE_FEATURE", "虚线样式缺少 presentation.vector_stroke_dash.v1 声明，只能只读保留；请通过显式场景编辑创建虚线"));
    }
    Ok(())
}

pub(super) fn declare_feature(object: &mut Map<String, Value>) -> Result<(), SceneError> {
    let features = object
        .entry("required_features")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| SceneError::new("SCENE_FEATURE", "required_features 必须为数组"))?;
    if !features
        .iter()
        .any(|v| v.as_str() == Some(SCENE_DASH_FEATURE))
    {
        features.push(Value::String(SCENE_DASH_FEATURE.into()));
    }
    Ok(())
}

pub(super) fn declare_scene(scene: &mut MapScene) -> Result<(), SceneError> {
    if uses_dash(scene) {
        declare_feature(&mut scene.extra)?;
    }
    Ok(())
}

pub(super) fn append_svg(out: &mut String, style: &SceneStyle) {
    if let Some(array) = &style.stroke_dasharray {
        out.push_str(" stroke-dasharray=\"");
        if array.is_empty() {
            out.push_str("none");
        } else {
            for (index, value) in array.iter().enumerate() {
                if index != 0 {
                    out.push(' ');
                }
                // SVG 与后端对 -0 的词法处理不同，数值零统一输出为 0。
                let value = if *value == 0.0 { 0.0 } else { *value };
                let _ = write!(out, "{value}");
            }
        }
        out.push('"');
    }
    if let Some(offset) = style.stroke_dashoffset {
        let _ = write!(out, " stroke-dashoffset=\"{offset}\"");
    }
}

/// 在 typed 反序列化之前保护旧未知样式字节；通用写入/删除也必须遵守能力门。
pub(crate) fn document_read_only(map: &Map<String, Value>, scene: &Value) -> bool {
    fn raw_style(value: Option<&Value>) -> bool {
        value.is_some_and(|style| {
            ["stroke_dasharray", "stroke_dashoffset"]
                .iter()
                .any(|field| style.get(*field).is_some_and(|value| !value.is_null()))
        })
    }
    let explicit =
        scene
            .get("nodes")
            .and_then(Value::as_object)
            .is_some_and(|nodes| {
                nodes.values().any(|node| {
                    raw_style(node.get("style"))
                        || node.get("geometry").is_some_and(|geometry| {
                            geometry.get("kind").and_then(Value::as_str) == Some("text")
                                && geometry.get("runs").and_then(Value::as_array).is_some_and(
                                    |runs| runs.iter().any(|run| raw_style(run.get("style"))),
                                )
                        })
                })
            });
    let declared = |value: Option<&Value>| {
        value.and_then(Value::as_array).is_some_and(|features| {
            features
                .iter()
                .any(|value| value.as_str() == Some(SCENE_DASH_FEATURE))
        })
    };
    explicit
        && (!declared(map.get("required_features")) || !declared(scene.get("required_features")))
}

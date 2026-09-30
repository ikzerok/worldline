//! 作者明确设定的平面比例；计算不依赖屏幕或栅格像素。
use super::MapCanvas;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const MEASUREMENT_FEATURE: &str = "presentation.measurement.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapMeasurement {
    pub points: [[f64; 2]; 2],
    pub distance: f64,
    pub unit: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub fn validate_measurement(canvas: &MapCanvas, value: &MapMeasurement) -> Result<(), String> {
    logical_length(canvas, value.points)?;
    if !value.distance.is_finite() || value.distance <= 0.0 {
        return Err("已知距离必须是有限正数".into());
    }
    if value.unit.trim().is_empty()
        || value.unit.chars().count() > 24
        || value.unit.chars().any(char::is_control)
    {
        return Err("单位须为 1–24 字符的非空纯文本，不能含控制字符".into());
    }
    Ok(())
}

pub fn measurement_distance(
    canvas: &MapCanvas,
    calibration: &MapMeasurement,
    points: [[f64; 2]; 2],
) -> Result<f64, String> {
    validate_measurement(canvas, calibration)?;
    let reference = logical_length(canvas, calibration.points)?;
    let measured = logical_length(canvas, points)?;
    // 某种乘除次序的中间值可溢出或下溢，即使最终数学结果仍可表示。
    // 仅在主次序失败时试等价次序；不能把中间失败误报为结果越界。
    for result in [
        (measured / reference) * calibration.distance,
        (calibration.distance / reference) * measured,
        (measured * calibration.distance) / reference,
    ] {
        if result.is_finite() && result > 0.0 {
            return Ok(result);
        }
    }
    Err("测量结果超出可表示范围，请调整校准距离或参照点".into())
}

fn logical_length(canvas: &MapCanvas, points: [[f64; 2]; 2]) -> Result<f64, String> {
    if canvas.width == 0
        || canvas.height == 0
        || canvas.width > i32::MAX as u32
        || canvas.height > i32::MAX as u32
    {
        return Err("地图逻辑宽高必须在 1–2147483647 内".into());
    }
    if points
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err("测量点必须是地图范围内的有限坐标".into());
    }
    let length = ((points[1][0] - points[0][0]) * f64::from(canvas.width))
        .hypot((points[1][1] - points[0][1]) * f64::from(canvas.height));
    if !length.is_finite() || length <= 0.0 {
        return Err("请选择两个不同的地图点".into());
    }
    Ok(length)
}

pub(super) fn parse_measurement(
    value: Option<&Value>,
    canvas: Option<&MapCanvas>,
) -> Result<Option<MapMeasurement>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed: MapMeasurement = serde_json::from_value(value.clone())
        .map_err(|_| "地图 measurement 格式无效".to_string())?;
    let canvas = canvas.ok_or("校准需要有效的地图逻辑宽高")?;
    validate_measurement(canvas, &parsed)?;
    Ok(Some(parsed))
}

use super::*;
use crate::presentation::{validate_measurement, MapCanvas, MapMeasurement, MEASUREMENT_FEATURE};

pub(super) fn apply_measurement(
    object: &mut Map<String, Value>,
    measurement: &MapMeasurement,
) -> Result<(), EditError> {
    let invalid = |message: String| EditError::InvalidSchema { message };
    let canvas = object
        .get("canvas")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("地图 canvas 无效".into()))?;
    if canvas.get("unit").and_then(Value::as_str) != Some("normalized") {
        return Err(invalid("地图 canvas.unit 必须是 normalized".into()));
    }
    let dimension = |key| {
        canvas
            .get(key)
            .and_then(Value::as_u64)
            .filter(|v| (1..=i32::MAX as u64).contains(v))
            .map(|v| v as u32)
            .ok_or_else(|| invalid("地图逻辑宽高无效".into()))
    };
    let canvas = MapCanvas {
        width: dimension("width")?,
        height: dimension("height")?,
        unit: "normalized".into(),
        extra: Map::new(),
    };
    validate_measurement(&canvas, measurement).map_err(invalid)?;
    let fields = object
        .entry("measurement")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid("地图 measurement 必须是对象".into()))?;
    let points = serde_json::to_value(measurement.points).map_err(|e| invalid(e.to_string()))?;
    let unchanged = serde_json::from_value::<MapMeasurement>(Value::Object(fields.clone()))
        .is_ok_and(|old| {
            old.points == measurement.points
                && old.distance == measurement.distance
                && old.unit == measurement.unit
        });
    if unchanged {
        return Err(invalid("校准未改变".into()));
    }
    fields.insert("points".into(), points);
    fields.insert("distance".into(), Value::from(measurement.distance));
    fields.insert("unit".into(), Value::String(measurement.unit.clone()));
    let features = object
        .entry("required_features")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| invalid("required_features 必须是数组".into()))?;
    if !features
        .iter()
        .any(|v| v.as_str() == Some(MEASUREMENT_FEATURE))
    {
        features.push(Value::String(MEASUREMENT_FEATURE.into()));
    }
    Ok(())
}

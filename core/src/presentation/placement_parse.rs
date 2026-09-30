use super::{
    extras, finite_unit, map_error, map_warning, parse_extensions, parse_navigation,
    parse_optional_object, parse_optional_string, required_object, required_string, MapGeometry,
    MapLayer, MapPlacement,
};
use crate::catalog::{Catalog, TargetRef};
use crate::diagnostic::Diagnostic;
use crate::workspace_documents::valid_id;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn parse_placements(
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
        Some("text") => {
            let position = parse_point(object.get("position"), file, path, diagnostics)?;
            let text = object.get("text").and_then(Value::as_str);
            let font_size = object.get("font_size").and_then(Value::as_f64);
            let color = object.get("color").and_then(Value::as_str);
            match (text, font_size, color) {
                (Some(text), Some(font_size), Some(color))
                    if super::valid_map_text(text, font_size, color) =>
                {
                    Some(MapGeometry::Text {
                        position,
                        text: text.into(),
                        font_size,
                        color: color.into(),
                    })
                }
                _ => {
                    diagnostics.push(map_error(
                        file,
                        "MAP006",
                        format!("{path} 文字标签内容、字号或颜色无效"),
                    ));
                    None
                }
            }
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

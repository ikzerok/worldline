use super::{Affine, MapScene, SceneError, WORLD_LIMIT};

pub(super) fn number(value: f64) -> Result<f64, SceneError> {
    if value.is_finite() && value.abs() <= WORLD_LIMIT {
        Ok(value)
    } else {
        Err(SceneError::new(
            "SCENE_NUMERIC",
            "数值非有限或超出世界数值预算",
        ))
    }
}

pub(super) fn checked_affine(matrix: Affine) -> Result<Affine, SceneError> {
    for value in matrix.0 {
        number(value)?;
    }
    Ok(matrix)
}

pub fn view_box_transform(
    view_box: [f64; 4],
    width: f64,
    height: f64,
    aspect: &str,
) -> Result<Affine, SceneError> {
    for value in view_box.into_iter().chain([width, height]) {
        number(value)?;
    }
    if view_box[2] <= 0.0 || view_box[3] <= 0.0 || width <= 0.0 || height <= 0.0 {
        return Err(SceneError::new(
            "SCENE_GEOMETRY",
            "viewBox 与 viewport 的宽高必须为正",
        ));
    }
    let parts: Vec<_> = aspect.split_ascii_whitespace().collect();
    let align = parts.first().copied().unwrap_or("xMidYMid");
    let mode = parts.get(1).copied().unwrap_or("meet");
    if parts.len() > 2 || !matches!(mode, "meet" | "slice") {
        return Err(SceneError::new(
            "SCENE_SVG_PROFILE",
            "preserveAspectRatio 格式无效",
        ));
    }
    let sx = width / view_box[2];
    let sy = height / view_box[3];
    if align == "none" {
        return checked_affine(Affine([
            sx,
            0.0,
            0.0,
            sy,
            -view_box[0] * sx,
            -view_box[1] * sy,
        ]));
    }
    let (ax, ay) = match align {
        "xMinYMin" => (0.0, 0.0),
        "xMidYMin" => (0.5, 0.0),
        "xMaxYMin" => (1.0, 0.0),
        "xMinYMid" => (0.0, 0.5),
        "xMidYMid" => (0.5, 0.5),
        "xMaxYMid" => (1.0, 0.5),
        "xMinYMax" => (0.0, 1.0),
        "xMidYMax" => (0.5, 1.0),
        "xMaxYMax" => (1.0, 1.0),
        _ => {
            return Err(SceneError::new(
                "SCENE_SVG_PROFILE",
                "preserveAspectRatio 对齐值无效",
            ))
        }
    };
    let scale = if mode == "slice" {
        sx.max(sy)
    } else {
        sx.min(sy)
    };
    checked_affine(Affine([
        scale,
        0.0,
        0.0,
        scale,
        (width - view_box[2] * scale) * ax - view_box[0] * scale,
        (height - view_box[3] * scale) * ay - view_box[1] * scale,
    ]))
}

/// fit 源 viewport 到目标 scene viewBox；源内部 viewBox/aspect 语义仍保留。
pub fn import_view_transform(
    source_scene: &MapScene,
    width: f64,
    height: f64,
    target_scene: &MapScene,
) -> Result<Affine, SceneError> {
    let source = view_box_transform(
        source_scene.view_box,
        width,
        height,
        &source_scene.preserve_aspect_ratio,
    )?;
    let [x, y, w, h] = target_scene.view_box;
    let fit = view_box_transform([0.0, 0.0, width, height], w, h, "xMidYMid meet")?;
    checked_affine(Affine([1.0, 0.0, 0.0, 1.0, x, y]).then(fit).then(source))
}

pub(super) fn root_clip(scene: &MapScene, width: f64, height: f64) -> Result<[f64; 4], SceneError> {
    let inverse = view_box_transform(scene.view_box, width, height, &scene.preserve_aspect_ratio)?
        .inverse()
        .ok_or_else(|| SceneError::new("SCENE_NUMERIC", "viewport 逆矩阵超出预算"))?;
    let a = inverse.point([0.0, 0.0]);
    let b = inverse.point([width, height]);
    for value in a.into_iter().chain(b) {
        number(value)?;
    }
    Ok([a[0], a[1], b[0] - a[0], b[1] - a[1]])
}

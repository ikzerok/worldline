use super::{svg_path::numbers, Affine, SceneError, WORLD_LIMIT};

fn profile(message: &str) -> SceneError {
    SceneError::new("SCENE_SVG_PROFILE", message)
}

fn checked(transform: Affine) -> Result<Affine, SceneError> {
    if transform
        .0
        .iter()
        .all(|value| value.is_finite() && value.abs() <= WORLD_LIMIT)
    {
        Ok(transform)
    } else {
        Err(SceneError::new(
            "SCENE_NUMERIC",
            "SVG 变换或累计矩阵超过有限世界坐标预算",
        ))
    }
}

fn translation(x: f64, y: f64) -> Affine {
    Affine([1.0, 0.0, 0.0, 1.0, x, y])
}

fn transform(name: &str, values: &[f64]) -> Result<Affine, SceneError> {
    let result = match (name, values) {
        ("matrix", &[a, b, c, d, e, f]) => Affine([a, b, c, d, e, f]),
        ("translate", &[x]) => translation(x, 0.0),
        ("translate", &[x, y]) => translation(x, y),
        ("scale", &[scale]) => Affine([scale, 0.0, 0.0, scale, 0.0, 0.0]),
        ("scale", &[x, y]) => Affine([x, 0.0, 0.0, y, 0.0, 0.0]),
        ("rotate", &[angle]) | ("rotate", &[angle, _, _]) => {
            let (sin, cos) = (angle % 360.0).to_radians().sin_cos();
            let rotation = Affine([cos, sin, -sin, cos, 0.0, 0.0]);
            if values.len() == 3 {
                let (x, y) = (values[1], values[2]);
                checked(translation(x, y).then(rotation))?.then(translation(-x, -y))
            } else {
                rotation
            }
        }
        ("skewX", &[angle]) => {
            Affine([1.0, 0.0, (angle % 180.0).to_radians().tan(), 1.0, 0.0, 0.0])
        }
        ("skewY", &[angle]) => {
            Affine([1.0, (angle % 180.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0])
        }
        ("matrix" | "translate" | "scale" | "rotate" | "skewX" | "skewY", _) => {
            return Err(profile("SVG 变换参数个数无效"));
        }
        _ => return Err(profile("SVG 包含不支持的变换函数")),
    };
    checked(result)
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// SVG column-vector transform lists multiply in source order: self × rhs.
pub(super) fn parse_transform(source: &str) -> Result<Affine, SceneError> {
    let bytes = source.as_bytes();
    let mut position = 0;
    let mut count = 0;
    let mut result = Affine::IDENTITY;
    loop {
        let separator_start = position;
        while bytes.get(position).is_some_and(|byte| whitespace(*byte)) {
            position += 1;
        }
        let mut separated = position != separator_start;
        if position == bytes.len() {
            return Ok(result);
        }
        if bytes[position] == b',' {
            if count == 0 {
                return Err(profile("SVG 变换列表不能以逗号开始"));
            }
            position += 1;
            separated = true;
            while bytes.get(position).is_some_and(|byte| whitespace(*byte)) {
                position += 1;
            }
            if position == bytes.len() {
                return Err(profile("SVG 变换列表不能以逗号结束"));
            }
        }
        if count > 0 && !separated {
            return Err(profile("SVG 变换函数之间需要空白或逗号分隔"));
        }
        if count >= 64 {
            return Err(SceneError::new("SCENE_LIMIT", "SVG 变换列表最多允许 64 项"));
        }
        let start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_alphabetic) {
            position += 1;
        }
        if start == position {
            return Err(profile("SVG 变换函数名无效"));
        }
        let name = &source[start..position];
        while bytes.get(position).is_some_and(|byte| whitespace(*byte)) {
            position += 1;
        }
        if bytes.get(position) != Some(&b'(') {
            return Err(profile("SVG 变换函数缺少左括号"));
        }
        position += 1;
        let arguments = position;
        while bytes.get(position).is_some_and(|byte| *byte != b')') {
            position += 1;
        }
        if position == bytes.len() {
            return Err(profile("SVG 变换函数缺少右括号"));
        }
        let values = numbers(&source[arguments..position])?;
        result = checked(result.then(transform(name, &values)?))?;
        position += 1;
        count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: Affine, expected: [f64; 6]) {
        for (actual, expected) in actual.0.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
        }
    }

    #[test]
    fn identity_matrix_translation_and_arbitrary_scale() {
        assert_eq!(parse_transform("").unwrap(), Affine::IDENTITY);
        assert_eq!(parse_transform(" \n\t").unwrap(), Affine::IDENTITY);
        close(
            parse_transform("matrix(1 2 3 4 5 6)").unwrap(),
            [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
        );
        close(
            parse_transform("translate(5)").unwrap(),
            [1.0, 0.0, 0.0, 1.0, 5.0, 0.0],
        );
        close(
            parse_transform("translate(-5, 6)").unwrap(),
            [1.0, 0.0, 0.0, 1.0, -5.0, 6.0],
        );
        close(
            parse_transform("scale(-2)").unwrap(),
            [-2.0, 0.0, 0.0, -2.0, 0.0, 0.0],
        );
        close(
            parse_transform("scale(-2,3)").unwrap(),
            [-2.0, 0.0, 0.0, 3.0, 0.0, 0.0],
        );
        close(
            parse_transform("scale(0,1)").unwrap(),
            [0.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        );
    }

    #[test]
    fn rotation_with_center_and_both_skews_are_full_affines() {
        close(
            parse_transform("rotate(90)").unwrap(),
            [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
        );
        close(
            parse_transform("rotate(90,2,3)").unwrap(),
            [0.0, 1.0, -1.0, 0.0, 5.0, 1.0],
        );
        close(
            parse_transform("rotate(-450)").unwrap(),
            [0.0, -1.0, 1.0, 0.0, 0.0, 0.0],
        );
        close(
            parse_transform("skewX(45)").unwrap(),
            [1.0, 0.0, 1.0, 1.0, 0.0, 0.0],
        );
        close(
            parse_transform("skewY(-45)").unwrap(),
            [1.0, -1.0, 0.0, 1.0, 0.0, 0.0],
        );
        let center = parse_transform("rotate(37 17 -23)")
            .unwrap()
            .point([17.0, -23.0]);
        assert!((center[0] - 17.0).abs() < 1e-10);
        assert!((center[1] + 23.0).abs() < 1e-10);
    }

    #[test]
    fn lists_multiply_self_by_rhs_in_svg_order() {
        close(
            parse_transform("translate(10 20) scale(2 3)").unwrap(),
            [2.0, 0.0, 0.0, 3.0, 10.0, 20.0],
        );
        close(
            parse_transform("scale(2 3),translate(10 20)").unwrap(),
            [2.0, 0.0, 0.0, 3.0, 20.0, 60.0],
        );
        close(
            parse_transform("translate(2 3) rotate(90) scale(-2 4)").unwrap(),
            [0.0, -2.0, -4.0, 0.0, 2.0, 3.0],
        );
        close(
            parse_transform("matrix(1,2,3,4,5,6) skewX(45)").unwrap(),
            [1.0, 2.0, 4.0, 6.0, 5.0, 6.0],
        );
    }

    #[test]
    fn malformed_functions_units_and_wrong_arities_are_rejected() {
        for source in [
            "none",
            "translate",
            "translate(1",
            "translate 1)",
            "translate((1))",
            "translate()",
            "translate(1 2 3)",
            "matrix(1 2 3 4 5)",
            "matrix(1 2 3 4 5 6 7)",
            "scale()",
            "scale(1 2 3)",
            "rotate(1 2)",
            "rotate(1 2 3 4)",
            "skewX(1 2)",
            "skewY()",
            "Rotate(90)",
            "skewx(45)",
            "translate(1px)",
            "rotate(90deg)",
            "scale(1,,2)",
            "scale(1,)",
            "scale(,1)",
            "scale(1-2)",
            ",scale(1)",
            "scale(1),",
            "scale(1),,scale(2)",
            "scale(1);scale(2)",
            "scale(1)scale(2)",
            "scale(1))",
            "scale(NaN)",
            "scale(1e)",
            "scale(1)\u{000b}scale(2)",
        ] {
            assert_eq!(
                parse_transform(source).unwrap_err().code,
                "SCENE_SVG_PROFILE",
                "{source}"
            );
        }
    }

    #[test]
    fn each_operation_and_accumulated_matrix_obey_numeric_budget() {
        for source in [
            "scale(1e309)",
            "translate(1000000001)",
            "scale(1e9) scale(2)",
            "translate(1e9) translate(1)",
            "rotate(180 1e9 0)",
            "skewX(90)",
            "skewY(-90)",
            "matrix(1 1 1 1 0 0) matrix(1e9 1e9 1e9 1e9 0 0)",
        ] {
            assert_eq!(
                parse_transform(source).unwrap_err().code,
                "SCENE_NUMERIC",
                "{source}"
            );
        }
        close(
            parse_transform("translate(1e9 -1e9)").unwrap(),
            [1.0, 0.0, 0.0, 1.0, 1e9, -1e9],
        );
        // An unsafe intermediate may not be hidden by a later small scale.
        assert_eq!(
            parse_transform("scale(1e9) scale(2) scale(.5)")
                .unwrap_err()
                .code,
            "SCENE_NUMERIC"
        );
    }

    #[test]
    fn transform_count_is_capped_even_for_identity_operations() {
        assert!(parse_transform(&"scale(1) ".repeat(64)).is_ok());
        assert_eq!(
            parse_transform(&"scale(1) ".repeat(65)).unwrap_err().code,
            "SCENE_LIMIT"
        );
    }
}

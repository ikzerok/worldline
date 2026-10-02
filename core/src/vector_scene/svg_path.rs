use super::{PathSegment, SceneError, SceneLimits, WORLD_LIMIT};

fn profile(message: &str) -> SceneError {
    SceneError::new("SCENE_SVG_PROFILE", message)
}

fn bounded(value: f64) -> Result<f64, SceneError> {
    if value.is_finite() && value.abs() <= WORLD_LIMIT {
        Ok(value)
    } else {
        Err(SceneError::new(
            "SCENE_NUMERIC",
            "SVG 数值必须有限且绝对值不超过世界坐标预算",
        ))
    }
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

struct Cursor<'a> {
    source: &'a str,
    position: usize,
    // A command starts a new argument list; commas cannot precede its first value.
    comma_allowed: bool,
}

impl<'a> Cursor<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
            comma_allowed: false,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.position).copied()
    }

    fn space(&mut self) -> bool {
        let start = self.position;
        while self.peek().is_some_and(whitespace) {
            self.position += 1;
        }
        self.position != start
    }

    fn separator(&mut self) -> Result<(), SceneError> {
        self.space();
        if self.peek() == Some(b',') {
            if !self.comma_allowed {
                return Err(profile("SVG 参数列表不能以逗号开始"));
            }
            self.position += 1;
            self.space();
        }
        Ok(())
    }

    fn number(&mut self) -> Result<f64, SceneError> {
        self.separator()?;
        let start = self.position;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.position += 1;
        }
        let mut digits = 0;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
            digits += 1;
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(profile("SVG 参数缺失或不是合法十进制数"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            let exponent = self.position;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if exponent == self.position {
                return Err(profile("SVG 数值指数缺少数字"));
            }
        }
        self.comma_allowed = true;
        let value = self.source[start..self.position]
            .parse::<f64>()
            .map_err(|_| profile("SVG 数值格式无效"))?;
        bounded(value)
    }

    fn flag(&mut self) -> Result<bool, SceneError> {
        self.separator()?;
        let value = match self.peek() {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return Err(profile("SVG 圆弧标志必须是单个 0 或 1")),
        };
        self.position += 1;
        self.comma_allowed = true;
        Ok(value)
    }
}

/// Parse an SVG comma/whitespace-separated number list, without units or suffixes.
pub(super) fn numbers(source: &str) -> Result<Vec<f64>, SceneError> {
    let mut cursor = Cursor::new(source);
    let mut result = Vec::new();
    cursor.space();
    while cursor.peek().is_some() {
        result.push(cursor.number()?);
        let separated = cursor.space();
        if cursor.peek().is_some() && cursor.peek() != Some(b',') && !separated {
            return Err(profile("SVG 数值列表需要空白或逗号分隔"));
        }
    }
    Ok(result)
}

struct PathParser<'a> {
    cursor: Cursor<'a>,
    current: [f64; 2],
    start: [f64; 2],
    cubic_control: Option<[f64; 2]>,
    quadratic_control: Option<[f64; 2]>,
}

impl PathParser<'_> {
    fn point(&mut self, relative: bool) -> Result<[f64; 2], SceneError> {
        let x = self.cursor.number()?;
        let y = self.cursor.number()?;
        if relative {
            Ok([bounded(self.current[0] + x)?, bounded(self.current[1] + y)?])
        } else {
            Ok([x, y])
        }
    }

    fn reflected(&self, control: Option<[f64; 2]>) -> Result<[f64; 2], SceneError> {
        match control {
            Some(control) => Ok([
                bounded(2.0 * self.current[0] - control[0])?,
                bounded(2.0 * self.current[1] - control[1])?,
            ]),
            None => Ok(self.current),
        }
    }

    fn segment(&mut self, command: u8) -> Result<PathSegment, SceneError> {
        let relative = command.is_ascii_lowercase();
        let segment = match command.to_ascii_uppercase() {
            b'M' => PathSegment::Move {
                to: self.point(relative)?,
            },
            b'L' => PathSegment::Line {
                to: self.point(relative)?,
            },
            b'H' | b'V' => {
                let axis = usize::from(command.eq_ignore_ascii_case(&b'V'));
                let value = self.cursor.number()?;
                let mut to = self.current;
                to[axis] = bounded(value + if relative { to[axis] } else { 0.0 })?;
                PathSegment::Line { to }
            }
            b'C' => PathSegment::Cubic {
                control1: self.point(relative)?,
                control2: self.point(relative)?,
                to: self.point(relative)?,
            },
            b'S' => PathSegment::Cubic {
                control1: self.reflected(self.cubic_control)?,
                control2: self.point(relative)?,
                to: self.point(relative)?,
            },
            b'Q' => PathSegment::Quadratic {
                control: self.point(relative)?,
                to: self.point(relative)?,
            },
            b'T' => PathSegment::Quadratic {
                control: self.reflected(self.quadratic_control)?,
                to: self.point(relative)?,
            },
            b'A' => {
                let rx = self.cursor.number()?;
                let ry = self.cursor.number()?;
                if rx < 0.0 || ry < 0.0 {
                    return Err(profile("SVG 圆弧半径不能为负数"));
                }
                let rotation = self.cursor.number()?;
                // Unlike flags and endpoint coordinates, rotation must be separated
                // from the first flag; its numeric token is otherwise greedy.
                if !self.cursor.space() && self.cursor.peek() != Some(b',') {
                    return Err(profile("SVG 圆弧旋转角与标志之间缺少分隔符"));
                }
                let large_arc = self.cursor.flag()?;
                let sweep = self.cursor.flag()?;
                let to = self.point(relative)?;
                PathSegment::Arc {
                    rx,
                    ry,
                    rotation,
                    large_arc,
                    sweep,
                    to,
                }
            }
            b'Z' => PathSegment::Close,
            _ => return Err(profile("SVG 路径包含不支持的命令")),
        };
        self.cubic_control = None;
        self.quadratic_control = None;
        match &segment {
            PathSegment::Move { to } => {
                self.start = *to;
                self.current = *to;
            }
            PathSegment::Line { to } | PathSegment::Arc { to, .. } => self.current = *to,
            PathSegment::Cubic { control2, to, .. } => {
                self.cubic_control = Some(*control2);
                self.current = *to;
            }
            PathSegment::Quadratic { control, to } => {
                self.quadratic_control = Some(*control);
                self.current = *to;
            }
            PathSegment::Close => self.current = self.start,
        }
        Ok(segment)
    }
}

/// Preserve typed curves while expanding shorthand, relative coordinates and repeats.
pub(super) fn parse_path(
    source: &str,
    max_segments: usize,
) -> Result<Vec<PathSegment>, SceneError> {
    let max_segments = max_segments.min(SceneLimits::default().max_segments);
    let mut parser = PathParser {
        cursor: Cursor::new(source),
        current: [0.0, 0.0],
        start: [0.0, 0.0],
        cubic_control: None,
        quadratic_control: None,
    };
    let mut result = Vec::new();
    let mut repeat = None;
    loop {
        parser.cursor.space();
        let Some(byte) = parser.cursor.peek() else {
            return Ok(result);
        };
        let command = if byte.is_ascii_alphabetic() {
            parser.cursor.position += 1;
            parser.cursor.comma_allowed = false;
            if !b"MmLlHhVvCcSsQqTtAaZz".contains(&byte) {
                return Err(profile("SVG 路径包含不支持的命令"));
            }
            byte
        } else {
            repeat.ok_or_else(|| profile("SVG 路径缺少命令或在关闭后重复了参数"))?
        };
        if result.is_empty() && !command.eq_ignore_ascii_case(&b'M') {
            return Err(profile("SVG 路径必须以 moveto 命令开始"));
        }
        if result.len() >= max_segments {
            return Err(SceneError::new("SCENE_LIMIT", "SVG 路径段数超过预算"));
        }
        result.push(parser.segment(command)?);
        repeat = match command {
            b'M' => Some(b'L'),
            b'm' => Some(b'l'),
            b'Z' | b'z' => None,
            _ => Some(command),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(source: &str) -> Vec<PathSegment> {
        parse_path(source, 100).unwrap()
    }

    fn m(to: [f64; 2]) -> PathSegment {
        PathSegment::Move { to }
    }
    fn l(to: [f64; 2]) -> PathSegment {
        PathSegment::Line { to }
    }
    fn c(control1: [f64; 2], control2: [f64; 2], to: [f64; 2]) -> PathSegment {
        PathSegment::Cubic {
            control1,
            control2,
            to,
        }
    }
    fn q(control: [f64; 2], to: [f64; 2]) -> PathSegment {
        PathSegment::Quadratic { control, to }
    }
    fn a(radii_rotation: [f64; 3], flags: [bool; 2], to: [f64; 2]) -> PathSegment {
        PathSegment::Arc {
            rx: radii_rotation[0],
            ry: radii_rotation[1],
            rotation: radii_rotation[2],
            large_arc: flags[0],
            sweep: flags[1],
            to,
        }
    }

    #[test]
    fn all_absolute_commands_preserve_curves_and_expand_shorthand() {
        let segments = path(
            "M1 2 L3 4 H5 V6 C7 8 9 10 11 12 S13 14 15 16 Q17 18 19 20 T21 22 A2 3 45 1 0 23 24 Z",
        );
        assert_eq!(
            segments,
            vec![
                m([1.0, 2.0]),
                l([3.0, 4.0]),
                l([5.0, 4.0]),
                l([5.0, 6.0]),
                c([7.0, 8.0], [9.0, 10.0], [11.0, 12.0]),
                c([13.0, 14.0], [13.0, 14.0], [15.0, 16.0]),
                q([17.0, 18.0], [19.0, 20.0]),
                q([21.0, 22.0], [21.0, 22.0]),
                a([2.0, 3.0, 45.0], [true, false], [23.0, 24.0]),
                PathSegment::Close,
            ]
        );
    }

    #[test]
    fn relative_commands_and_repeated_move_pairs_use_current_endpoint() {
        let segments =
            path("m1 2 3 4 l1-1 h2 v-3 c1 2 3 4 5 6 s1 2 3 4 q1 2 3 4 t1 2 a2 3 0 01-2-3 z m1 1");
        assert_eq!(
            segments,
            vec![
                m([1.0, 2.0]),
                l([4.0, 6.0]),
                l([5.0, 5.0]),
                l([7.0, 5.0]),
                l([7.0, 2.0]),
                c([8.0, 4.0], [10.0, 6.0], [12.0, 8.0]),
                c([14.0, 10.0], [13.0, 10.0], [15.0, 12.0]),
                q([16.0, 14.0], [18.0, 16.0]),
                q([20.0, 18.0], [19.0, 18.0]),
                a([2.0, 3.0, 0.0], [false, true], [17.0, 15.0]),
                PathSegment::Close,
                m([2.0, 3.0]),
            ]
        );
    }

    #[test]
    fn repeated_command_arguments_and_multiple_subpaths() {
        let segments = path("M0 0 1 1 L2 2 3 3 H4 5 V6 7 C1 2 3 4 5 6 7 8 9 10 11 12 S1 2 3 4 5 6 7 8 Q1 2 3 4 5 6 7 8 T9 10 11 12 A1 2 0 0 1 13 14 3 4 5 1 0 15 16 Z M20 21z");
        assert_eq!(segments.len(), 21);
        assert_eq!(segments[5], l([5.0, 3.0]));
        assert_eq!(segments[7], l([5.0, 7.0]));
        assert_eq!(segments[11], c([5.0, 6.0], [5.0, 6.0], [7.0, 8.0]));
        assert_eq!(segments[19], m([20.0, 21.0]));
        assert_eq!(segments[20], PathSegment::Close);
    }

    #[test]
    fn repeated_relative_groups_match_explicit_commands() {
        for (implicit, explicit) in [
            ("l1 2 3 4", "l1 2l3 4"),
            ("h1 2 v3 4", "h1h2v3v4"),
            ("c1 2 3 4 5 6 7 8 9 10 11 12", "c1 2 3 4 5 6c7 8 9 10 11 12"),
            ("s1 2 3 4 5 6 7 8", "s1 2 3 4s5 6 7 8"),
            ("q1 2 3 4 5 6 7 8", "q1 2 3 4q5 6 7 8"),
            ("t1 2 3 4", "t1 2t3 4"),
            ("a1 2 3 01 4 5 6 7 8 10 9 10", "a1 2 3 01 4 5a6 7 8 10 9 10"),
        ] {
            assert_eq!(
                path(&format!("M10 20{implicit}")),
                path(&format!("M10 20{explicit}"))
            );
        }
    }

    #[test]
    fn smooth_controls_reset_across_other_commands_and_close() {
        let segments =
            path("M0 0 C1 1 2 2 3 3 L4 4 S5 5 6 6 Q7 7 8 8 H9 T10 10 Z S1 1 2 2 M3 3 T4 4");
        assert_eq!(segments[3], c([4.0, 4.0], [5.0, 5.0], [6.0, 6.0]));
        assert_eq!(segments[6], q([9.0, 8.0], [10.0, 10.0]));
        assert_eq!(segments[8], c([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]));
        assert_eq!(segments[10], q([3.0, 3.0], [4.0, 4.0]));
    }

    #[test]
    fn compact_flags_and_decimal_or_signed_endpoints_are_unambiguous() {
        for (arguments, expected) in [
            ("0110 20", [10.0, 20.0]),
            ("01.5.6", [0.5, 0.6]),
            ("01-10-20", [-10.0, -20.0]),
            ("0,1,10,20", [10.0, 20.0]),
        ] {
            let segments = path(&format!("M0 0A1 2 30 {arguments}"));
            assert_eq!(segments[1], a([1.0, 2.0, 30.0], [false, true], expected));
        }
        for flags in [
            "2 0", "-1 0", "+1 0", "0.0 1", "1e0 1", "0 2", "0 -1", "0 +1",
        ] {
            assert!(
                parse_path(&format!("M0 0A1 2 0 {flags} 3 4"), 100).is_err(),
                "{flags}"
            );
        }
        assert_eq!(
            path("M0 0A0 0 0 00 1 1")[1],
            a([0.0; 3], [false; 2], [1.0; 2])
        );
    }

    #[test]
    fn decimals_exponents_and_svg_whitespace() {
        assert_eq!(
            path(" \tM.5.6\r\nL1e-2-2E+1 "),
            vec![m([0.5, 0.6]), l([0.01, -20.0]),]
        );
        assert_eq!(numbers(" +.5,\t-2E+1 3. ").unwrap(), vec![0.5, -20.0, 3.0]);
        assert!(numbers("").unwrap().is_empty());
        assert!(path(" \r\n ").is_empty());
    }

    #[test]
    fn rejects_incomplete_unknown_and_malformed_commands() {
        for source in [
            "M",
            "M0",
            "L0 0",
            "Z",
            "M0 0L",
            "M0 0C1 2 3 4 5",
            "M0 0S1 2 3",
            "M0 0Q1 2 3",
            "M0 0T1",
            "M0 0A1 2 3 0 1 2",
            "M0 0R1 2",
            "M0 0z1 2",
            "M,0 0",
            "M0,,0",
            "M0 0,",
            "M0 0,L1 1",
            "M0 0Z,",
            "M0 0A-1 2 0 0 1 3 4",
            "M0 0A1 -2 0 0 1 3 4",
            "M0 0L1e 2",
            "M0 0L1e+ 2",
            "M0 0L. 2",
            "MNaN 0",
            "MInf 0",
            "M0 0\u{000b}L1 1",
        ] {
            assert_eq!(
                parse_path(source, 100).unwrap_err().code,
                "SCENE_SVG_PROFILE",
                "{source}"
            );
        }
        for source in [
            ",1", "1,", "1,,2", "1-2", "1.2.3", "1px", "NaN", "1e", "1;2",
        ] {
            assert!(numbers(source).is_err(), "{source}");
        }
    }

    #[test]
    fn rejects_numeric_overflow_in_values_relative_points_and_reflections() {
        for source in [
            "M1e309 0",
            "M1000000001 0",
            "M1e9 0l1 0",
            "M0 -1e9v-1",
            "M0 0C0 0 -1e9 0 1e9 0S0 0 0 0",
            "M0 0Q-1e9 0 1e9 0T0 0",
            "M0 0A1e10 1 0 0 0 1 1",
        ] {
            assert_eq!(
                parse_path(source, 100).unwrap_err().code,
                "SCENE_NUMERIC",
                "{source}"
            );
        }
        assert_eq!(numbers("1e309").unwrap_err().code, "SCENE_NUMERIC");
        assert_eq!(path("M1e9 -1e9")[0], m([1e9, -1e9]));
    }

    #[test]
    fn segment_budget_counts_move_close_and_repeated_arguments() {
        assert!(parse_path("", 0).unwrap().is_empty());
        assert_eq!(parse_path("M0 0", 0).unwrap_err().code, "SCENE_LIMIT");
        assert_eq!(parse_path("M0 0 1 1Z", 2).unwrap_err().code, "SCENE_LIMIT");
        assert_eq!(parse_path("M0 0 1 1Z", 3).unwrap().len(), 3);
        let source = format!("M0 0{}", "L0 0".repeat(SceneLimits::default().max_segments));
        assert_eq!(
            parse_path(&source, usize::MAX).unwrap_err().code,
            "SCENE_LIMIT"
        );
    }
}

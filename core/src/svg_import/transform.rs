//! 有界二维相似变换；拒绝无法用标量描边宽度表达的非等比缩放。
#[derive(Clone, Copy, Debug)]
pub(super) struct Transform(pub [f64; 6]);
impl Transform {
    pub const IDENTITY: Self = Self([1., 0., 0., 1., 0., 0.]);
    pub fn point(self, p: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }
    pub fn then(self, rhs: Self) -> Result<Self, String> {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = rhs.0;
        let out = Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ]);
        if out.0.iter().any(|v| !v.is_finite()) || out.scale() == 0. || !out.scale().is_finite() {
            return Err("SVG 变换溢出或退化".into());
        }
        Ok(out)
    }
    pub fn scale(self) -> f64 {
        self.0[0].hypot(self.0[1])
    }
}
pub(super) fn parse(source: &str) -> Result<Transform, String> {
    let mut rest = source.trim();
    let mut result = Transform::IDENTITY;
    let mut count = 0;
    while !rest.is_empty() {
        count += 1;
        if count > 64 {
            return Err("SVG 变换列表超过 64 项".into());
        }
        let open = rest.find('(').ok_or("SVG 变换缺少左括号")?;
        let name = rest[..open].trim();
        let close = rest[open + 1..]
            .find(')')
            .map(|i| i + open + 1)
            .ok_or("SVG 变换缺少右括号")?;
        let values = super::path::numbers(&rest[open + 1..close])?;
        let t = match (name, values.as_slice()) {
            ("translate", [x]) => Transform([1., 0., 0., 1., *x, 0.]),
            ("translate", [x, y]) => Transform([1., 0., 0., 1., *x, *y]),
            ("scale", [s]) if *s != 0. => Transform([*s, 0., 0., *s, 0., 0.]),
            ("scale", [x, y]) if *x != 0. && x.abs() == y.abs() => {
                Transform([*x, 0., 0., *y, 0., 0.])
            }
            ("scale", _) => return Err(
                "SVG scale 仅支持非零等比缩放；非等比缩放无法保留描边，请先在绘图工具中展开描边"
                    .into(),
            ),
            ("rotate", [angle]) => rotation(*angle),
            ("rotate", [angle, x, y]) => Transform([1., 0., 0., 1., *x, *y])
                .then(rotation(*angle))?
                .then(Transform([1., 0., 0., 1., -*x, -*y]))?,
            ("translate" | "rotate", _) => return Err("SVG 变换参数数量无效".into()),
            _ => {
                return Err(format!(
                    "不支持 SVG 变换：{name}（仅支持 translate、等比 scale、rotate）"
                ))
            }
        };
        result = result.then(t)?;
        rest = rest[close + 1..].trim_start();
        if let Some(tail) = rest.strip_prefix(',') {
            rest = tail.trim_start();
            if rest.is_empty() {
                return Err("SVG 变换列表末尾缺少变换".into());
            }
        }
    }
    // 空属性按 SVG 的恒等变换处理。
    Ok(result)
}
fn rotation(angle: f64) -> Transform {
    let (s, c) = (angle.rem_euclid(360.).to_radians()).sin_cos();
    Transform([c, s, -s, c, 0., 0.])
}

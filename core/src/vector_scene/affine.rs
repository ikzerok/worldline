use super::WORLD_LIMIT;
use serde::{Deserialize, Serialize};

/// SVG matrix(a b c d e f)，列向量坐标；then(rhs) 返回 self × rhs。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Affine(pub [f64; 6]);

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub fn point(self, point: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [
            a * point[0] + c * point[1] + e,
            b * point[0] + d * point[1] + f,
        ]
    }

    pub fn then(self, rhs: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = rhs.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }

    pub fn inverse(self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant == 0.0 {
            return None;
        }
        let result = Self([
            d / determinant,
            -b / determinant,
            -c / determinant,
            a / determinant,
            (c * f - d * e) / determinant,
            (b * e - a * f) / determinant,
        ]);
        result
            .0
            .iter()
            .all(|v| v.is_finite() && v.abs() <= WORLD_LIMIT)
            .then_some(result)
    }
}

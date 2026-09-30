//! 有符号闭区间抽样，保持旧有效非负区间的 RNG 序列。
use super::util::next_rnd;

pub(super) fn integer(rng: &mut u64, lower: f64, upper: f64) -> Result<f64, String> {
    const MAX_SAFE: f64 = 9_007_199_254_740_991.0;
    if !lower.is_finite() || !upper.is_finite() {
        return Err("rnd 的上下界必须是有限数值".into());
    }
    if lower > upper {
        return Err(format!("rnd 的上界({upper})小于下界({lower})"));
    }
    let (lo, hi) = (lower.ceil(), upper.floor());
    if lo > hi {
        return Err("rnd 的闭区间不包含整数".into());
    }
    if lo < -MAX_SAFE || hi > MAX_SAFE {
        return Err("rnd 的整数边界超出安全整数范围".into());
    }
    let (lo, hi) = (lo as i64, hi as i64);
    let span = (i128::from(hi) - i128::from(lo) + 1) as u64;
    let offset = next_rnd(rng) % span;
    Ok((i128::from(lo) + i128::from(offset)) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_intervals_and_invalid_ranges() {
        let mut rng = 31;
        assert_eq!(integer(&mut rng, -2.0, -2.0), Ok(-2.0));
        let mut saw_negative = false;
        for _ in 0..200 {
            let value = integer(&mut rng, -4.0, 4.0).unwrap();
            assert!((-4.0..=4.0).contains(&value));
            saw_negative |= value < 0.0;
        }
        assert!(saw_negative);
        for (lo, hi) in [
            (-1.0, -2.0),
            (0.1, 0.9),
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
            (0.0, 9_007_199_254_740_992.0),
        ] {
            let before = rng;
            assert!(integer(&mut rng, lo, hi).is_err());
            assert_eq!(rng, before);
        }
    }

    #[test]
    fn old_nonnegative_sequences_and_single_point_consumption_are_preserved() {
        for (lo, hi) in [
            (0.0f64, 0.0f64),
            (1.0, 6.0),
            (0.2, 9.8),
            (0.0, 9007199254740991.0),
        ] {
            let mut old = 123;
            let mut new = old;
            for _ in 0..50 {
                let (a, b) = (lo.ceil() as u64, hi.floor() as u64);
                let expected = (a + next_rnd(&mut old) % (b - a + 1)) as f64;
                assert_eq!(integer(&mut new, lo, hi), Ok(expected));
                assert_eq!(old, new);
            }
        }
    }
}

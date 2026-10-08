//! 新检查戳独有的精确u64字符串编码；不改变其它运行或持久DTO。
use serde::{de, Deserializer, Serializer};

pub(super) fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    struct Decimal;
    impl de::Visitor<'_> for Decimal {
        type Value = u64;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("规范十进制u64字符串（0或无前导零的ASCII数字）")
        }
        fn visit_str<E: de::Error>(self, value: &str) -> Result<u64, E> {
            if value.is_empty()
                || value.len() > 20
                || (value.len() > 1 && value.starts_with('0'))
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(E::custom("检查戳必须使用规范十进制u64字符串"));
            }
            value.parse().map_err(|_| E::custom("检查戳超出u64范围"))
        }
    }
    deserializer.deserialize_str(Decimal)
}

#[cfg(test)]
#[path = "stamp_wire_tests.rs"]
mod tests;

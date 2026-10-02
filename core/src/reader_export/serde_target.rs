//! reader 授权 DTO 局部拒绝未知身份字段，不改变其他消费者的 TargetRef 契约。
use crate::catalog::TargetRef;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTarget {
    kind: String,
    id: String,
}

impl From<StrictTarget> for TargetRef {
    fn from(value: StrictTarget) -> Self {
        Self {
            kind: value.kind,
            id: value.id,
        }
    }
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<TargetRef, D::Error> {
    StrictTarget::deserialize(input).map(Into::into)
}

pub(super) fn deserialize_optional<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<Option<TargetRef>, D::Error> {
    Option::<StrictTarget>::deserialize(input).map(|target| target.map(Into::into))
}

pub(super) fn deserialize_vec<'de, D: Deserializer<'de>>(
    input: D,
) -> Result<Vec<TargetRef>, D::Error> {
    Vec::<StrictTarget>::deserialize(input)
        .map(|targets| targets.into_iter().map(Into::into).collect())
}

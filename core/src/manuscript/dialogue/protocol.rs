use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictTarget {
    kind: String,
    id: String,
}
impl From<StrictTarget> for TargetRef {
    fn from(value: StrictTarget) -> Self {
        TargetRef::new(&value.kind, &value.id)
    }
}
pub(super) fn target<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<TargetRef, D::Error> {
    StrictTarget::deserialize(d).map(Into::into)
}
pub(super) fn optional_target<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<TargetRef>, D::Error> {
    Option::<StrictTarget>::deserialize(d).map(|v| v.map(Into::into))
}

pub fn parse_dialogue_edit_request(source: &str) -> Result<DialogueEditRequest> {
    decode(source, MAX_DIALOGUE_REQUEST_BYTES)
}
pub fn parse_dialogue_target(source: &str) -> Result<TargetRef> {
    decode::<StrictTarget>(source, 4096).map(Into::into)
}
fn decode<T: serde::de::DeserializeOwned>(source: &str, limit: usize) -> Result<T> {
    if source.len() > limit {
        return Err(DialogueError::new(
            "BUDGET_EXCEEDED",
            "台词请求超过字节预算",
        ));
    }
    let value = crate::parse_unique_json(source.as_bytes())
        .map_err(|e| DialogueError::new("INVALID_REQUEST", format!("台词 JSON 无效：{e}")))?;
    serde_json::from_value(value)
        .map_err(|e| DialogueError::new("INVALID_REQUEST", format!("台词请求字段无效：{e}")))
}

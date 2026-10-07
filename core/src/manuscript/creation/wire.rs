use super::*;

pub fn parse_manuscript_chapter_create_request(
    json: &str,
) -> Result<ManuscriptChapterCreateRequest, String> {
    if json.len() > 64 * 1024 {
        return Err("新章请求超过 64 KiB 预算".into());
    }
    let value = crate::parse_unique_json(json.as_bytes())?;
    serde_json::from_value(value).map_err(|error| format!("无效新章请求：{error}"))
}

pub(super) fn target<'de, D: serde::Deserializer<'de>>(d: D) -> Result<TargetRef, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Strict {
        kind: String,
        id: String,
    }
    let value = Strict::deserialize(d)?;
    Ok(TargetRef {
        kind: value.kind,
        id: value.id,
    })
}

pub(super) fn revision<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Revision, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Strict {
        workspace_generation: u64,
        content_generation: u64,
        presentation_generation: u64,
    }
    let value = Strict::deserialize(d)?;
    Ok(Revision {
        workspace_generation: value.workspace_generation,
        content_generation: value.content_generation,
        presentation_generation: value.presentation_generation,
    })
}

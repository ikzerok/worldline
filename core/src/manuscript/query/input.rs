//! 所有机器消费者复用同一只读DTO、严格JSON与请求预算。
use super::*;
use serde_json::Value;
pub const MAX_MANUSCRIPT_QUERY_INPUT_BYTES: usize = 4 * 1024 * 1024;

impl ManuscriptQueryRequest {
    pub fn validate(&self) -> Result<(), ManuscriptQueryError> {
        if self.schema_version != MANUSCRIPT_QUERY_SCHEMA_VERSION {
            return Err(ManuscriptQueryError::new(
                "UNSUPPORTED_QUERY_VERSION",
                "不支持的书稿查询版本",
            ));
        }
        if !(1..=crate::manuscript::MAX_MANUSCRIPT_PAGE_SIZE).contains(&self.limit) {
            return Err(ManuscriptQueryError::new(
                "INVALID_LIMIT",
                "书稿查询limit必须为1到100；请求未钳制",
            ));
        }
        if self.manuscript_id.is_empty() {
            return Err(ManuscriptQueryError::new(
                "INVALID_QUERY",
                "书稿查询必须指定稳定书稿 ID",
            ));
        }
        if [&self.text, &self.status, &self.pov]
            .iter()
            .any(|value| value.len() > 4096)
            || self.manuscript_id.len() > 256
            || self.section_id.as_ref().is_some_and(|id| id.len() > 256)
            || self.selected_id.as_ref().is_some_and(|id| id.len() > 256)
            || self.collapsed.len() > 16_384
            || self.collapsed.iter().any(|id| id.len() > 256)
            || self
                .cursor
                .as_ref()
                .is_some_and(|cursor| cursor.len() > 512)
        {
            return Err(ManuscriptQueryError::new(
                "BUDGET_EXCEEDED",
                "书稿查询字段超过输入预算；请求未截断",
            ));
        }
        Ok(())
    }
}

pub fn parse_manuscript_query_request(raw: &str) -> Result<ManuscriptQueryRequest, String> {
    let value = parse(raw)?;
    let query: ManuscriptQueryRequest =
        serde_json::from_value(value).map_err(|error| format!("无效书稿查询 DTO：{error}"))?;
    query.validate().map_err(|error| error.to_string())?;
    Ok(query)
}

pub fn parse_manuscript_query_drafts(raw: &str) -> Result<Vec<ManuscriptQueryDraft>, String> {
    let value = parse(raw)?;
    let drafts = value.as_array().ok_or("编排草稿必须是数组")?;
    for draft in drafts {
        if let Some(entries) = draft
            .get("draft")
            .and_then(|value| value.get("entries"))
            .and_then(Value::as_array)
        {
            for entry in entries {
                for field in ["pov", "target_ref"] {
                    if let Some(target) = entry.get(field).filter(|target| !target.is_null()) {
                        let target = target.as_object().ok_or("草稿引用必须为kind/id对象")?;
                        if target.len() != 2
                            || !target.contains_key("kind")
                            || !target.contains_key("id")
                        {
                            return Err("草稿引用只能包含kind和id，未知字段未被忽略".into());
                        }
                    }
                }
            }
        }
    }
    serde_json::from_value(value).map_err(|error| format!("无效书稿编排草稿 DTO：{error}"))
}

fn parse(raw: &str) -> Result<Value, String> {
    if raw.len() > MAX_MANUSCRIPT_QUERY_INPUT_BYTES {
        return Err("书稿查询输入超过4 MiB预算；请求未截断".into());
    }
    crate::parse_unique_json(raw.as_bytes()).map_err(|error| format!("无效书稿查询JSON：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manuscript_query_machine_input_rejects_duplicate_unknown_and_oversized_fields() {
        assert!(parse_manuscript_query_request(
            r#"{"schema_version":1,"manuscript_id":"book","text":"a","text":"b"}"#
        )
        .is_err());
        assert!(parse_manuscript_query_request(
            r#"{"schema_version":1,"manuscript_id":"book","typo":true}"#
        )
        .is_err());
        let mut request = ManuscriptQueryRequest {
            manuscript_id: "book".into(),
            ..Default::default()
        };
        request.text = "x".repeat(4097);
        assert_eq!(request.validate().unwrap_err().code, "BUDGET_EXCEEDED");
        assert!(parse_manuscript_query_drafts(r#"[{"expected_baseline":"base","draft":{"id":"book","title":"Book","entries":[{"id":"one","kind":"chapter","title":"One","target_ref":{"kind":"event","id":"start","future":true}}]}}]"#).is_err());
        assert!(parse_manuscript_query_drafts("[]").unwrap().is_empty());
    }
}

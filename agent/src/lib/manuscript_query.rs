//! 书稿查询机器入口：不应用、不保存，严格DTO与筛选语义均来自core。
use super::*;
use worldline_core::manuscript::{
    parse_manuscript_query_drafts, parse_manuscript_query_request, MAX_MANUSCRIPT_QUERY_INPUT_BYTES,
};

impl Server {
    pub(super) fn manuscript_query(&mut self, params: &Value) -> Result<Value, ProtoError> {
        if !fits(params, MAX_MANUSCRIPT_QUERY_INPUT_BYTES) {
            return Err(ProtoError::new(
                -32602,
                "书稿查询请求超过4 MiB预算；请求未截断",
            ));
        }
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "书稿查询参数必须是对象"))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "project_id" | "query" | "drafts"))
        {
            return Err(ProtoError::new(-32602, "书稿查询含未知参数"));
        }
        let id = param_str(params, "project_id")?;
        let raw = params
            .get("query")
            .ok_or_else(|| ProtoError::new(-32602, "需要query DTO"))?;
        let query = parse_manuscript_query_request(&raw.to_string())
            .map_err(|message| ProtoError::new(-32602, message))?;
        let raw_drafts = params
            .get("drafts")
            .map(Value::to_string)
            .unwrap_or_else(|| "[]".into());
        let drafts = parse_manuscript_query_drafts(&raw_drafts)
            .map_err(|message| ProtoError::new(-32602, message))?;
        let unit = self
            .projects
            .get(id)
            .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
        let outcome = unit
            .project
            .manuscript_query_snapshot(&[], &drafts)
            .and_then(|snapshot| snapshot.query(&query));
        let payload = match outcome {
            Ok(page) => {
                json!({"ok":page.complete,"page":page,"error":if page.complete { Value::Null } else {
                json!({"code":"INCOMPLETE_SNAPSHOT","message":"书稿快照未完整确认；页面仅表示可识别范围，请核对诊断"})
            },"applied":false,"saved":false,"baseline":unit.project.content_baseline()})
            }
            Err(error) => {
                json!({"ok":false,"page":null,"error":error,"applied":false,"saved":false,
                "baseline":unit.project.content_baseline()})
            }
        };
        if !fits(&payload, MAX_MANUSCRIPT_QUERY_INPUT_BYTES) {
            return Ok(
                json!({"ok":false,"page":null,"error":{"code":"BUDGET_EXCEEDED",
                "message":"书稿查询响应超过4 MiB预算；页面未截断，请缩小页或查询范围"},"applied":false,"saved":false}),
            );
        }
        Ok(payload)
    }
}

fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("manuscript_query_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}

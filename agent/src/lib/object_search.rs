use super::*;
use worldline_core::object_search::{ObjectSearchError, ObjectSearchFilter, ObjectSearchOptions};

impl Server {
    pub(super) fn object_search(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "参数必须是对象"))?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "project_id" | "query" | "filter" | "options" | "expected_baseline"
            )
        }) {
            return Err(ProtoError::new(-32602, "对象检索含未知参数"));
        }
        if serde_json::to_vec(params)
            .map_err(|e| ProtoError::new(-32602, e.to_string()))?
            .len()
            > 64 * 1024
        {
            return Err(ProtoError::new(-32602, "对象检索参数超过 64 KiB"));
        }
        let id = param_str(params, "project_id")?;
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ProtoError::new(-32602, "query 必须是字符串"))?;
        let filter: ObjectSearchFilter = params
            .get("filter")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|e| ProtoError::new(-32602, format!("无效检索 filter：{e}")))?
            .unwrap_or_default();
        let options: ObjectSearchOptions = params
            .get("options")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|e| ProtoError::new(-32602, format!("无效检索 options：{e}")))?
            .unwrap_or_default();
        let expected = if params.get("expected_baseline").is_some() {
            Some(param_str(params, "expected_baseline")?)
        } else {
            None
        };
        let unit = self
            .projects
            .get(id)
            .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
        let baseline = unit.project.content_baseline();
        if expected.is_some_and(|value| value != baseline) {
            return Ok(
                json!({"ok":false,"page":null,"baseline":baseline,"error":{"code":"STALE_BASELINE","message":"对象检索基线已过期，请重新查询"}}),
            );
        }
        let content = unit.project.compile_object_search_snapshot();
        let mut response = json!({"schema_version":1,"baseline":baseline,"language_version":unit.project.language_version(),
            "snapshot":"applied","diagnostics":content.diagnostics,"workspace_diagnostics":unit.project.authoring_diagnostics(),
            "read_only":!unit.project.authoring_diagnostics().is_empty()});
        match content
            .analysis
            .catalog
            .search_objects_filtered_page(query, &filter, options)
        {
            Ok(page) => {
                response["ok"] = json!(!content.has_errors());
                response["page"] = json!(page);
                if content.has_errors() {
                    response["error"] = json!({"code":"INVALID_SOURCE","message":"当前已应用源码存在错误；目录页不代表完整可解析工程"});
                }
            }
            Err(error) => {
                response["ok"] = json!(false);
                response["page"] = Value::Null;
                response["error"] = json!({"code":error_code(&error),"message":error.to_string()});
            }
        }
        Ok(response)
    }
}
fn error_code(error: &ObjectSearchError) -> &'static str {
    match error {
        ObjectSearchError::InvalidLimit { .. } => "INVALID_LIMIT",
        ObjectSearchError::InvalidCandidateBudget { .. } => "INVALID_CANDIDATE_BUDGET",
        ObjectSearchError::CandidateBudgetExceeded { .. } => "CANDIDATE_BUDGET_EXCEEDED",
        ObjectSearchError::InvalidOffset { .. } => "INVALID_OFFSET",
    }
}

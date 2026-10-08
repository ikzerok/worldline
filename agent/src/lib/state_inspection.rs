//! session.inspect只读取已存在Story，不隐式创建、选择或推进。
use super::*;
use worldline_runtime::StateInspectionQuery;
impl Server {
    pub(super) fn session_inspect(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "参数必须是对象"))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "session_id" | "query"))
        {
            return Err(ProtoError::new(-32602, "状态检查含未知参数"));
        }
        if serde_json::to_writer(&mut InputBudget(0), params).is_err() {
            return Err(ProtoError::new(-32602, "状态检查参数超过64KiB"));
        }
        let query: StateInspectionQuery = params
            .get("query")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| ProtoError::new(-32602, format!("状态查询参数无效：{error}")))?
            .unwrap_or_default();
        let session = self.session(params)?;
        match session.story.inspect_state(&query) {
            Ok(page) => Ok(json!({"ok":true,"inspection":page})),
            Err(error) if error.code == "INVALID_INSPECTION_QUERY" => {
                Err(ProtoError::new(-32602, error.message))
            }
            Err(error) => Ok(json!({"ok":false,"inspection":null,"error":error})),
        }
    }
}

struct InputBudget(usize);
impl std::io::Write for InputBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= 64 * 1024)
            .ok_or_else(|| std::io::Error::other("状态检查参数超过64KiB"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

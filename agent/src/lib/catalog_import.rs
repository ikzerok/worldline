use super::*;
use worldline_core::catalog_import::CatalogImportRequest;

impl Server {
    pub(super) fn catalog_import(&mut self, params: &Value, apply: bool) -> Result<Value, ProtoError> {
        let object = params.as_object().ok_or_else(|| ProtoError::new(-32602, "参数必须是对象"))?;
        if object.keys().any(|key| !matches!(key.as_str(), "project_id" | "request") && !(apply && key == "plan_digest")) {
            return Err(ProtoError::new(-32602, "资料导入含未知参数"));
        }
        let id = param_str(params, "project_id")?;
        let request: CatalogImportRequest = serde_json::from_value(params.get("request").cloned().ok_or_else(|| ProtoError::new(-32602, "需要request DTO"))?)
            .map_err(|error| ProtoError::new(-32602, format!("无效资料导入DTO：{error}")))?;
        let digest = if apply { Some(param_str(params, "plan_digest")?) } else { None };
        let unit = self.projects.get_mut(id).ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
        let result = if let Some(digest) = digest {
            unit.project.apply_catalog_import(&request, digest).map(|result| json!({"ok":true,"operation":"apply","plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline,"saved":false}))
        } else {
            unit.project.preview_catalog_import(&request).map(|plan| {
                let mut payload=json!({"ok":plan.can_apply,"operation":"preview","plan":plan,"saved":false});
                if !plan.can_apply { payload["error"]=json!({"code":"CATALOG_IMPORT_BLOCKED","message":"资料导入存在阻断诊断，请查看完整计划"}); }
                payload
            })
        };
        Ok(match result {
            Ok(result) => result,
            Err(message) => json!({"ok":false,"saved":false,"error":{"code":"CATALOG_IMPORT_REJECTED","message":message}}),
        })
    }

    /// 与导入解耦的显式保存；不刷新、不丢弃调用者已提交的内存改稿。
    pub(super) fn project_save(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let object = params.as_object().ok_or_else(|| ProtoError::new(-32602, "参数必须是对象"))?;
        if object.keys().any(|key| !matches!(key.as_str(), "project_id" | "expected_baseline")) {
            return Err(ProtoError::new(-32602, "project.save含未知参数"));
        }
        let id = param_str(params, "project_id")?;
        let expected = param_str(params, "expected_baseline")?;
        let unit = self.projects.get_mut(id).ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
        if unit.project.content_baseline() != expected {
            return Ok(json!({"ok":false,"saved":false,"error":{"code":"STALE_BASELINE","message":"保存基线已过期"}}));
        }
        Ok(match unit.project.save() {
            Ok(()) => json!({"ok":true,"saved":true,"baseline":unit.project.content_baseline()}),
            Err(message) => json!({"ok":false,"saved":false,"error":{"code":"SAVE_FAILED","message":message}}),
        })
    }
}

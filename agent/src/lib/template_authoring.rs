//! 模板结构、投影和事务均由core执行；RPC不持有另一份模板模型。
use super::*;
use serde::de::DeserializeOwned;
use worldline_core::project_templates::protocol::{
    TemplateDraftRequest, TemplateMutationRequest, MAX_TEMPLATE_REQUEST_BYTES,
};

impl Server {
    pub(super) fn template_draft(&mut self, params: &Value) -> Result<Value, ProtoError> {
        parameters(params, false)?;
        let request: TemplateDraftRequest = request(params)?;
        let unit = self
            .projects
            .get(param_str(params, "project_id")?)
            .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
        match unit.project.template_draft_request(&request) {
            Ok(result) => {
                let mut payload = serde_json::to_value(result)
                    .map_err(|e| ProtoError::new(-32603, e.to_string()))?;
                payload["ok"] = json!(true);
                payload["revision"] = json!(unit.scene_revision);
                Ok(payload)
            }
            Err(error) => Ok(json!({"ok":false,"projection":null,"error":error,
                "baseline":unit.project.content_baseline(),"revision":unit.scene_revision,
                "applied":false,"saved":false})),
        }
    }

    pub(super) fn template_mutation(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        parameters(params, apply)?;
        let request: TemplateMutationRequest = request(params)?;
        let digest = if apply {
            Some(param_str(params, "plan_digest")?)
        } else {
            None
        };
        let unit = self
            .projects
            .get_mut(param_str(params, "project_id")?)
            .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
        let outcome = if let Some(digest) = digest {
            unit.project
                .apply_template_request(&mut unit.scene_revision, &request, digest)
                .map(|result| {
                    json!({"ok":true,"plan":result.plan,"changed_files":result.changed_files,
                    "applied":true,"saved":false})
                })
        } else {
            unit.project
                .preview_template_request(unit.scene_revision, &request)
                .map(|plan| json!({"ok":plan.can_apply,"plan":plan,"applied":false,"saved":false}))
        };
        let mut payload = outcome.unwrap_or_else(|error| {
            json!({"ok":false,"plan":null,
            "error":error,"applied":false,"saved":false})
        });
        payload["baseline"] = json!(unit.project.content_baseline());
        payload["revision"] = json!(unit.scene_revision);
        Ok(payload)
    }
}

fn parameters(params: &Value, apply: bool) -> Result<(), ProtoError> {
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "参数必须是对象"))?;
    if object.keys().any(|key| {
        !matches!(key.as_str(), "project_id" | "request") && !(apply && key == "plan_digest")
    }) {
        return Err(ProtoError::new(-32602, "模板请求含未知参数"));
    }
    Ok(())
}

fn request<T: DeserializeOwned>(params: &Value) -> Result<T, ProtoError> {
    let value = params
        .get("request")
        .ok_or_else(|| ProtoError::new(-32602, "需要模板 request DTO"))?;
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("额度溢出"))?;
            if self.0 > MAX_TEMPLATE_REQUEST_BYTES {
                return Err(std::io::Error::other("请求过大"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(0), value)
        .map_err(|_| ProtoError::new(-32602, "模板请求超过 4 MiB 预算"))?;
    serde_json::from_value(value.clone())
        .map_err(|e| ProtoError::new(-32602, format!("无效模板 DTO：{e}")))
}

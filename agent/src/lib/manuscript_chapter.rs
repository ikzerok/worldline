use super::*;
use worldline_core::manuscript::ManuscriptChapterCreateRequest;

impl Server {
    pub(super) fn manuscript_chapter(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "参数必须是对象"))?;
        if object.keys().any(|key| {
            !matches!(key.as_str(), "project_id" | "request") && !(apply && key == "plan_digest")
        }) {
            return Err(ProtoError::new(-32602, "新章请求含未知参数"));
        }
        let id = param_str(params, "project_id")?;
        let value = params
            .get("request")
            .ok_or_else(|| ProtoError::new(-32602, "需要 request DTO"))?;
        if serde_json::to_vec(value)
            .map_err(|e| ProtoError::new(-32602, e.to_string()))?
            .len()
            > 64 * 1024
        {
            return Err(ProtoError::new(-32602, "新章请求超过 64 KiB 预算"));
        }
        let request: ManuscriptChapterCreateRequest = serde_json::from_value(value.clone())
            .map_err(|e| ProtoError::new(-32602, format!("无效新章 DTO：{e}")))?;
        let digest = if apply {
            Some(param_str(params, "plan_digest")?)
        } else {
            None
        };
        let unit = self
            .projects
            .get_mut(id)
            .ok_or_else(|| ProtoError::new(-32602, "未知 project_id"))?;
        let operation = if apply { "apply" } else { "preview" };
        let outcome = if let Some(digest) = digest {
            unit.project.apply_manuscript_chapter_create(&mut unit.scene_revision, &request, digest)
                .map(|result| json!({"ok":true,"operation":operation,"plan":result.plan,"result":result,"applied":true,"saved":false}))
        } else {
            unit.project.preview_manuscript_chapter_create(unit.scene_revision, &request)
                .map(|plan| json!({"ok":plan.can_apply,"operation":operation,"plan":plan,"applied":false,"saved":false}))
        };
        let mut payload = match outcome {
            Ok(value) => value,
            Err(error) => {
                json!({"ok":false,"operation":operation,"plan":null,"error":error,"applied":false,"saved":false})
            }
        };
        payload["revision"] = json!(unit.scene_revision);
        payload["baseline"] = json!(unit.project.content_baseline());
        Ok(payload)
    }
}

use super::*;
use worldline_core::source_lifecycle::SourceLifecycleRequest;

impl Server {
    pub(super) fn source_lifecycle(
        &mut self,
        params: &Value,
        apply: bool,
    ) -> Result<Value, ProtoError> {
        let allowed: &[&str] = if apply {
            &["path", "project_id", "request", "plan_digest"]
        } else {
            &["path", "project_id", "request"]
        };
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "源码生命周期参数必须是对象"))?;
        if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(ProtoError::new(
                -32602,
                format!("未知或不适用的源码生命周期参数 `{key}`"),
            ));
        }
        if params.get("project_id").is_some() == params.get("path").is_some() {
            return Err(ProtoError::new(-32602, "必须且只能提供project_id或path"));
        }
        let request: SourceLifecycleRequest = serde_json::from_value(
            params
                .get("request")
                .cloned()
                .ok_or_else(|| ProtoError::new(-32602, "需要request DTO"))?,
        )
        .map_err(|error| ProtoError::new(-32602, format!("无效源码生命周期DTO：{error}")))?;
        let digest = if apply {
            Some(nonempty_string(params, "plan_digest")?)
        } else {
            None
        };
        if params.get("project_id").is_some() {
            let id = nonempty_string(params, "project_id")?;
            let unit = self
                .projects
                .get_mut(id)
                .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
            return Ok(execute(&mut unit.project, &request, digest));
        }
        let path = nonempty_string(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(message) => {
                return Ok(json!({"ok":false,"operation":operation(digest),"plan":null,
                    "baseline":null,"applied":false,"saved":false,
                    "error":{"code":"IO_ERROR","message":message,"stage":"open"}}));
            }
        };
        Ok(execute(&mut project, &request, digest))
    }
}

fn nonempty_string<'a>(params: &'a Value, key: &str) -> Result<&'a str, ProtoError> {
    let value = param_str(params, key)?;
    if value.trim().is_empty() {
        return Err(ProtoError::new(-32602, format!("参数 `{key}` 不能为空")));
    }
    Ok(value)
}

fn operation(digest: Option<&str>) -> &'static str {
    if digest.is_some() {
        "apply"
    } else {
        "preview"
    }
}

fn execute(project: &mut Project, request: &SourceLifecycleRequest, digest: Option<&str>) -> Value {
    let operation = operation(digest);
    let result = match digest {
        Some(digest) => project.apply_source_lifecycle(request, digest),
        None => project.preview_source_lifecycle(request),
    };
    let plan = match result {
        Ok(plan) => plan,
        Err(message) => {
            return json!({"ok":false,"operation":operation,"plan":null,
                "baseline":project.content_baseline(),"applied":false,"saved":false,
                "error":{"code":"SOURCE_LIFECYCLE_REJECTED","message":message,
                    "stage":operation}});
        }
    };
    let applied = digest.is_some()
        && !(matches!(plan.request, SourceLifecycleRequest::MoveEntity { .. })
            && plan.changes.is_empty());
    if applied {
        // 保存失败可能已有文件替换；不可恢复旧快照、谎报零修改或丢弃恢复日志。
        if let Err(message) = project.save() {
            return json!({"ok":false,"operation":operation,"plan":plan,
                "baseline":project.content_baseline(),"applied":true,"saved":false,
                "error":{"code":"SOURCE_LIFECYCLE_REJECTED","message":message,
                    "stage":"save"}});
        }
    }
    json!({"ok":true,"operation":operation,"plan":plan,
        "baseline":project.content_baseline(),"applied":applied,"saved":applied})
}

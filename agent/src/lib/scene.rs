use super::*;
use worldline_core::presentation_commands::Revision;
use worldline_core::vector_scene::{self, SceneBatch, SceneError};

impl Server {
    pub(super) fn scene(&mut self, params: &Value, operation: &str) -> Result<Value, ProtoError> {
        let allowed: &[&str] = match operation {
            "svg-preview" => &["source"],
            "preview" => &["path", "project_id", "batch"],
            "apply" => &["path", "project_id", "batch", "baseline", "plan_digest"],
            "export" => &["path", "project_id", "map_id"],
            _ => unreachable!(),
        };
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "场景参数必须是对象"))?;
        if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(ProtoError::new(
                -32602,
                format!("未知或不适用的场景参数 `{key}`"),
            ));
        }
        if operation == "svg-preview" {
            let source = param_str(params, "source")?;
            return Ok(match worldline_core::svg_import::preview_scene(source) {
                Ok(preview) => json!({"ok":true,"operation":operation,"preview":preview}),
                Err(error) => failure(operation, error, None),
            });
        }
        let batch = if matches!(operation, "preview" | "apply") {
            let value = params
                .get("batch")
                .ok_or_else(|| ProtoError::new(-32602, "场景操作缺少batch"))?;
            if value.to_string().len() > 32 * 1024 * 1024 {
                return Err(ProtoError::new(-32602, "场景请求超过32MiB预算"));
            }
            Some(
                serde_json::from_value::<SceneBatch>(value.clone())
                    .map_err(|e| ProtoError::new(-32602, format!("场景batch无效：{e}")))?,
            )
        } else {
            param_str(params, "map_id")?;
            None
        };
        if operation == "apply" {
            param_str(params, "baseline")?;
            param_str(params, "plan_digest")?;
        }
        if params.get("path").is_some() == params.get("project_id").is_some() {
            return Err(ProtoError::new(
                -32602,
                "场景操作必须且只能提供path或project_id",
            ));
        }
        if params.get("project_id").is_some() {
            let id = param_str(params, "project_id")?;
            let unit = self
                .projects
                .get_mut(id)
                .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
            return Ok(execute(
                &mut unit.project,
                &mut unit.scene_revision,
                params,
                operation,
                batch,
            ));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(failure(
                    operation,
                    SceneError::new("SCENE_STORAGE", error),
                    None,
                ))
            }
        };
        Ok(execute(
            &mut project,
            &mut Revision::default(),
            params,
            operation,
            batch,
        ))
    }
}

fn execute(
    project: &mut Project,
    revision: &mut Revision,
    params: &Value,
    operation: &str,
    batch: Option<SceneBatch>,
) -> Value {
    match project.refresh() {
        Ok(conflicts) if !conflicts.is_empty() => {
            return failure(
                operation,
                SceneError::new("SCENE_CONFLICT", "工程存在外部修改冲突"),
                Some(project.content_baseline()),
            )
        }
        Err(error) => {
            return failure(
                operation,
                SceneError::new("SCENE_STORAGE", error),
                Some(project.content_baseline()),
            )
        }
        _ => {}
    }
    let baseline = project.content_baseline();
    if operation == "export" {
        let index = project.map_index();
        let map_id = params["map_id"].as_str().expect("validated map_id");
        let Some(map) = index.maps.get(map_id) else {
            return failure(
                operation,
                SceneError::new("SCENE_REFERENCE", "地图不存在或无法安全读取"),
                Some(baseline),
            );
        };
        return match vector_scene::map_to_safe_svg(map, None) {
            Ok(svg) => {
                json!({"ok":true,"operation":operation,"svg":svg,"baseline":baseline,"revision":revision})
            }
            Err(error) => failure(operation, error, Some(baseline)),
        };
    }
    if operation == "apply" && params["baseline"].as_str() != Some(&baseline) {
        return failure(
            operation,
            SceneError::new("SCENE_STALE", "场景预览内容基线已过期"),
            Some(baseline),
        );
    }
    let batch = batch.expect("validated batch");
    let plan = match vector_scene::preview_batch(project, *revision, batch.clone()) {
        Ok(plan) => plan,
        Err(error) => return failure(operation, error, Some(baseline)),
    };
    let digest = match worldline_core::scene_protocol::plan_digest(&baseline, &batch, &plan) {
        Ok(digest) => digest,
        Err(error) => return failure(operation, error, Some(baseline)),
    };
    if operation == "preview" {
        return json!({"ok":true,"operation":operation,"plan":plan,"baseline":baseline,
            "revision":revision,"plan_digest":digest});
    }
    if params["plan_digest"].as_str() != Some(&digest) {
        return failure(
            operation,
            SceneError::new("SCENE_STALE", "场景预览摘要已过期或与请求不匹配"),
            Some(baseline),
        );
    }
    let mut candidate = project.clone();
    let mut next_revision = *revision;
    let result = match vector_scene::apply_batch(&mut candidate, &mut next_revision, &plan) {
        Ok(result) => result,
        Err(error) => return failure(operation, error, Some(baseline)),
    };
    if let Err(error) = candidate.save() {
        return failure(
            operation,
            SceneError::new("SCENE_STORAGE", error),
            Some(baseline),
        );
    }
    *project = candidate;
    *revision = next_revision;
    json!({"ok":true,"operation":operation,"plan":plan,"result":{"changed_files":result.changed_files,"affected_refs":result.affected_refs,"diagnostics":result.diagnostics},"baseline":project.content_baseline(),
        "revision":revision,"plan_digest":digest})
}

fn failure(operation: &str, error: SceneError, baseline: Option<String>) -> Value {
    json!({"ok":false,"operation":operation,"error":error,"baseline":baseline})
}

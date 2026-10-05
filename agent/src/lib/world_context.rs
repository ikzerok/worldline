use super::projects::{query_failure, query_payload_base, refreshed_workspace};
use super::*;
use worldline_core::WorldContextOptions;

struct Request {
    target: Option<TargetRef>,
    options: WorldContextOptions,
    left: Option<String>,
    right: Option<String>,
    expected_baseline: Option<String>,
}
impl Server {
    pub(super) fn world_context(
        &mut self,
        params: &Value,
        mode: &str,
    ) -> Result<Value, ProtoError> {
        let request = parse(params, mode)?;
        if params.get("project_id").is_some() {
            let id = nonempty(params, "project_id")?;
            let unit = self
                .projects
                .get_mut(id)
                .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
            return Ok(execute(&mut unit.project, request, mode));
        }
        let path = nonempty(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(message) => return Ok(query_failure("IO_ERROR", message, None, None, None, &[])),
        };
        Ok(execute(&mut project, request, mode))
    }
}
fn parse(params: &Value, mode: &str) -> Result<Request, ProtoError> {
    let allowed = match mode {
        "object" => &["path", "project_id", "target"][..],
        "context" => &["path", "project_id", "target", "options"][..],
        "temporal" => &["path", "project_id", "left", "right", "expected_baseline"][..],
        _ => unreachable!(),
    };
    let object = params
        .as_object()
        .ok_or_else(|| ProtoError::new(-32602, "查询参数必须是对象"))?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(ProtoError::new(-32602, format!("未知或不适用参数 `{key}`")));
    }
    if params.get("path").is_some() == params.get("project_id").is_some() {
        return Err(ProtoError::new(-32602, "必须且只能提供path或project_id"));
    }
    // 两个分支均在打开工程前验证身份字符串。
    nonempty(
        params,
        if params.get("path").is_some() {
            "path"
        } else {
            "project_id"
        },
    )?;
    let target = if mode != "temporal" {
        let target = params
            .get("target")
            .and_then(Value::as_object)
            .ok_or_else(|| ProtoError::new(-32602, "target必须是kind/id对象"))?;
        if target.len() != 2 || !target.contains_key("kind") || !target.contains_key("id") {
            return Err(ProtoError::new(-32602, "target只能包含kind和id"));
        }
        let value = Value::Object(target.clone());
        let kind = nonempty(&value, "kind")?;
        let id = nonempty(&value, "id")?;
        if !worldline_core::catalog::TARGET_KINDS.contains(&kind) {
            return Err(ProtoError::new(-32602, "未知target kind"));
        }
        Some(TargetRef::new(kind, id))
    } else {
        None
    };
    let options = params
        .get("options")
        .map(|value| serde_json::from_value::<WorldContextOptions>(value.clone()))
        .transpose()
        .map_err(|e| ProtoError::new(-32602, format!("无效上下文选项：{e}")))?
        .unwrap_or_default();
    options
        .validate()
        .map_err(|e| ProtoError::new(-32602, e.to_string()))?;
    Ok(Request {
        target,
        options,
        left: (mode == "temporal")
            .then(|| nonempty(params, "left").map(str::to_string))
            .transpose()?,
        right: (mode == "temporal")
            .then(|| nonempty(params, "right").map(str::to_string))
            .transpose()?,
        expected_baseline: params
            .get("expected_baseline")
            .map(|_| nonempty(params, "expected_baseline").map(str::to_string))
            .transpose()?,
    })
}
fn nonempty<'a>(params: &'a Value, key: &str) -> Result<&'a str, ProtoError> {
    let value = param_str(params, key)?;
    if value.trim().is_empty() {
        return Err(ProtoError::new(-32602, format!("{key}不能为空")));
    }
    Ok(value)
}
fn execute(project: &mut Project, request: Request, mode: &str) -> Value {
    let snapshot = match refreshed_workspace(project) {
        Ok(snapshot) => snapshot,
        Err(message) => {
            let result = project.compile();
            return query_failure(
                "IO_ERROR",
                message,
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            );
        }
    };
    let mut payload = query_payload_base(&snapshot);
    payload.insert(
        "executable_context".into(),
        json!(worldline_core::world_context::EXECUTABLE_CONTEXT_CAPABILITY),
    );
    let mut ok = !snapshot.result.has_errors();
    match mode {
        "object" => match snapshot
            .result
            .lookup_world_object(request.target.as_ref().unwrap())
        {
            Ok(object) => {
                payload.insert("object".into(), json!(object));
                payload.insert(
                    "snapshot".into(),
                    json!(snapshot.result.world_context_snapshot()),
                );
            }
            Err(error) => {
                ok = false;
                payload.insert("object".into(), Value::Null);
                payload.insert(
                    "error".into(),
                    json!({"code":error.code(),"message":error.to_string()}),
                );
            }
        },
        "context" => match snapshot
            .result
            .query_world_context(request.target.as_ref().unwrap(), request.options)
        {
            Ok(mut context) => {
                context.content_baseline = Some(snapshot.baseline.clone());
                if !snapshot.conflicts.is_empty() || !project.recovery_conflicts().is_empty() {
                    context.mark_source_conflict();
                }
                payload.insert("truncated".into(), json!(context.truncated));
                payload.insert("context".into(), json!(context));
            }
            Err(error) => {
                ok = false;
                payload.insert("context".into(), Value::Null);
                payload.insert(
                    "error".into(),
                    json!({"code":error.code(),"message":error.to_string()}),
                );
            }
        },
        "temporal" => {
            if !snapshot.conflicts.is_empty() || !project.recovery_conflicts().is_empty() {
                ok = false;
                payload.insert("comparison".into(), Value::Null);
                payload.insert("error".into(), json!({"code":"CONFLICT","message":"当前稿件与磁盘存在冲突，请先保留并核对两个版本"}));
            } else if request
                .expected_baseline
                .as_ref()
                .is_some_and(|value| value != &snapshot.baseline)
            {
                ok = false;
                payload.insert("comparison".into(), Value::Null);
                payload.insert(
                    "error".into(),
                    json!({"code":"STALE_BASELINE","message":"稿件已变化，请重新查询时间证据"}),
                );
            } else {
                let comparison = snapshot.result.analysis.timeline.compare(
                    request.left.as_deref().unwrap(),
                    request.right.as_deref().unwrap(),
                );
                ok &= !matches!(
                    comparison.relation,
                    worldline_core::timeline::TemporalRelation::Invalid
                        | worldline_core::timeline::TemporalRelation::Unknown
                );
                payload.insert("comparison".into(), json!(comparison));
            }
        }
        _ => unreachable!(),
    }
    payload.insert("ok".into(), json!(ok));
    if !snapshot.conflicts.is_empty() {
        payload.insert("conflicts".into(), json!(snapshot.conflicts));
    }
    Value::Object(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_rpc_refuses_dirty_external_conflict_even_with_current_baseline() {
        let root = std::env::temp_dir().join(format!(
            "agent-world-context-conflict-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".world")).unwrap();
        std::fs::write(
            root.join(".world/project.json"),
            r#"{"schema_version":1,"language_version":"1.13"}"#,
        )
        .unwrap();
        let path = root.join("world.wl");
        let source = "period night\nevent a during night\n  -> END\nevent b during night follows a\n  -> END\n";
        std::fs::write(&path, source).unwrap();
        let mut server = Server::default();
        server
            .project_open(&json!({"path":root}))
            .unwrap_or_else(|error| panic!("{}", error.message));
        let project = &mut server.projects.get_mut("p1").unwrap().project;
        // Windows 的已载入文档键可能使用规范化前缀；沿用 Project 的真实入口身份。
        let path = project.entry.clone();
        project
            .set_text(&path, format!("{source}// local\n"))
            .unwrap();
        let baseline = project.content_baseline();
        std::fs::write(&path, format!("{source}// external\n")).unwrap();
        let value = server
            .world_context(
                &json!({"project_id":"p1","left":"a","right":"b","expected_baseline":baseline}),
                "temporal",
            )
            .unwrap_or_else(|error| panic!("{}", error.message));
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "CONFLICT");
        assert!(value["comparison"].is_null());
        assert!(!value["conflicts"].as_array().unwrap().is_empty());
        let context = server
            .world_context(
                &json!({"project_id":"p1","target":{"kind":"event","id":"a"}}),
                "context",
            )
            .unwrap_or_else(|error| panic!("{}", error.message));
        assert_eq!(context["ok"], true);
        assert_eq!(context["context"]["complete"], false);
        assert_eq!(context["context"]["truncated"], false);
        assert!(context["context"]["reasons"]
            .as_array()
            .unwrap()
            .contains(&json!("source_conflict")));
        assert!(!context["conflicts"].as_array().unwrap().is_empty());
        assert!(server.projects["p1"].project.documents[&path]
            .text
            .contains("// local"));
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("// external"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

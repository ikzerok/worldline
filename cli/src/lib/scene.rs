use super::*;
use std::collections::BTreeMap;
use worldline_core::presentation_commands::Revision;
use worldline_core::vector_scene::{self, SceneBatch, SceneError};

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let operation = args.first().map(String::as_str).ok_or("scene需要操作")?;
    let allowed: &[&str] = match operation {
        "svg-preview" => &["--source"],
        "preview" => &["--request-json"],
        "apply" => &["--request-json", "--baseline", "--plan-digest"],
        "export" => &["--map-id"],
        _ => return Err("scene操作须为svg-preview、preview、apply或export".into()),
    };
    let mut path = None;
    let mut values = BTreeMap::new();
    let mut json_output = false;
    let mut args = args[1..].iter();
    while let Some(argument) = args.next() {
        let (key, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(key, value)| (key, Some(value)));
        if key == "--json" {
            if inline.is_some() || json_output {
                return Err("--json不接受值且不能重复".into());
            }
            json_output = true;
        } else if allowed.contains(&key) {
            let value = inline
                .map(str::to_owned)
                .or_else(|| args.next().cloned())
                .ok_or_else(|| format!("{key}缺少值"))?;
            if values.insert(key.to_string(), value).is_some() {
                return Err(format!("{key}不能重复"));
            }
        } else if !argument.starts_with('-') && operation != "svg-preview" && path.is_none() {
            path = Some(PathBuf::from(argument));
        } else {
            return Err(format!("未知或不适用的scene参数：{argument}"));
        }
    }
    for key in allowed {
        if !values.contains_key(*key) {
            return Err(format!("scene {operation}需要{key}"));
        }
    }
    if operation == "svg-preview" {
        return emit(
            out,
            operation,
            json_output,
            worldline_core::svg_import::preview_scene(&values["--source"])
                .map(|preview| json!({"preview":preview})),
        );
    }
    let path = path.ok_or("scene需要工程目录或入口")?;
    let mut project = Project::open(&path)?;
    if operation == "export" {
        let index = project.map_index();
        let result = index
            .maps
            .get(&values["--map-id"])
            .ok_or_else(|| SceneError::new("SCENE_REFERENCE", "地图不存在或无法安全读取"))
            .and_then(|map| vector_scene::map_to_safe_svg(map, None))
            .map(|svg| json!({"svg":svg,"baseline":project.content_baseline()}));
        return emit(out, operation, json_output, result);
    }
    let request = &values["--request-json"];
    if request.len() > 32 * 1024 * 1024 {
        return Err("scene请求超过32MiB预算".into());
    }
    let batch: SceneBatch =
        serde_json::from_value(worldline_core::parse_unique_json(request.as_bytes())?)
            .map_err(|error| format!("scene batch无效：{error}"))?;
    let baseline = project.content_baseline();
    let result = (|| {
        if operation == "apply" && values["--baseline"] != baseline {
            return Err(SceneError::new("SCENE_STALE", "场景预览内容基线已过期"));
        }
        let mut revision = Revision::default();
        let plan = vector_scene::preview_batch(&project, revision, batch.clone())?;
        let digest = worldline_core::scene_protocol::plan_digest(&baseline, &batch, &plan)?;
        if operation == "preview" {
            return Ok(
                json!({"plan":plan,"baseline":baseline,"revision":revision,"plan_digest":digest}),
            );
        }
        if values["--plan-digest"] != digest {
            return Err(SceneError::new(
                "SCENE_STALE",
                "场景摘要已过期或与请求不匹配",
            ));
        }
        let result = vector_scene::apply_batch(&mut project, &mut revision, &plan)?;
        project
            .save()
            .map_err(|error| SceneError::new("SCENE_STORAGE", error))?;
        Ok(
            json!({"plan":plan,"result":{"changed_files":result.changed_files,"affected_refs":result.affected_refs,"diagnostics":result.diagnostics},"baseline":project.content_baseline(),"revision":revision,"plan_digest":digest}),
        )
    })();
    emit(out, operation, json_output, result)
}

fn emit(
    out: &mut impl Write,
    operation: &str,
    json_output: bool,
    result: Result<Value, SceneError>,
) -> Result<i32, String> {
    let (code, mut payload) = match result {
        Ok(mut value) => {
            value["ok"] = json!(true);
            (0, value)
        }
        Err(error) => (1, json!({"ok":false,"error":error})),
    };
    payload["operation"] = json!(operation);
    if json_output {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else if let Some(svg) = payload.get("svg").and_then(Value::as_str) {
        writeln!(out, "{svg}").map_err(|e| e.to_string())?;
    } else if code == 0 {
        writeln!(
            out,
            "场景{operation}完成；摘要 {}",
            payload
                .get("plan_digest")
                .and_then(Value::as_str)
                .unwrap_or("无写入")
        )
        .map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}：{}",
            payload["error"]["code"], payload["error"]["message"]
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(code)
}

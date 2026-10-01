use super::*;
use worldline_core::source_edit::SourceEditRequest;

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    execute(args, out, false)
}
pub(super) fn schema_command(
    args: &[String],
    out: &mut impl Write,
    apply: bool,
) -> Result<i32, String> {
    let mut args = args.to_vec();
    args.insert(0, if apply { "apply" } else { "preview" }.into());
    execute(&args, out, true)
}
fn execute(args: &[String], out: &mut impl Write, schema: bool) -> Result<i32, String> {
    let apply = match args.first().map(String::as_str) {
        Some("preview") => false,
        Some("apply") => true,
        _ => return Err("source-edit需要preview或apply".into()),
    };
    let mut path = None;
    let mut request = None;
    let mut digest = None;
    let mut json_output = false;
    let mut index = 1;
    while index < args.len() {
        let argument = &args[index];
        let (key, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(k, v)| (k, Some(v)));
        match key {
            "--json" if inline.is_none() => json_output = true,
            "--request-json" | "--plan-digest" => {
                let value = if let Some(value) = inline {
                    value.to_owned()
                } else {
                    index += 1;
                    args.get(index).ok_or("参数缺少值")?.clone()
                };
                let slot = if key == "--request-json" {
                    &mut request
                } else {
                    &mut digest
                };
                if slot.replace(value).is_some() {
                    return Err(format!("{key}不能重复"));
                }
            }
            _ if !argument.starts_with('-') && path.is_none() => {
                path = Some(PathBuf::from(argument))
            }
            _ => return Err(format!("source-edit未知或重复参数：{argument}")),
        }
        index += 1;
    }
    let path = path.ok_or("source-edit需要工程目录")?;
    let request: SourceEditRequest = serde_json::from_value(worldline_core::parse_unique_json(
        request.ok_or("source-edit需要--request-json")?.as_bytes(),
    )?)
    .map_err(|e| format!("无效源码草稿DTO：{e}"))?;
    if apply && digest.is_none() {
        return Err("apply需要--plan-digest".into());
    }
    if !apply && digest.is_some() {
        return Err("preview不能提供--plan-digest".into());
    }
    let mut project = Project::open(&path)?;
    let result = if schema {
        if apply {
            project
                .apply_schema_edit(&request, digest.as_deref().unwrap())
                .and_then(|preview| {
                    project.save()?;
                    Ok(json!(preview))
                })
        } else {
            project
                .preview_schema_edit(&request)
                .map(|preview| json!(preview))
        }
    } else if apply {
        project
            .apply_source_edit(&request, digest.as_deref().unwrap())
            .and_then(|preview| {
                project.save()?;
                Ok(json!(preview))
            })
    } else {
        project
            .preview_source_edit(&request)
            .map(|preview| json!(preview))
    };
    let (code, payload) = match result {
        Ok(preview) => (
            0,
            json!({"ok":true,"operation":if apply{"apply"}else{"preview"},"preview":preview,"baseline":project.content_baseline()}),
        ),
        Err(message) => (
            1,
            json!({"ok":false,"error":{"code":if schema {"SCHEMA_EDIT_REJECTED"} else {"SOURCE_EDIT_REJECTED"},"message":message},"baseline":project.content_baseline()}),
        ),
    };
    if json_output {
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else if code == 0 {
        writeln!(
            out,
            "源码草稿{}：预览摘要 {}",
            if apply {
                "已应用并保存"
            } else {
                "预览完成"
            },
            payload["preview"]["plan_digest"]
        )
        .map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{}", payload["error"]["message"]).map_err(|e| e.to_string())?;
    }
    Ok(code)
}

pub(super) fn schema_index(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let mut path = None;
    for argument in args {
        if argument == "--json" {
            continue;
        }
        if argument.starts_with('-') || path.replace(PathBuf::from(argument)).is_some() {
            return Err("schema-index只接受一个工程目录和--json".into());
        }
    }
    let project = Project::open(&path.ok_or("schema-index需要工程目录")?)?;
    let index = project.schema_index();
    let payload = json!({"ok":true,"index":index,"baseline":project.content_baseline(),"workspace_diagnostics":project.authoring_diagnostics()});
    writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    Ok(0)
}

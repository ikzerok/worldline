use super::*;
use worldline_core::source_lifecycle::SourceLifecycleRequest;

struct Args {
    path: PathBuf,
    request: SourceLifecycleRequest,
    digest: Option<String>,
    json: bool,
}

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let args = parse(args)?;
    let operation = if args.digest.is_some() {
        "apply"
    } else {
        "preview"
    };
    let mut project = match Project::open(&args.path) {
        Ok(project) => project,
        Err(message) => {
            let payload = json!({"ok":false,"operation":operation,"plan":null,
                "baseline":null,"applied":false,"saved":false,
                "error":{"code":"IO_ERROR","message":message,"stage":"open"}});
            return output(out, args.json, 2, payload);
        }
    };
    let result = match args.digest.as_deref() {
        Some(digest) => project.apply_source_lifecycle(&args.request, digest),
        None => project.preview_source_lifecycle(&args.request),
    };
    let plan = match result {
        Ok(plan) => plan,
        Err(message) => {
            let payload = json!({"ok":false,"operation":operation,"plan":null,
                "baseline":project.content_baseline(),"applied":false,"saved":false,
                "error":{"code":"SOURCE_LIFECYCLE_REJECTED","message":message,
                    "stage":operation}});
            return output(out, args.json, 1, payload);
        }
    };
    let applied = args.digest.is_some()
        && !(matches!(plan.request, SourceLifecycleRequest::MoveEntity { .. })
            && plan.changes.is_empty());
    if applied {
        // 保存可在逐文件替换中失败；保留已应用缓冲与 recoverable journal。
        if let Err(message) = project.save() {
            let payload = json!({"ok":false,"operation":operation,"plan":plan,
                "baseline":project.content_baseline(),"applied":true,"saved":false,
                "error":{"code":"SOURCE_LIFECYCLE_REJECTED","message":message,
                    "stage":"save"}});
            return output(out, args.json, 1, payload);
        }
    }
    let payload = json!({"ok":true,"operation":operation,"plan":plan,
        "baseline":project.content_baseline(),"applied":applied,"saved":applied});
    output(out, args.json, 0, payload)
}

fn parse(args: &[String]) -> Result<Args, String> {
    let apply = match args.first().map(String::as_str) {
        Some("preview") => false,
        Some("apply") => true,
        _ => return Err("source-lifecycle需要preview或apply".into()),
    };
    let mut path = None;
    let mut request = None;
    let mut digest = None;
    let mut json = false;
    let mut index = 1;
    while index < args.len() {
        let argument = &args[index];
        let (key, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(key, value)| (key, Some(value)));
        match key {
            "--json" if inline.is_none() && !json => json = true,
            "--request-json" | "--plan-digest" => {
                let value = match inline {
                    Some(value) => value.to_owned(),
                    None => {
                        index += 1;
                        args.get(index).ok_or("参数缺少值")?.clone()
                    }
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
            _ if !argument.starts_with('-') && path.is_none() && !argument.is_empty() => {
                path = Some(PathBuf::from(argument));
            }
            _ => return Err(format!("source-lifecycle未知或重复参数：{argument}")),
        }
        index += 1;
    }
    if apply
        && digest
            .as_ref()
            .is_none_or(|value: &String| value.trim().is_empty())
    {
        return Err("apply需要非空--plan-digest".into());
    }
    if !apply && digest.is_some() {
        return Err("preview不能提供--plan-digest".into());
    }
    let request = request.ok_or("source-lifecycle需要--request-json")?;
    let request = serde_json::from_value(worldline_core::parse_unique_json(request.as_bytes())?)
        .map_err(|error| format!("无效源码生命周期DTO：{error}"))?;
    Ok(Args {
        path: path.ok_or("source-lifecycle需要工程目录或入口")?,
        request,
        digest,
        json,
    })
}

fn output(out: &mut impl Write, json: bool, code: i32, payload: Value) -> Result<i32, String> {
    if json {
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else if code == 0 {
        writeln!(
            out,
            "源码生命周期{}：预览摘要 {}",
            if payload["saved"] == true {
                "已应用并保存"
            } else {
                "预览完成"
            },
            payload["plan"]["plan_digest"]
        )
        .map_err(|error| error.to_string())?;
    } else {
        writeln!(out, "{}", payload["error"]["message"]).map_err(|error| error.to_string())?;
        if payload["error"]["stage"] == "save" {
            writeln!(
                out,
                "内存操作已应用，保存未完成；磁盘可能部分写入，请重新打开工程恢复保存事务。"
            )
            .map_err(|error| error.to_string())?;
        }
    }
    Ok(code)
}

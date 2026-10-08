//! 显式模板草稿、预览与应用；仅--save进入既有保存事务。
use super::*;
use worldline_core::presentation_commands::Revision;
use worldline_core::project_templates::protocol::{
    parse_template_draft_request, parse_template_mutation_request,
};

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    match execute(args, out) {
        Ok(code) => Ok(code),
        Err(message) if args.iter().any(|a| a == "--json") => emit(
            json!({
                "ok":false,"error":{"code":"INVALID_ARGUMENT","message":message},
                "applied":false,"saved":false
            }),
            true,
            2,
            out,
        ),
        Err(message) => Err(message),
    }
}

fn execute(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let operation = args
        .first()
        .ok_or("template 需要 draft / preview / apply")?
        .as_str();
    if !matches!(operation, "draft" | "preview" | "apply") {
        return Err("template 操作仅支持 draft / preview / apply".into());
    }
    let path = args.get(1).ok_or("template 需要工程目录或入口")?;
    let mut request_json = None;
    let mut digest = None;
    let mut save = false;
    let mut json_output = false;
    let mut iter = args[2..].iter();
    while let Some(argument) = iter.next() {
        let (key, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(k, v)| (k, Some(v)));
        match key {
            "--json" if inline.is_none() && !json_output => json_output = true,
            "--save" if inline.is_none() && !save && operation == "apply" => save = true,
            "--request-json" | "--plan-digest" => {
                if key == "--plan-digest" && operation != "apply" {
                    return Err("仅apply接受--plan-digest".into());
                }
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数缺少值")?;
                let slot = if key == "--request-json" {
                    &mut request_json
                } else {
                    &mut digest
                };
                if slot.replace(value).is_some() {
                    return Err(format!("{key}不能重复"));
                }
            }
            _ => return Err(format!("template未知、重复或不适用参数：{argument}")),
        }
    }
    let request_json = request_json.ok_or("需要--request-json")?;
    if operation == "apply" && digest.is_none() {
        return Err("apply需要--plan-digest".into());
    }
    let mut project = match Project::open_read_only(Path::new(path)) {
        Ok(project) => project,
        Err(message) => {
            return emit(
                json!({"ok":false,"error":{"code":"IO_ERROR","message":message},
            "applied":false,"saved":false}),
                json_output,
                2,
                out,
            )
        }
    };
    let mut revision = Revision::default();
    let result = if operation == "draft" {
        let request = parse_template_draft_request(&request_json)?;
        project.template_draft_request(&request).map(|result| {
            let mut payload = serde_json::to_value(result).expect("模板结果可序列化");
            payload["ok"] = json!(true);
            payload
        })
    } else {
        let request = parse_template_mutation_request(&request_json)?;
        if let Some(digest) = digest {
            project
                .apply_template_request(&mut revision, &request, &digest)
                .map(|result| {
                    json!({"ok":true,"plan":result.plan,"changed_files":result.changed_files,
                    "applied":true,"saved":false})
                })
        } else {
            project
                .preview_template_request(revision, &request)
                .map(|plan| json!({"ok":plan.can_apply,"plan":plan,"applied":false,"saved":false}))
        }
    };
    let mut payload = result
        .unwrap_or_else(|error| json!({"ok":false,"error":error,"applied":false,"saved":false}));
    if save && payload["applied"] == true {
        match project.save() {
            Ok(()) => payload["saved"] = json!(true),
            Err(message) => {
                payload["ok"] = json!(false);
                payload["save_error"] = json!(message);
            }
        }
    }
    payload["baseline"] = json!(project.content_baseline());
    payload["revision"] = json!(revision);
    payload["operation"] = json!(operation);
    let code = if payload["ok"] == true { 0 } else { 1 };
    emit(payload, json_output, code, out)
}

fn emit(payload: Value, json_output: bool, code: i32, out: &mut impl Write) -> Result<i32, String> {
    let text = if json_output {
        payload.to_string()
    } else {
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
    };
    writeln!(out, "{text}").map_err(|e| e.to_string())?;
    Ok(code)
}

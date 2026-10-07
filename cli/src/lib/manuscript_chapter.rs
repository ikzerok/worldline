use super::*;
use worldline_core::{
    manuscript::parse_manuscript_chapter_create_request, presentation_commands::Revision,
};

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    match execute(args, out) {
        Ok(code) => Ok(code),
        Err(message) if args.iter().any(|arg| arg == "--json") => emit(
            json!({"ok":false,"applied":false,"saved":false,"error":{"code":"INVALID_ARGUMENT","message":message}}),
            true,
            2,
            out,
        ),
        Err(message) => Err(message),
    }
}
fn execute(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let apply = match args.first().map(String::as_str) {
        Some("preview") => false,
        Some("apply") => true,
        _ => return Err("manuscript-chapter 需要 preview 或 apply".into()),
    };
    let path = args.get(1).ok_or("manuscript-chapter 需要工程目录或入口")?;
    let mut request_json = None;
    let mut digest = None;
    let mut save = false;
    let mut json_output = false;
    let mut iter = args[2..].iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        match key {
            "--json" if inline.is_none() && !json_output => json_output = true,
            "--save" if inline.is_none() && !save => save = true,
            "--request-json" | "--plan-digest" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数缺少值")?;
                let slot = if key == "--request-json" {
                    &mut request_json
                } else {
                    &mut digest
                };
                if value.is_empty() || slot.replace(value).is_some() {
                    return Err(format!("{key} 不能为空或重复"));
                }
            }
            _ => return Err(format!("manuscript-chapter 未知或重复参数：{arg}")),
        }
    }
    if apply != digest.is_some() || (!apply && save) {
        return Err(
            "apply 必须提供 --plan-digest；preview 不能提供 --plan-digest 或 --save".into(),
        );
    }
    let request =
        parse_manuscript_chapter_create_request(&request_json.ok_or("需要 --request-json")?)?;
    let mut project = match Project::open_read_only(Path::new(path)) {
        Ok(project) => project,
        Err(message) => {
            return emit(
                json!({"ok":false,"applied":false,"saved":false,"stage":"open","error":{"code":"IO_ERROR","message":message}}),
                json_output,
                2,
                out,
            )
        }
    };
    let mut revision = Revision::default();
    let operation = if apply { "apply" } else { "preview" };
    let outcome = if let Some(digest) = digest {
        project.apply_manuscript_chapter_create(&mut revision, &request, &digest).map(|result| {
            json!({"ok":true,"operation":operation,"plan":result.plan,"result":result,"applied":true,"saved":false,"notice":"仅内存应用；CLI退出会丢弃候选，请使用 --save 持久化"})
        })
    } else {
        project.preview_manuscript_chapter_create(revision, &request)
            .map(|plan| json!({"ok":plan.can_apply,"operation":operation,"plan":plan,"applied":false,"saved":false}))
    };
    let mut payload = match outcome {
        Ok(value) => value,
        Err(error) => {
            json!({"ok":false,"operation":operation,"plan":null,"error":error,"applied":false,"saved":false})
        }
    };
    if save && payload["ok"] == true {
        match project.save() {
            Ok(()) => {
                payload["saved"] = json!(true);
                payload["notice"] = json!("已通过保存事务保存");
            }
            Err(message) => {
                payload["ok"] = json!(false);
                payload["stage"] = json!("save");
                payload["error"] = json!({"code":"SAVE_FAILED","message":message});
                payload["notice"] =
                    json!("内存已应用，保存失败；磁盘可能存在待恢复事务，请重新打开并处理");
            }
        }
    }
    payload["revision"] = json!(revision);
    payload["baseline"] = json!(project.content_baseline());
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

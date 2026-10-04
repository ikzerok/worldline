use super::*;
use std::io::Read;
use worldline_core::catalog_import::{parse_catalog_import_request, MAX_CSV_BYTES};

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    match execute(args, out) {
        Ok(code) => Ok(code),
        Err(message) if args.iter().any(|argument| argument == "--json") => {
            writeln!(out, "{}", json!({"ok":false,"saved":false,"error":{"code":"INVALID_ARGUMENT","message":message}})).map_err(|error| error.to_string())?;
            Ok(2)
        }
        Err(message) => Err(message),
    }
}
fn execute(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let apply = match args.first().map(String::as_str) {
        Some("preview") => false,
        Some("apply") => true,
        _ => {
            return Err(
                "catalog-import需要preview或apply；apply默认只改内存，请加--save持久化".into(),
            )
        }
    };
    let path = args.get(1).ok_or("catalog-import需要工程目录")?;
    let mut request_json = None;
    let mut csv_path = None;
    let mut digest = None;
    let mut save = false;
    let mut json_output = false;
    let mut iter = args[2..].iter();
    while let Some(argument) = iter.next() {
        let (key, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(key, value)| (key, Some(value)));
        match key {
            "--json" if inline.is_none() && !json_output => json_output = true,
            "--save" if inline.is_none() && !save => save = true,
            "--request-json" | "--csv" | "--plan-digest" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数缺少值")?;
                let slot = match key {
                    "--request-json" => &mut request_json,
                    "--csv" => &mut csv_path,
                    _ => &mut digest,
                };
                if slot.replace(value).is_some() {
                    return Err(format!("{key}不能重复"));
                }
            }
            _ => return Err(format!("catalog-import未知或重复参数：{argument}")),
        }
    }
    if apply != digest.is_some() {
        return Err("apply必须提供--plan-digest，preview不能提供".into());
    }
    if save && !apply {
        return Err("--save仅apply可用".into());
    }
    let mut request = parse_catalog_import_request(&request_json.ok_or("需要--request-json")?)?;
    if let Some(path) = csv_path {
        match read_csv(&path) {
            Ok(csv) => request.csv = csv,
            Err(message) => {
                return emit(
                    json!({"ok":false,"saved":false,"applied":false,"stage":"input","error":{"code":"CSV_INPUT_REJECTED","message":message}}),
                    json_output,
                    out,
                )
            }
        }
    }
    let mut project = match Project::open_read_only(Path::new(path)) {
        Ok(project) => project,
        Err(message) => {
            return emit(
                json!({"ok":false,"saved":false,"applied":false,"stage":"load","error":{"code":"WORKSPACE_READ_REJECTED","message":message}}),
                json_output,
                out,
            )
        }
    };
    let outcome = if apply {
        project.apply_catalog_import(&request, digest.as_deref().unwrap()).map(|result| {
            let mut payload=json!({"ok":true,"operation":"apply","applied":true,"stage":"apply","plan":result.plan,"changed_files":result.changed_files,"new_baseline":result.new_baseline,"saved":false,"notice":"仅内存应用；CLI退出时将丢弃候选，请使用--save持久化"});
            if save {
                payload["stage"]=json!("save");
                match project.save() {
                    Ok(()) => { payload["saved"]=json!(true); payload["notice"]=json!("已通过保存事务保存"); }
                    Err(message) => {
                        payload["ok"]=json!(false);
                        payload["error"]=json!({"code":"SAVE_FAILED","message":message});
                        payload["notice"]=json!("内存已应用，保存失败；磁盘可能存在待恢复事务，请重新打开并处理，不能认作零磁盘写入");
                    }
                }
            }
            payload
        })
    } else {
        project.preview_catalog_import(&request).map(|plan| {
            let mut payload=json!({"ok":plan.can_apply,"operation":"preview","plan":plan,"saved":false,"applied":false,"stage":"preview"});
            if !plan.can_apply { payload["error"]=json!({"code":"CATALOG_IMPORT_BLOCKED","message":"资料导入存在阻断诊断，请查看完整计划"}); }
            payload
        })
    };
    let payload = match outcome {
        Ok(payload) => payload,
        Err(message) => {
            json!({"ok":false,"saved":false,"applied":false,"stage":if apply {"apply"} else {"preview"},"error":{"code":"CATALOG_IMPORT_REJECTED","message":message}})
        }
    };
    emit(payload, json_output, out)
}
fn read_csv(path: &str) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("CSV输入必须是普通文件".into());
    }
    if metadata.len() > MAX_CSV_BYTES as u64 {
        return Err("CSV超过2 MiB预算".into());
    }
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("CSV输入必须是普通文件".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CSV_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_CSV_BYTES {
        return Err("CSV超过2 MiB预算".into());
    }
    String::from_utf8(bytes).map_err(|_| "CSV必须为UTF-8".into())
}
fn emit(payload: Value, json_output: bool, out: &mut impl Write) -> Result<i32, String> {
    if json_output {
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(if payload["ok"] == true { 0 } else { 1 })
}

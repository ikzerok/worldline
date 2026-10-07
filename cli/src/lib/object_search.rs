use super::*;
use worldline_core::object_search::{ObjectSearchError, ObjectSearchFilter, ObjectSearchOptions};

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    match execute(args, out) {
        Ok(code) => Ok(code),
        Err(message) if args.iter().any(|arg| arg == "--json") => emit(
            json!({"ok":false,"page":null,"error":{"code":"INVALID_ARGUMENT","message":message}}),
            true,
            2,
            out,
        ),
        Err(message) => Err(message),
    }
}
fn execute(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    let path = args.first().ok_or("object-search 需要工程目录或入口")?;
    let mut query = None;
    let mut filter_json = None;
    let mut options_json = None;
    let mut expected = None;
    let mut json_output = false;
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        match key {
            "--json" if inline.is_none() && !json_output => json_output = true,
            "--query" | "--filter-json" | "--options-json" | "--expected-baseline" => {
                let value = inline
                    .map(str::to_owned)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数缺少值")?;
                let slot = match key {
                    "--query" => &mut query,
                    "--filter-json" => &mut filter_json,
                    "--options-json" => &mut options_json,
                    _ => &mut expected,
                };
                if slot.replace(value).is_some() {
                    return Err(format!("{key} 不能重复"));
                }
            }
            _ => return Err(format!("object-search 未知或重复参数：{arg}")),
        }
    }
    let query = query.ok_or("需要 --query（空字符串表示全部）")?;
    if query
        .len()
        .saturating_add(filter_json.as_ref().map_or(0, String::len))
        .saturating_add(options_json.as_ref().map_or(0, String::len))
        .saturating_add(expected.as_ref().map_or(0, String::len))
        > 64 * 1024
    {
        return Err("对象检索参数超过 64 KiB".into());
    }
    let decode = |text: &str| worldline_core::parse_unique_json(text.as_bytes());
    let filter: ObjectSearchFilter = filter_json
        .as_deref()
        .map(decode)
        .transpose()?
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let options: ObjectSearchOptions = options_json
        .as_deref()
        .map(decode)
        .transpose()?
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let project = match Project::open_read_only(Path::new(path)) {
        Ok(project) => project,
        Err(message) => {
            return emit(
                json!({"ok":false,"page":null,"error":{"code":"IO_ERROR","message":message}}),
                json_output,
                2,
                out,
            )
        }
    };
    let baseline = project.content_baseline();
    if expected.is_some_and(|value| value != baseline) {
        return emit(
            json!({"ok":false,"page":null,"baseline":baseline,"error":{"code":"STALE_BASELINE","message":"对象检索基线已过期，请重新查询"}}),
            json_output,
            1,
            out,
        );
    }
    let content = project.compile_object_search_snapshot();
    let mut payload = json!({"schema_version":1,"baseline":baseline,"language_version":project.language_version(),"snapshot":"applied",
        "diagnostics":content.diagnostics,"workspace_diagnostics":project.authoring_diagnostics(),"read_only":!project.authoring_diagnostics().is_empty()});
    match content
        .analysis
        .catalog
        .search_objects_filtered_page(&query, &filter, options)
    {
        Ok(page) => {
            payload["ok"] = json!(!content.has_errors());
            payload["page"] = json!(page);
            if content.has_errors() {
                payload["error"] = json!({"code":"INVALID_SOURCE","message":"当前已应用源码存在错误；目录页不代表完整可解析工程"});
            }
        }
        Err(error) => {
            payload["ok"] = json!(false);
            payload["page"] = Value::Null;
            payload["error"] = json!({"code":error_code(&error),"message":error.to_string()});
        }
    }
    let code = if payload["ok"] == true { 0 } else { 1 };
    emit(payload, json_output, code, out)
}
fn error_code(error: &ObjectSearchError) -> &'static str {
    match error {
        ObjectSearchError::InvalidLimit { .. } => "INVALID_LIMIT",
        ObjectSearchError::InvalidCandidateBudget { .. } => "INVALID_CANDIDATE_BUDGET",
        ObjectSearchError::CandidateBudgetExceeded { .. } => "CANDIDATE_BUDGET_EXCEEDED",
        ObjectSearchError::InvalidOffset { .. } => "INVALID_OFFSET",
    }
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

//! 只读书稿查询CLI；不恢复事务、不刷新、不应用、不保存。
use super::*;
use worldline_core::manuscript::{
    parse_manuscript_query_drafts, parse_manuscript_query_request, ManuscriptQueryDraft,
    ManuscriptQueryRequest, MAX_MANUSCRIPT_QUERY_INPUT_BYTES,
};
const HELP: &str = "用法: wl manuscript-query <目录或入口> --query-json JSON [--drafts-json JSON] [--json]\nquery为schema_version:1的核心DTO；drafts为绑定工程基线的完整编排草稿数组。只读，不保存。";

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        writeln!(out, "{HELP}").map_err(|error| error.to_string())?;
        return Ok(0);
    }
    let (path, query, drafts, json_mode) = match parse(args) {
        Ok(value) => value,
        Err(message) => return emit(out, failure("INVALID_ARGUMENT", message), true, 2),
    };
    let project = match Project::open_read_only(&path) {
        Ok(project) => project,
        Err(message) => return emit(out, failure("IO_ERROR", message), json_mode, 2),
    };
    let outcome = project
        .manuscript_query_snapshot(&[], &drafts)
        .and_then(|snapshot| snapshot.query(&query));
    let payload = match outcome {
        Ok(page) => {
            json!({"ok":page.complete,"page":page,"error":if page.complete { Value::Null } else {
            json!({"code":"INCOMPLETE_SNAPSHOT","message":"书稿快照未完整确认；页面仅表示可识别范围，请核对诊断"})
        },"applied":false,"saved":false,"baseline":project.content_baseline()})
        }
        Err(error) => json!({"ok":false,"page":null,"error":error,"applied":false,"saved":false,
            "baseline":project.content_baseline()}),
    };
    let code = if payload["ok"] == true { 0 } else { 1 };
    emit(out, payload, json_mode, code)
}

type Parsed = (
    PathBuf,
    ManuscriptQueryRequest,
    Vec<ManuscriptQueryDraft>,
    bool,
);
fn parse(args: &[String]) -> Result<Parsed, String> {
    if args.iter().map(|arg| arg.len()).sum::<usize>() > MAX_MANUSCRIPT_QUERY_INPUT_BYTES {
        return Err("书稿查询请求超过4 MiB预算；请求未截断".into());
    }
    let mut path = None;
    let mut query = None;
    let mut drafts = None;
    let mut json_mode = false;
    let mut iter = args.iter();
    while let Some(argument) = iter.next() {
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(flag, value)| {
                (flag, Some(value))
            });
        match flag {
            "--json" if inline.is_none() && !json_mode => json_mode = true,
            "--query-json" | "--drafts-json" => {
                let raw = inline
                    .map(str::to_owned)
                    .or_else(|| iter.next().cloned())
                    .ok_or("JSON参数缺少值")?;
                let slot = if flag == "--query-json" {
                    &mut query
                } else {
                    &mut drafts
                };
                if slot.replace(raw).is_some() {
                    return Err(format!("{flag}不能重复"));
                }
            }
            _ if !argument.starts_with('-') && path.is_none() => {
                path = Some(PathBuf::from(argument))
            }
            _ => return Err(format!("未知或重复的书稿查询参数：{argument}")),
        }
    }
    Ok((
        path.ok_or(HELP)?,
        parse_manuscript_query_request(&query.ok_or("需要--query-json")?)?,
        parse_manuscript_query_drafts(drafts.as_deref().unwrap_or("[]"))?,
        json_mode,
    ))
}
fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"page":null,"error":{"code":code,"message":message.into()},"applied":false,"saved":false})
}
fn emit(out: &mut impl Write, payload: Value, json_mode: bool, code: i32) -> Result<i32, String> {
    let bytes = if json_mode {
        serde_json::to_vec(&payload)
    } else {
        serde_json::to_vec_pretty(&payload)
    }
    .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_MANUSCRIPT_QUERY_INPUT_BYTES {
        serde_json::to_writer(
            &mut *out,
            &failure(
                "BUDGET_EXCEEDED",
                "书稿查询响应超过4 MiB预算；页面未截断，请缩小页或查询范围",
            ),
        )
        .map_err(|error| error.to_string())?;
        writeln!(out).map_err(|error| error.to_string())?;
        return Ok(1);
    }
    out.write_all(&bytes)
        .and_then(|_| writeln!(out))
        .map_err(|error| error.to_string())?;
    Ok(code)
}

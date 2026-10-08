//! 同范围作者审稿CLI；只有显式output才向新目标交付Markdown。
use super::*;
use worldline_core::manuscript::{
    generate_manuscript_delivery, parse_manuscript_delivery_request, parse_manuscript_query_drafts,
    write_manuscript_markdown_new, ManuscriptDeliveryRequest, ManuscriptQueryDraft,
    MAX_MANUSCRIPT_DELIVERY_RESPONSE_BYTES, MAX_MANUSCRIPT_QUERY_INPUT_BYTES,
};
const HELP: &str = "用法: wl manuscript-delivery <目录或入口> --request-json JSON [--drafts-json JSON] [--output /新路径/作者审稿.md] [--json]\n作者私密静态全分支；query使用既有书稿筛选DTO。未执行、未应用、未保存工程。output只能是工作区外全新.md。";

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        writeln!(out, "{HELP}").map_err(|error| error.to_string())?;
        return Ok(0);
    }
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(error) => return emit(out, failure("INVALID_ARGUMENT", error), 2),
    };
    let project = match Project::open_read_only(&parsed.path) {
        Ok(project) => project,
        Err(error) => return emit(out, failure("IO_ERROR", error), 2),
    };
    let report = match project
        .manuscript_delivery_snapshot(&[], &parsed.drafts, &parsed.request)
        .and_then(|snapshot| generate_manuscript_delivery(snapshot, &mut |_| true))
    {
        Ok(report) => report,
        Err(error) => return emit(out, failure(&error.code, error.message), 1),
    };
    if let Some(path) = &parsed.output {
        let delivered = project
            .validate_manuscript_delivery(&[], &parsed.drafts, &report)
            .map_err(|error| error.to_string())
            .and_then(|()| {
                write_manuscript_markdown_new(&project.root, path, &report, &mut || {
                    project
                        .validate_manuscript_delivery(&[], &parsed.drafts, &report)
                        .map_err(|error| error.to_string())
                })
            });
        if let Err(error) = delivered {
            return emit(out, failure("DELIVERY_FAILED", error), 1);
        }
    }
    let complete = report.complete();
    emit(
        out,
        json!({"ok":complete,"report":report,"error":if complete { Value::Null } else {
        json!({"code":"INCOMPLETE_DELIVERY","message":"范围或章节未完整确认；Markdown没有交付"})
    },"applied":false,"saved":false,"delivered":parsed.output.is_some()}),
        if complete { 0 } else { 1 },
    )
}
struct Parsed {
    path: PathBuf,
    request: ManuscriptDeliveryRequest,
    drafts: Vec<ManuscriptQueryDraft>,
    output: Option<PathBuf>,
}
fn parse(args: &[String]) -> Result<Parsed, String> {
    if args.iter().map(String::len).sum::<usize>() > MAX_MANUSCRIPT_QUERY_INPUT_BYTES {
        return Err("交付参数超过4MiB预算".into());
    }
    let (mut path, mut request, mut drafts, mut output) = (None, None, None, None);
    let mut json_mode = false;
    let mut iter = args.iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "--json" if !json_mode => json_mode = true,
            "--request-json" if request.is_none() => {
                request = Some(parse_manuscript_delivery_request(iter.next().ok_or(HELP)?)?)
            }
            "--drafts-json" if drafts.is_none() => {
                drafts = Some(parse_manuscript_query_drafts(iter.next().ok_or(HELP)?)?)
            }
            "--output" if output.is_none() => {
                output = Some(PathBuf::from(iter.next().ok_or(HELP)?))
            }
            value if !value.starts_with('-') && path.is_none() => path = Some(PathBuf::from(value)),
            _ => return Err(format!("未知或重复参数：{argument}\n{HELP}")),
        }
    }
    Ok(Parsed {
        path: path.ok_or(HELP)?,
        request: request.ok_or("需要--request-json")?,
        drafts: drafts.unwrap_or_default(),
        output,
    })
}
fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"report":null,"error":{"code":code,"message":message.into()},"applied":false,"saved":false,"delivered":false})
}
fn emit(out: &mut impl Write, value: Value, code: i32) -> Result<i32, String> {
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_MANUSCRIPT_DELIVERY_RESPONSE_BYTES {
        serde_json::to_writer(
            &mut *out,
            &failure("BUDGET_EXCEEDED", "交付响应超过32MiB预算；未截断"),
        )
        .map_err(|error| error.to_string())?;
        writeln!(out).map_err(|error| error.to_string())?;
        return Ok(1);
    }
    out.write_all(&bytes)
        .and_then(|()| writeln!(out))
        .map_err(|error| error.to_string())?;
    Ok(code)
}

//! 私密制作材料；core 决定范围、身份、locale 和完整字节。
use super::dialogue::{emit, failure, fits, MAX_RESPONSE};
use super::*;
use worldline_core::manuscript::{parse_manuscript_query_drafts, ManuscriptQueryDraft};
use worldline_core::production_script::{
    parse_production_export_options, parse_production_script_request, write_production_script_new,
    ProductionExportOptions, ProductionScriptRequest,
};

const HELP: &str = "用法: wl production-script query PROJECT --request-json JSON [--drafts-json JSON] [--offset N] [--limit N] [--json]\n       wl production-script export PROJECT --request-json JSON --options-json JSON [--drafts-json JSON] [--output /新文件] [--json]\n作者本地私密制作材料；不执行、不应用、不保存工程或发送第三方。direction 默认排除。CSV带显示前缀，精确值用JSON，不保证所有表格软件安全。";
const MAX_INPUT: usize = 4 * 1024 * 1024;

struct Args {
    path: PathBuf,
    request: ProductionScriptRequest,
    drafts: Vec<ManuscriptQueryDraft>,
    options: Option<ProductionExportOptions>,
    output: Option<PathBuf>,
    offset: usize,
    limit: usize,
}

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        writeln!(out, "{HELP}").map_err(|error| error.to_string())?;
        return Ok(0);
    }
    let parsed = match parse(args) {
        Ok(value) => value,
        Err(message) => return emit(out, failure("INVALID_PARAMS", message), 2),
    };
    let project = match Project::open_read_only(&parsed.path) {
        Ok(project) => project,
        Err(message) => return emit(out, failure("IO_ERROR", message), 2),
    };
    let snapshot = match project.production_script_snapshot(&[], &parsed.drafts, &parsed.request) {
        Ok(value) => value,
        Err(error) => return emit(out, failure(&error.code, error.message), 1),
    };
    let Some(options) = &parsed.options else {
        return match snapshot.page(parsed.offset, parsed.limit) {
            Ok(page) => emit(
                out,
                json!({"ok":true,"page":page,"baseline":project.content_baseline(),
                "applied":false,"saved":false,"delivered":false,"error":null}),
                0,
            ),
            Err(error) => emit(out, failure(&error.code, error.message), 1),
        };
    };
    let artifact = match snapshot.export(options) {
        Ok(value) => value,
        Err(error) => return emit(out, failure(&error.code, error.message), 1),
    };
    let text = std::str::from_utf8(artifact.bytes()).map_err(|error| error.to_string())?;
    let payload = json!({"ok":true,"artifact":{"format":artifact.format(),"snapshot_key":artifact.snapshot_key(),
        "text":text,"byte_count":artifact.bytes().len()},"baseline":project.content_baseline(),
        "applied":false,"saved":false,"delivered":parsed.output.is_some(),"error":null});
    if !fits(&payload, MAX_RESPONSE) {
        return emit(
            out,
            failure("BUDGET_EXCEEDED", "完整台本响应超过64MiB；未创建文件"),
            1,
        );
    }
    if let Some(path) = &parsed.output {
        let result = project
            .validate_production_script(&[], &parsed.drafts, &snapshot)
            .map_err(|error| error.to_string())
            .and_then(|()| {
                write_production_script_new(&project.root, path, &artifact, &mut || {
                    project
                        .validate_production_script(&[], &parsed.drafts, &snapshot)
                        .map_err(|error| error.to_string())
                })
            });
        if let Err(message) = result {
            return emit(out, failure("DELIVERY_FAILED", message), 1);
        }
    }
    emit(out, payload, 0)
}

fn parse(args: &[String]) -> Result<Args, String> {
    if args
        .iter()
        .try_fold(0usize, |sum, arg| sum.checked_add(arg.len()))
        .is_none_or(|sum| sum > MAX_INPUT)
    {
        return Err("台本参数超过4MiB预算".into());
    }
    let operation = args
        .first()
        .filter(|arg| matches!(arg.as_str(), "query" | "export"))
        .ok_or(HELP)?;
    let (mut path, mut request, mut drafts, mut options, mut output) =
        (None, None, None, None, None);
    let (mut offset, mut limit, mut json_mode) = (None, None, false);
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" if !json_mode => json_mode = true,
            "--request-json" if request.is_none() => {
                request = Some(parse_production_script_request(iter.next().ok_or(HELP)?)?)
            }
            "--drafts-json" if drafts.is_none() => {
                drafts = Some(parse_manuscript_query_drafts(iter.next().ok_or(HELP)?)?)
            }
            "--options-json" if options.is_none() => {
                options = Some(parse_production_export_options(iter.next().ok_or(HELP)?)?)
            }
            "--output" if output.is_none() => {
                output = Some(PathBuf::from(
                    iter.next().filter(|value| !value.is_empty()).ok_or(HELP)?,
                ))
            }
            "--offset" if offset.is_none() => offset = Some(number(iter.next().ok_or(HELP)?)?),
            "--limit" if limit.is_none() => limit = Some(number(iter.next().ok_or(HELP)?)?),
            value if !value.starts_with('-') && path.is_none() && !value.is_empty() => {
                path = Some(PathBuf::from(value))
            }
            _ => return Err(format!("未知或重复参数：{arg}\n{HELP}")),
        }
    }
    if (operation == "query" && (options.is_some() || output.is_some()))
        || (operation == "export" && (options.is_none() || offset.is_some() || limit.is_some()))
    {
        return Err(HELP.into());
    }
    Ok(Args {
        path: path.ok_or(HELP)?,
        request: request.ok_or("需要 --request-json")?,
        drafts: drafts.unwrap_or_default(),
        options,
        output,
        offset: offset.unwrap_or(0),
        limit: limit.unwrap_or(50),
    })
}

fn number(value: &str) -> Result<usize, String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("分页参数必须为非负十进制整数".into());
    }
    value.parse().map_err(|_| "分页整数超过支持范围".into())
}

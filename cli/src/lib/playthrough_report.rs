//! 只读试玩报告入口；正文、来源与 Markdown 均来自 runtime 的验证结果。
use super::*;
use std::collections::BTreeSet;
use worldline_runtime::{
    generate_playthrough_report, PlaythroughReportOptions, ReplayCancellation, RouteStatus,
};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;
const HELP: &str = "用法: wl playthrough-report <目录或入口> --trace-json '<DTO>' [--max-steps N] [--time-budget-ms N] [--json]\n只读验证当前已应用稿；含作者私密内容，导出前请核对分享范围。";

struct Args {
    path: PathBuf,
    trace: ReplayTrace,
    options: PlaythroughReportOptions,
    json: bool,
}

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        writeln!(out, "{HELP}").map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let json_mode = args.iter().any(|arg| arg == "--json");
    let args = match parse(args) {
        Ok(args) => args,
        Err(message) => return failure(json_mode, "INVALID_PARAMS", &message, 2, out),
    };
    let project = match Project::open_read_only(&args.path) {
        Ok(project) => project,
        Err(message) => return failure(args.json, "IO_ERROR", &message, 2, out),
    };
    let snapshot = match project.compile_read_only() {
        Ok(snapshot) => snapshot,
        Err(message) => {
            return diagnostic_failure(
                args.json,
                "invalid_snapshot",
                &message,
                &[],
                project.authoring_diagnostics(),
                out,
            );
        }
    };
    if snapshot.has_errors() {
        return diagnostic_failure(
            args.json,
            "COMPILE_FAILED",
            "当前稿件编译失败",
            &snapshot.diagnostics,
            project.authoring_diagnostics(),
            out,
        );
    }
    match generate_playthrough_report(
        &snapshot,
        &args.trace,
        args.options,
        &ReplayCancellation::new(),
    ) {
        Ok(report) => {
            let ok = report.status == RouteStatus::Replayed;
            // runtime 在构造 DTO 前已检查其 1 MiB 上限；这里只加有界机器外壳。
            write_response(
                args.json,
                json!({"ok":ok,"report":report}),
                i32::from(!ok),
                out,
            )
        }
        Err(error) => {
            let code = if matches!(
                error.code.as_str(),
                "invalid_options" | "input_limit" | "invalid_trace"
            ) {
                2
            } else {
                1
            };
            failure(args.json, &error.code, &error.message, code, out)
        }
    }
}

fn failure(
    json_mode: bool,
    code: &str,
    message: &str,
    exit_code: i32,
    out: &mut impl Write,
) -> Result<i32, String> {
    let skeleton = json!({"ok":false,"report":null,"error":{"code":null,"message":null}});
    let overhead = serde_json::to_vec(&skeleton).unwrap().len() - 8;
    let mut counter = Counter(MAX_RESPONSE_BYTES - 1 - overhead);
    if serde_json::to_writer(&mut counter, code)
        .and_then(|_| serde_json::to_writer(&mut counter, message))
        .is_err()
    {
        return failure(
            json_mode,
            "output_limit",
            "试玩报告响应超过字节限制",
            1,
            out,
        );
    }
    write_response(
        json_mode,
        json!({"ok":false,"report":null,"error":{"code":code,"message":message}}),
        exit_code,
        out,
    )
}
fn diagnostic_failure(
    json_mode: bool,
    code: &str,
    message: &str,
    diagnostics: &[Diagnostic],
    workspace: &[Diagnostic],
    out: &mut impl Write,
) -> Result<i32, String> {
    // 先流式编码借用的集合，不为判断预算先建立诊断 Value 副本。
    let skeleton = json!({"ok":false,"report":null,"error":{"code":null,"message":null},
        "diagnostics":null,"workspace_diagnostics":null});
    let overhead = serde_json::to_vec(&skeleton).unwrap().len() - 16;
    let mut counter = Counter(MAX_RESPONSE_BYTES - 1 - overhead);
    if serde_json::to_writer(&mut counter, code)
        .and_then(|_| serde_json::to_writer(&mut counter, message))
        .and_then(|_| serde_json::to_writer(&mut counter, diagnostics))
        .and_then(|_| serde_json::to_writer(&mut counter, workspace))
        .is_err()
    {
        return failure(
            json_mode,
            "output_limit",
            "试玩报告响应超过字节限制",
            1,
            out,
        );
    }
    write_response(
        json_mode,
        json!({"ok":false,"report":null,"error":{"code":code,"message":message},
        "diagnostics":diagnostics,"workspace_diagnostics":workspace}),
        1,
        out,
    )
}
struct Counter(usize);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0 {
            return Err(std::io::Error::other("JSON超额"));
        }
        self.0 -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn write_response(
    json_mode: bool,
    value: Value,
    exit_code: i32,
    out: &mut impl Write,
) -> Result<i32, String> {
    let mut buffer = BoundedBuffer(Vec::new());
    if serde_json::to_writer(&mut buffer, &value).is_err() {
        return failure(
            json_mode,
            "output_limit",
            "试玩报告响应超过字节限制",
            1,
            out,
        );
    }
    if json_mode {
        out.write_all(&buffer.0)
            .and_then(|_| writeln!(out))
            .map_err(|e| e.to_string())?;
    } else if let Some(markdown) = value["report"]["markdown"].as_str() {
        out.write_all(markdown.as_bytes())
            .map_err(|e| e.to_string())?;
        if !markdown.ends_with('\n') {
            writeln!(out).map_err(|e| e.to_string())?;
        }
    } else {
        writeln!(
            out,
            "试玩报告失败 [{}]: {}",
            value["error"]["code"], value["error"]["message"]
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(exit_code)
}
struct BoundedBuffer(Vec<u8>);
impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) >= MAX_RESPONSE_BYTES {
            return Err(std::io::Error::other("响应超额"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut path = None;
    let mut trace = None;
    let mut json = false;
    let mut options = PlaythroughReportOptions::default();
    let mut seen = BTreeSet::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        if !key.starts_with('-') {
            if path.replace(PathBuf::from(arg)).is_some() {
                return Err("只能提供一个工程目录或入口".into());
            }
            continue;
        }
        if !seen.insert(key) {
            return Err(format!("参数 {key} 不能重复"));
        }
        if key == "--json" && inline.is_none() {
            json = true;
            continue;
        }
        if !matches!(key, "--trace-json" | "--max-steps" | "--time-budget-ms") {
            return Err("含未知playthrough-report参数".into());
        }
        let value = inline
            .or_else(|| iter.next().map(String::as_str))
            .ok_or_else(|| format!("参数 {key} 缺少值"))?;
        match key {
            "--trace-json" => {
                if value.len() > options.max_trace_bytes {
                    return Err("trace JSON 超过4 MiB输入限制".into());
                }
                let value = worldline_core::parse_unique_json(value.as_bytes())
                    .map_err(|e| format!("trace JSON无效：{e}"))?;
                trace =
                    Some(serde_json::from_value(value).map_err(|e| format!("trace DTO无效：{e}"))?);
            }
            "--max-steps" => {
                options.budget.max_steps = value.parse().map_err(|_| "max-steps必须是非负整数")?
            }
            "--time-budget-ms" => {
                options.budget.time_budget_ms =
                    value.parse().map_err(|_| "time-budget-ms必须是非负整数")?
            }
            _ => unreachable!(),
        }
    }
    options.validate().map_err(|e| e.message)?;
    Ok(Args {
        path: path.ok_or("playthrough-report需要工程目录或入口")?,
        trace: trace.ok_or("playthrough-report需要--trace-json")?,
        options,
        json,
    })
}

//! 只读路线对照入口；语义与来源均来自同一次 runtime pair 调用。
use super::*;
use std::collections::BTreeSet;
use worldline_runtime::{compare_routes, ReplayCancellation, RouteComparisonOptions, RouteStatus};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;
const HELP: &str = "用法: wl route-compare <目录或入口> --left-trace-json '<DTO>' --right-trace-json '<DTO>' [--max-steps N] [--time-budget-ms N] [--json]\n只读比较同一当前稿件；同步调用受两侧合计预算限制。";

struct Args {
    path: PathBuf,
    left: ReplayTrace,
    right: ReplayTrace,
    options: RouteComparisonOptions,
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
        Err(message) => return failure(args.json, "invalid_snapshot", &message, 1, out),
    };
    if snapshot.has_errors() {
        return write_response(
            args.json,
            json!({"ok":false,"comparison":null,"error":{"code":"COMPILE_FAILED","message":"当前稿件编译失败"},
                "diagnostics":snapshot.diagnostics,"workspace_diagnostics":project.authoring_diagnostics()}),
            1,
            out,
        );
    }
    match compare_routes(&snapshot, &args.left, &args.right, args.options, &ReplayCancellation::new()) {
        Ok(comparison) => {
            let ok = comparison.left.status == RouteStatus::Replayed
                && comparison.right.status == RouteStatus::Replayed;
            // runtime 在构造 DTO 前已检查其 1 MiB 上限；这里只加有界机器外壳。
            write_response(args.json, json!({"ok":ok,"comparison":comparison}), i32::from(!ok), out)
        }
        Err(error) => {
            let code = if matches!(error.code.as_str(), "invalid_options" | "input_limit" | "invalid_trace") { 2 } else { 1 };
            failure(args.json, &error.code, &error.message, code, out)
        }
    }
}

fn failure(json_mode: bool, code: &str, message: &str, exit_code: i32, out: &mut impl Write) -> Result<i32, String> {
    write_response(json_mode, json!({"ok":false,"comparison":null,"error":{"code":code,"message":message}}), exit_code, out)
}
fn write_response(json_mode: bool, value: Value, exit_code: i32, out: &mut impl Write) -> Result<i32, String> {
    let mut buffer = BoundedBuffer(Vec::new());
    if serde_json::to_writer(&mut buffer, &value).is_err() {
        return failure(json_mode, "output_limit", "路线对照响应超过字节限制", 1, out);
    }
    if json_mode {
        out.write_all(&buffer.0).and_then(|_| writeln!(out)).map_err(|e| e.to_string())?;
    } else if value["comparison"].is_object() {
        writeln!(out, "左侧：{}；完整记录：{}；已结束：{}\n右侧：{}；完整记录：{}；已结束：{}\n首个不同选择：{}",
            value["comparison"]["left"]["status"], value["comparison"]["left"]["complete"], value["comparison"]["left"]["ended"],
            value["comparison"]["right"]["status"], value["comparison"]["right"]["complete"], value["comparison"]["right"]["ended"],
            value["comparison"]["alignment"]["first_difference"])
            .map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "路线对照失败 [{}]: {}", value["error"]["code"], value["error"]["message"]).map_err(|e| e.to_string())?;
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
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut path = None;
    let mut left = None;
    let mut right = None;
    let mut json = false;
    let mut options = RouteComparisonOptions::default();
    let mut seen = BTreeSet::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg.split_once('=').map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        if !key.starts_with('-') {
            if path.replace(PathBuf::from(arg)).is_some() { return Err("只能提供一个工程目录或入口".into()); }
            continue;
        }
        if !seen.insert(key) { return Err(format!("参数 {key} 不能重复")); }
        if key == "--json" && inline.is_none() { json = true; continue; }
        if !matches!(key, "--left-trace-json" | "--right-trace-json" | "--max-steps" | "--time-budget-ms") {
            return Err(format!("未知参数 {key}"));
        }
        let value = inline.or_else(|| iter.next().map(String::as_str)).ok_or_else(|| format!("参数 {key} 缺少值"))?;
        match key {
            "--left-trace-json" | "--right-trace-json" => {
                if value.len() > options.max_trace_bytes { return Err("trace JSON 超过4 MiB输入限制".into()); }
                let value = worldline_core::parse_unique_json(value.as_bytes()).map_err(|e| format!("trace JSON无效：{e}"))?;
                let trace = serde_json::from_value(value).map_err(|e| format!("trace DTO无效：{e}"))?;
                if key == "--left-trace-json" { left = Some(trace); } else { right = Some(trace); }
            }
            "--max-steps" => options.budget.max_steps = value.parse().map_err(|_| "max-steps必须是非负整数")?,
            "--time-budget-ms" => options.budget.time_budget_ms = value.parse().map_err(|_| "time-budget-ms必须是非负整数")?,
            _ => unreachable!(),
        }
    }
    options.validate().map_err(|e| e.message)?;
    Ok(Args {
        path: path.ok_or("route-compare需要工程目录或入口")?,
        left: left.ok_or("route-compare需要--left-trace-json")?,
        right: right.ok_or("route-compare需要--right-trace-json")?,
        options, json,
    })
}

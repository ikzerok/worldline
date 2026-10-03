use super::*;
use std::collections::BTreeSet;
use worldline_core::problems::{ProblemCursor, ProblemQuery, ProblemsOptions, ProblemsReport};

const HELP: &str = "用法: wl problems <目录或入口> [--query-json '<JSON>'] [--cursor-json '<JSON>'] [--limit N] [--options-json '<JSON>'] [--related ID] [--json]\n只读工程问题快照；默认每页50条，最多200条。--related不能搭配非空查询；ID须从page.entries[].id完整复制，旧报告ID不可沿用。";

struct Args {
    path: PathBuf,
    query: ProblemQuery,
    cursor: Option<ProblemCursor>,
    options: ProblemsOptions,
    limit: usize,
    related: Option<String>,
    json: bool,
}

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        writeln!(out, "{HELP}").map_err(|error| error.to_string())?;
        return Ok(0);
    }
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            return failure(
                args.iter().any(|arg| arg == "--json"),
                "INVALID_PARAMS",
                &error,
                out,
            )
        }
    };
    let project = match Project::open(&parsed.path) {
        Ok(project) => project,
        Err(error) => return failure(parsed.json, "IO_ERROR", &error, out),
    };
    // One build; filtering and either page kind only read this report.
    let report = match project.problems_report(&parsed.options) {
        Ok(report) => report,
        Err(error) => return failure(parsed.json, &error.code, &error.message, out),
    };
    let mut limit = parsed.limit;
    let (page, payload) = loop {
        let page = match parsed.related.as_deref() {
            Some(id) => report
                .related_page(id, parsed.cursor.as_ref(), limit)
                .map(|page| json!(page)),
            None => report
                .query(&parsed.query, parsed.cursor.as_ref(), limit)
                .map(|page| json!(page)),
        };
        let page = match page {
            Ok(page) => page,
            Err(error) => return failure(parsed.json, &error.code, &error.message, out),
        };
        let payload = json!({"ok":true,"report":summary(&report),"page":page});
        if payload.to_string().len() <= 1024 * 1024 {
            break (page, payload);
        }
        let count = page
            .get("entries")
            .or_else(|| page.get("locations"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if count <= 1 {
            return failure(
                parsed.json,
                "BUDGET_EXCEEDED",
                "报告摘要与单条问题超过响应字节预算",
                out,
            );
        }
        limit = count / 2;
    };
    if parsed.json {
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(
            out,
            "工程问题：{} 条；完整：{}；截断：{}",
            report.entries.len(),
            report.complete,
            report.truncated
        )
        .map_err(|error| error.to_string())?;
        if let Some(entries) = page["entries"].as_array() {
            for entry in entries {
                writeln!(
                    out,
                    "{} [{}] {}: {}",
                    entry["severity"].as_str().unwrap_or(""),
                    entry["code"].as_str().unwrap_or(""),
                    entry["primary"]["path"].as_str().unwrap_or("位置不可用"),
                    entry["message"].as_str().unwrap_or("")
                )
                .map_err(|error| error.to_string())?;
            }
        } else if let Some(locations) = page["locations"].as_array() {
            for location in locations {
                writeln!(out, "{location}").map_err(|error| error.to_string())?;
            }
        }
        if !page["next_cursor"].is_null() {
            writeln!(out, "下一页游标：{}", page["next_cursor"])
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(i32::from(
        !report.complete
            || report.content_has_errors
            || report
                .entries
                .iter()
                .any(|entry| entry.severity == Severity::Error),
    ))
}

fn summary(report: &ProblemsReport) -> Value {
    json!({"schema_version":report.schema_version,"report_version":report.report_version,
        "content_baseline":report.content_baseline,"source_observation":report.source_observation,
        "language_version":report.language_version,
        "content_has_errors":report.content_has_errors,"read_only":report.read_only,
        "complete":report.complete,"truncated":report.truncated,"reasons":report.reasons,
        "coverage":report.coverage,"limits":report.limits,"compile_count":report.compile_count})
}

fn failure(json: bool, code: &str, message: &str, out: &mut impl Write) -> Result<i32, String> {
    if json {
        writeln!(
            out,
            "{}",
            json!({"ok":false,"error":{"code":code,"message":message}})
        )
    } else {
        writeln!(out, "工程问题查询失败 [{code}]: {message}\n{HELP}")
    }
    .map_err(|error| error.to_string())?;
    Ok(2)
}

fn json_value(value: &str) -> Result<Value, String> {
    worldline_core::parse_unique_json(value.as_bytes())
        .map_err(|error| format!("JSON 参数无效：{error}"))
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        path: PathBuf::new(),
        query: ProblemQuery::default(),
        cursor: None,
        options: ProblemsOptions::default(),
        limit: 0,
        related: None,
        json: false,
    };
    let mut seen = BTreeSet::new();
    let mut path = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(key, value)| (key, Some(value)));
        if key.starts_with('-') {
            if !seen.insert(key) {
                return Err(format!("参数 {key} 不能重复"));
            }
            if key == "--json" {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                parsed.json = true;
                continue;
            }
            if !matches!(
                key,
                "--query-json" | "--cursor-json" | "--options-json" | "--limit" | "--related"
            ) {
                return Err(format!("未知参数 {key}"));
            }
            let value = inline
                .or_else(|| iter.next().map(String::as_str))
                .ok_or_else(|| format!("参数 {key} 缺少值"))?;
            match key {
                "--query-json" => {
                    parsed.query = serde_json::from_value(json_value(value)?)
                        .map_err(|error| error.to_string())?
                }
                "--cursor-json" => {
                    parsed.cursor = Some(
                        serde_json::from_value(json_value(value)?)
                            .map_err(|error| error.to_string())?,
                    )
                }
                "--options-json" => {
                    parsed.options = serde_json::from_value(json_value(value)?)
                        .map_err(|error| error.to_string())?
                }
                "--limit" => parsed.limit = value.parse().map_err(|_| "--limit 必须是非负整数")?,
                "--related" if !value.trim().is_empty() => parsed.related = Some(value.into()),
                _ => return Err("--related 需要非空问题 ID".into()),
            }
        } else if path.replace(PathBuf::from(arg)).is_some() {
            return Err("problems 只能提供一个目录或入口".into());
        }
    }
    parsed.path = path.ok_or("problems 需要目录或入口")?;
    if parsed.related.is_some() && parsed.query != ProblemQuery::default() {
        return Err("--related 不能搭配非空 query".into());
    }
    Ok(parsed)
}

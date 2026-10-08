//! 一次性显式材料输入；preview/apply均不写盘，save独立重建完整计划后保存。
use super::*;
#[path = "reconciliation/budget.rs"]
mod budget;
use worldline_core::project::reconciliation::{
    ReconciliationInput, ReconciliationPlan, ReconciliationRequest, ReconciliationSession,
};

struct Args {
    operation: String,
    path: PathBuf,
    input: PathBuf,
    request: Option<PathBuf>,
    digest: Option<String>,
    json: bool,
}

#[derive(serde::Serialize)]
struct Response<'a> {
    ok: bool,
    operation: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    session: Option<&'a ReconciliationSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<&'a ReconciliationPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<&'a str>,
    applied: bool,
    saved: bool,
}

pub(super) fn command(arguments: &[String], out: &mut impl Write) -> Result<i32, String> {
    execute(arguments, out, budget::MAX_RESPONSE)
}

fn execute(
    arguments: &[String],
    out: &mut impl Write,
    response_limit: usize,
) -> Result<i32, String> {
    let json_mode = arguments.iter().any(|arg| arg == "--json");
    let args = match parse(arguments) {
        Ok(args) => args,
        Err(message) => {
            return failure(
                out,
                json_mode,
                "INVALID_PARAMS",
                "params",
                &message,
                false,
                2,
            )
        }
    };
    let materials = (|| {
        let input: ReconciliationInput = budget::read(&args.input)?;
        let request = args
            .request
            .as_ref()
            .map(|path| budget::read::<ReconciliationRequest>(path))
            .transpose()?;
        Ok::<_, String>((input, request))
    })();
    let (input, request) = match materials {
        Ok(value) => value,
        Err(message) => {
            return failure(
                out,
                args.json,
                "INVALID_PARAMS",
                "params",
                &message,
                false,
                2,
            )
        }
    };
    let mut project = match Project::open_reconciliation_input(&args.path, &input) {
        Ok(project) => project,
        Err(message) => {
            return failure(
                out,
                args.json,
                "RECONCILIATION_REJECTED",
                "capture",
                &message,
                false,
                1,
            )
        }
    };
    let session = match project.capture_reconciliation() {
        Ok(session) => session,
        Err(message) => {
            return failure(
                out,
                args.json,
                "RECONCILIATION_REJECTED",
                "capture",
                &message,
                false,
                1,
            )
        }
    };
    if args.operation == "capture" {
        return report(
            out,
            &args,
            &Response {
                ok: true,
                operation: "capture",
                session: Some(&session),
                plan: None,
                baseline: None,
                applied: false,
                saved: false,
            },
            response_limit,
        );
    }
    let request = request.ok_or("外改预览需要--request JSON文件")?;
    let plan = match project.preview_reconciliation(&session, &request) {
        Ok(plan) => plan,
        Err(message) => {
            return failure(
                out,
                args.json,
                "RECONCILIATION_REJECTED",
                "preview",
                &message,
                false,
                1,
            )
        }
    };
    let mut response = Response {
        ok: true,
        operation: &args.operation,
        session: None,
        plan: Some(&plan),
        baseline: Some(&plan.candidate_baseline),
        applied: false,
        saved: false,
    };
    if args.operation == "preview" {
        return report(out, &args, &response, response_limit);
    }
    if args.digest.as_deref() != Some(plan.plan_digest.as_str()) {
        return failure(
            out,
            args.json,
            "RECONCILIATION_REJECTED",
            "apply",
            "已审阅摘要与完整重建计划不一致",
            false,
            1,
        );
    }
    response.applied = true;
    response.saved = args.operation == "save";
    // 完整成功结果含换行先编码进有界缓冲；超过预算时采纳和保存都没有开始。
    let encoded = match budget::encode_limited(&response, response_limit) {
        Ok(bytes) => bytes,
        Err(message) => {
            return failure(
                out,
                args.json,
                "OUTPUT_LIMIT",
                "preflight",
                &message,
                false,
                1,
            )
        }
    };
    if let Err(message) = project.apply_reconciliation(&plan) {
        return failure(
            out,
            args.json,
            "RECONCILIATION_REJECTED",
            "apply",
            &message,
            false,
            1,
        );
    }
    if args.operation == "save" {
        if let Err(message) = project.save() {
            return failure(out, args.json, "SAVE_FAILED", "save", &message, true, 1);
        }
    }
    write_success(out, &args, &encoded, Some(&plan))
}

fn parse(args: &[String]) -> Result<Args, String> {
    if args.len() > 12
        || args
            .iter()
            .any(|arg| arg.len() > 4096 || arg.split(['/', '\\']).count() > 128)
    {
        return Err("外改命令参数超过数量或4096字节单项预算".into());
    }
    let operation = args.first().filter(|op| matches!(op.as_str(), "capture" | "preview" | "apply" | "save"))
        .ok_or("用法：wl reconciliation capture|preview|apply|save <目录或入口> --input <JSON文件> [--request <JSON文件>] [--plan-digest <摘要>] [--json]")?.clone();
    let path = PathBuf::from(args.get(1).ok_or("需要目录或入口")?);
    let mut input = None;
    let mut request = None;
    let mut digest = None;
    let mut json = false;
    let mut index = 2;
    while index < args.len() {
        let flag = &args[index];
        if flag == "--json" {
            if json {
                return Err("--json重复".into());
            }
            json = true;
            index += 1;
            continue;
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag}需要值"))?
            .clone();
        let duplicate = match flag.as_str() {
            "--input" => input.replace(PathBuf::from(value)).is_some(),
            "--request" => request.replace(PathBuf::from(value)).is_some(),
            "--plan-digest" => digest.replace(value).is_some(),
            _ => return Err("未知外改参数".into()),
        };
        if duplicate {
            return Err("重复外改参数".into());
        }
        index += 2;
    }
    if (operation != "capture") != request.is_some()
        || matches!(operation.as_str(), "apply" | "save") != digest.is_some()
    {
        return Err(
            "capture不接受request/digest；preview需要request；apply/save需要request和plan-digest"
                .into(),
        );
    }
    if digest.as_ref().is_some_and(|value: &String| {
        value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err("plan-digest须为16位十六进制".into());
    }
    Ok(Args {
        operation,
        path,
        input: input.ok_or("需要--input显式旧/本地稿材料")?,
        request,
        digest,
        json,
    })
}

fn report(
    out: &mut impl Write,
    args: &Args,
    response: &Response<'_>,
    response_limit: usize,
) -> Result<i32, String> {
    match budget::encode_limited(response, response_limit) {
        Ok(bytes) => write_success(out, args, &bytes, response.plan),
        Err(message) => failure(
            out,
            args.json,
            "OUTPUT_LIMIT",
            "preflight",
            &message,
            false,
            1,
        ),
    }
}
fn write_success(
    out: &mut impl Write,
    args: &Args,
    bytes: &[u8],
    plan: Option<&ReconciliationPlan>,
) -> Result<i32, String> {
    if args.json {
        out.write_all(bytes).map_err(|error| error.to_string())?;
    } else if let Some(plan) = plan {
        writeln!(
            out,
            "{} · 未解决 {} · 可采纳 {} · 计划 {}",
            args.operation, plan.unresolved, plan.can_apply, plan.plan_digest
        )
        .map_err(|error| error.to_string())?;
        if args.operation == "apply" {
            writeln!(
                out,
                "仅在本进程内采纳，未保存；--json包含完整候选，持久写入须另行显式save"
            )
            .map_err(|error| error.to_string())?;
        }
    } else {
        writeln!(out, "已捕获外改材料；使用--json读取完整三方字节")
            .map_err(|error| error.to_string())?;
    }
    Ok(0)
}
fn failure(
    out: &mut impl Write,
    json_mode: bool,
    code: &str,
    stage: &str,
    message: &str,
    applied: bool,
    exit: i32,
) -> Result<i32, String> {
    let mut end = message.len().min(4096);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    let response = json!({"ok":false,"applied":applied,"saved":false,"error":{"code":code,"stage":stage,
        "message":&message[..end],"detail_truncated":end < message.len()}});
    if json_mode {
        out.write_all(&budget::encode(&response)?)
            .map_err(|error| error.to_string())?;
    } else {
        writeln!(out, "{}", response["error"]["message"]).map_err(|error| error.to_string())?;
    }
    Ok(exit)
}

#[cfg(test)]
#[path = "reconciliation/tests.rs"]
mod tests;

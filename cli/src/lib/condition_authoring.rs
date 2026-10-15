//! 条件投影和精确补丁的机器入口；全部语义与事务由 core 提供。
use super::*;
use worldline_core::condition_authoring::{
    parse_condition_authoring_request, parse_condition_selector, ConditionAuthoringRequest,
    ConditionSelector, MAX_CONDITION_OUTPUT_BYTES,
};

const MAX_RESPONSE: usize = MAX_CONDITION_OUTPUT_BYTES + 4096;
const HELP: &str = "用法: wl condition-authoring query PROJECT --selector-json JSON [--json]\n       wl condition-authoring preview PROJECT --request-json JSON [--json]\n       wl condition-authoring apply PROJECT --request-json JSON --plan-digest DIGEST [--save] [--json]\n默认只改本进程内存；仅 --save 持久化，不控制编辑器未提交输入。";

struct Args {
    operation: String,
    path: PathBuf,
    selector: Option<ConditionSelector>,
    request: Option<ConditionAuthoringRequest>,
    digest: Option<String>,
    save: bool,
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
    let mut project = match Project::open_read_only(&parsed.path) {
        Ok(project) => project,
        Err(message) => return emit(out, failure("IO_ERROR", message), 2),
    };
    if parsed.operation == "query" {
        return match project.condition_authoring(parsed.selector.as_ref().unwrap()) {
            Ok(projection) => emit(
                out,
                json!({"ok":true,"projection":projection,
                    "baseline":project.content_baseline(),"applied":false,"saved":false,"error":null}),
                0,
            ),
            Err(message) => emit(out, failure("CONDITION_AUTHORING_REJECTED", message), 1),
        };
    }
    let request = parsed.request.as_ref().unwrap();
    let plan = match project.preview_condition_authoring(request) {
        Ok(plan) => plan,
        Err(message) => return emit(out, failure("CONDITION_AUTHORING_REJECTED", message), 1),
    };
    let apply = parsed.operation == "apply";
    if apply && parsed.digest.as_deref() != Some(plan.plan_digest.as_str()) {
        return emit(
            out,
            failure(
                "CONDITION_AUTHORING_REJECTED",
                "计划摘要已过期或不符；没有应用",
            ),
            1,
        );
    }
    let changed = apply && plan.changed;
    let notice = if !changed {
        "预览或无变化；没有应用、没有保存"
    } else if parsed.save {
        "已通过工程保存事务保存；请重新验证受影响的运行与存档"
    } else {
        "仅内存应用；进程退出将丢弃候选，请显式 --save 保存"
    };
    let mut payload = json!({"ok":true,"operation":parsed.operation,"plan":plan,
        "baseline":project.content_baseline(),"applied":changed,"saved":changed && parsed.save,
        "notice":notice,"error":null});
    // 当前与候选基线均为固定长度散列；完整成功包先计量，再允许内存或磁盘写入。
    if !fits(&payload) {
        return emit(
            out,
            failure("OUTPUT_LIMIT", "完整条件响应超过额度；没有应用"),
            1,
        );
    }
    if apply {
        if let Err(message) = project.apply_condition_authoring(request, &plan.plan_digest) {
            return emit(out, failure("CONDITION_AUTHORING_REJECTED", message), 1);
        }
    }
    if changed && parsed.save {
        if let Err(message) = project.save() {
            let mut failed = failure("SAVE_FAILED", message);
            failed["applied"] = json!(true);
            failed["stage"] = json!("save");
            failed["baseline"] = json!(project.content_baseline());
            failed["notice"] = json!("内存已应用；磁盘保存失败，请检查可恢复事务，不能视为回滚");
            return emit(out, failed, 1);
        }
    }
    payload["baseline"] = json!(project.content_baseline());
    emit(out, payload, 0)
}

fn parse(args: &[String]) -> Result<Args, String> {
    let operation = args
        .first()
        .filter(|arg| matches!(arg.as_str(), "query" | "preview" | "apply"))
        .ok_or(HELP)?
        .to_owned();
    let (mut path, mut selector, mut request, mut digest) = (None, None, None, None);
    let (mut save, mut json_mode) = (false, false);
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" if !json_mode => json_mode = true,
            "--save" if !save => save = true,
            "--selector-json" if selector.is_none() => {
                selector = Some(
                    parse_condition_selector(iter.next().ok_or(HELP)?)
                        .map_err(|error| error.to_string())?,
                );
            }
            "--request-json" if request.is_none() => {
                request = Some(
                    parse_condition_authoring_request(iter.next().ok_or(HELP)?)
                        .map_err(|error| error.to_string())?,
                );
            }
            "--plan-digest" if digest.is_none() => {
                digest = Some(
                    iter.next()
                        .filter(|value| !value.is_empty() && value.len() <= 256)
                        .ok_or(HELP)?
                        .to_owned(),
                );
            }
            value if !value.starts_with('-') && !value.is_empty() && path.is_none() => {
                path = Some(PathBuf::from(value));
            }
            _ => return Err(format!("未知或重复参数：{arg}\n{HELP}")),
        }
    }
    let valid = match operation.as_str() {
        "query" => selector.is_some() && request.is_none() && digest.is_none() && !save,
        "preview" => request.is_some() && selector.is_none() && digest.is_none() && !save,
        "apply" => request.is_some() && selector.is_none() && digest.is_some(),
        _ => false,
    };
    if !valid {
        return Err(HELP.into());
    }
    Ok(Args {
        operation,
        path: path.ok_or(HELP)?,
        selector,
        request,
        digest,
        save,
    })
}

fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message.into()},
        "applied":false,"saved":false})
}

fn fits(value: &impl serde::Serialize) -> bool {
    dialogue::fits(value, MAX_RESPONSE - 1)
}

fn emit(out: &mut impl Write, value: Value, code: i32) -> Result<i32, String> {
    let (value, code) = if fits(&value) {
        (value, code)
    } else {
        let mut failed = failure("OUTPUT_LIMIT", "完整条件响应超过额度；未截断");
        for key in ["applied", "saved"] {
            if let Some(actual) = value.get(key) {
                failed[key] = actual.clone();
            }
        }
        (failed, 1)
    };
    serde_json::to_writer(&mut *out, &value).map_err(|error| error.to_string())?;
    writeln!(out).map_err(|error| error.to_string())?;
    Ok(code)
}

//! 对白机器入口只消费 core 的正式投影与原子计划。
use super::*;
use worldline_core::manuscript::{
    parse_dialogue_edit_request, parse_dialogue_target, DialogueEditRequest,
};

pub(super) const MAX_RESPONSE: usize = 64 * 1024 * 1024;
const HELP: &str = "用法: wl dialogue query PROJECT --target-json JSON [--json]\n       wl dialogue preview PROJECT --request-json JSON [--json]\n       wl dialogue apply PROJECT --request-json JSON --plan-digest DIGEST [--save] [--json]\n正式对白和源码同源；默认只改本进程内存，不自动保存或控制编辑器。";

struct Args {
    operation: String,
    path: PathBuf,
    target: Option<TargetRef>,
    request: Option<DialogueEditRequest>,
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
    let target = parsed
        .target
        .as_ref()
        .or_else(|| parsed.request.as_ref().map(|request| &request.target))
        .expect("validated target or request");
    let buffer = match project.open_writing_buffer(target) {
        Ok(buffer) => buffer,
        Err(message) => return emit(out, failure("SOURCE_UNAVAILABLE", message), 1),
    };
    if parsed.operation == "query" {
        return match project.project_dialogue_buffer(&buffer, target) {
            Ok(projection) => emit(
                out,
                json!({"ok":true,"projection":projection,
                "baseline":project.content_baseline(),"applied":false,"saved":false,"error":null}),
                0,
            ),
            Err(error) => emit(out, failure(&error.code, error.message), 1),
        };
    }
    let plan = match project.preview_dialogue_edit(&buffer, parsed.request.as_ref().unwrap()) {
        Ok(plan) => plan,
        Err(error) => return emit(out, failure(&error.code, error.message), 1),
    };
    let apply = parsed.operation == "apply";
    if apply && parsed.digest.as_deref() != Some(plan.plan_digest.as_str()) {
        return emit(
            out,
            failure("STALE_DRAFT", "预览摘要已过期，请重新预览；没有应用"),
            1,
        );
    }
    let changed = apply && !plan.no_change;
    let notice = if !changed {
        "未应用、未保存；预览或无变化"
    } else if parsed.save {
        "已通过工程保存事务保存"
    } else {
        "仅内存应用；进程退出将丢弃候选，请显式 --save 保存"
    };
    let mut payload = json!({"ok":true,"operation":parsed.operation,"plan":plan,
        "baseline":project.content_baseline(),"applied":changed,"saved":changed && parsed.save,
        "notice":notice,"error":null});
    // 成功包所有字段先完整核验；仅基线散列将在提交后等长替换。
    if !fits(&payload, MAX_RESPONSE) {
        return emit(
            out,
            failure("BUDGET_EXCEEDED", "完整对白响应超过64MiB；没有应用"),
            1,
        );
    }
    if apply {
        if let Err(error) = project.apply_dialogue_edit(&buffer, &plan) {
            return emit(out, failure(&error.code, error.message), 1);
        }
    }
    if changed && parsed.save {
        if let Err(message) = project.save() {
            let mut failed = failure("SAVE_FAILED", message);
            failed["applied"] = json!(true);
            failed["stage"] = json!("save");
            failed["baseline"] = json!(project.content_baseline());
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
    let (mut path, mut target, mut request, mut digest) = (None, None, None, None);
    let (mut save, mut json_mode) = (false, false);
    let mut iter = args[1..].iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" if !json_mode => json_mode = true,
            "--save" if !save => save = true,
            "--target-json" if target.is_none() => {
                target = Some(
                    parse_dialogue_target(iter.next().ok_or(HELP)?)
                        .map_err(|error| error.to_string())?,
                )
            }
            "--request-json" if request.is_none() => {
                request = Some(
                    parse_dialogue_edit_request(iter.next().ok_or(HELP)?)
                        .map_err(|error| error.to_string())?,
                )
            }
            "--plan-digest" if digest.is_none() => {
                let value = iter
                    .next()
                    .filter(|value| !value.is_empty() && value.len() <= 256)
                    .ok_or(HELP)?;
                digest = Some(value.to_owned());
            }
            value if !value.starts_with('-') && path.is_none() && !value.is_empty() => {
                path = Some(PathBuf::from(value))
            }
            _ => return Err(format!("未知或重复参数：{arg}\n{HELP}")),
        }
    }
    let valid = match operation.as_str() {
        "query" => target.is_some() && request.is_none() && digest.is_none() && !save,
        "preview" => request.is_some() && target.is_none() && digest.is_none() && !save,
        "apply" => request.is_some() && target.is_none() && digest.is_some(),
        _ => false,
    };
    if !valid {
        return Err(HELP.into());
    }
    Ok(Args {
        operation,
        path: path.ok_or(HELP)?,
        target,
        request,
        digest,
        save,
    })
}

pub(super) fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message.into()},
        "applied":false,"saved":false,"delivered":false})
}

pub(super) fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("response_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}

pub(super) fn emit(out: &mut impl Write, value: Value, code: i32) -> Result<i32, String> {
    let (value, code) = if fits(&value, MAX_RESPONSE) {
        (value, code)
    } else {
        let mut failed = failure("BUDGET_EXCEEDED", "完整响应超过64MiB；未截断");
        for key in ["applied", "saved", "delivered"] {
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

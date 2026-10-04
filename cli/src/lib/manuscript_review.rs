//! 作者全分支审稿的只读 CLI；不调用恢复、刷新或保存。
use super::*;
use worldline_core::manuscript::{review_projection, MAX_REVIEW_JSON_BYTES};
const HELP: &str = "用法: wl manuscript-review <目录或入口> --target-json '{\"kind\":\"event\",\"id\":\"start\"}' [--json]\n静态全分支作者审稿，不是实际运行，不展开调用。";

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "-h" | "--help") {
        writeln!(out, "{HELP}").map_err(|error| error.to_string())?;
        return Ok(0);
    }
    let (path, target, json_mode) = match parse(args) {
        Ok(args) => args,
        Err(message) => return write(out, failure("invalid_params", &message), 2),
    };
    let project = match Project::open_read_only(&path) {
        Ok(project) => project,
        Err(message) => return write(out, failure("io_error", &message), 2),
    };
    let result = match project.compile_read_only() {
        Ok(result) => result,
        Err(message) => return write(out, failure("invalid_snapshot", &message), 1),
    };
    let (value, code) = match review_projection(&result, &target) {
        Ok(review) => (json!({"ok":true,"review":review,"error":null}), 0),
        Err(error) => (json!({"ok":false,"review":null,"error":error}), 1),
    };
    if !json_mode {
        writeln!(out, "静态全分支作者审稿 · 未执行条件或调用").map_err(|e| e.to_string())?;
    }
    write(out, value, code)
}
fn parse(args: &[String]) -> Result<(PathBuf, TargetRef, bool), String> {
    let mut path = None;
    let mut target = None;
    let mut json_mode = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--json" if !json_mode => json_mode = true,
            "--target-json" if target.is_none() => {
                let raw = iter.next().ok_or(HELP)?;
                if raw.len() > 4096 {
                    return Err("目标参数超过4096字节".into());
                }
                let value = worldline_core::parse_unique_json(raw.as_bytes())
                    .map_err(|_| "目标必须是合法JSON")?;
                let object = value.as_object().ok_or("target必须是kind/id对象")?;
                if object.len() != 2 {
                    return Err("target只能包含kind和id".into());
                }
                let kind = value
                    .get("kind")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .ok_or("缺少target.kind")?;
                let id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .ok_or("缺少target.id")?;
                target = Some(TargetRef::new(kind, id));
            }
            value if !value.starts_with('-') && path.is_none() => path = Some(PathBuf::from(value)),
            _ => return Err(HELP.into()),
        }
    }
    Ok((path.ok_or(HELP)?, target.ok_or(HELP)?, json_mode))
}
fn failure(code: &str, message: &str) -> Value {
    json!({"ok":false,"review":null,"error":{"code":code,"message":message}})
}
fn write(out: &mut impl Write, value: Value, code: i32) -> Result<i32, String> {
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    if bytes.len() >= MAX_REVIEW_JSON_BYTES + 4096 {
        serde_json::to_writer(&mut *out, &failure("review_limit", "审稿响应超过字节预算"))
            .map_err(|e| e.to_string())?;
        writeln!(out).map_err(|e| e.to_string())?;
        return Ok(1);
    }
    out.write_all(&bytes)
        .and_then(|_| writeln!(out))
        .map_err(|e| e.to_string())?;
    Ok(code)
}

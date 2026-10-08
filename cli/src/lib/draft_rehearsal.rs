//! 显式未应用正文的隔离试演；参数之外不建立另一套语言解释。
use super::*;
use std::{collections::BTreeSet, fs::File, io::Read};
use worldline_runtime::{
    draft_rehearsal::{
        run_draft_rehearsal, DraftRehearsalRunRequest, MAX_DRAFT_REHEARSAL_REQUEST_BYTES,
        MAX_DRAFT_REHEARSAL_RESULT_BYTES,
    },
    ReplayCancellation,
};

const MAX_RESPONSE_BYTES: usize = MAX_DRAFT_REHEARSAL_RESULT_BYTES + 4096;
const HELP: &str = "用法: wl draft-rehearsal <目录或入口> (--request-json '<DTO>' | --request <JSON文件>) [--json]\n只读执行明确的未应用正文快照；不应用、不保存、不改写正式运行记录。";

struct Args {
    path: PathBuf,
    request: DraftRehearsalRunRequest,
    json: bool,
}

pub(super) fn command(args: &[String], out: &mut impl Write) -> Result<i32, String> {
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        writeln!(out, "{HELP}").map_err(|error| error.to_string())?;
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
    match run_draft_rehearsal(&project, &args.request, &ReplayCancellation::new()) {
        Ok(result) => {
            #[derive(serde::Serialize)]
            struct Envelope<'a> {
                ok: bool,
                applied: bool,
                saved: bool,
                result: &'a worldline_runtime::draft_rehearsal::DraftRehearsalRunResult,
            }
            let code = i32::from(!result.ok);
            let payload = Envelope {
                ok: result.ok,
                applied: false,
                saved: false,
                result: &result,
            };
            match encode(&payload) {
                Ok(bytes) => write(args.json, &bytes, code, out),
                Err(_) => failure(
                    args.json,
                    "OUTPUT_LIMIT",
                    "草稿试演响应超过字节限制；未交付不完整证据",
                    1,
                    out,
                ),
            }
        }
        Err(message) => failure(args.json, "INVALID_PARAMS", &message, 2, out),
    }
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut path = None;
    let mut input = None;
    let mut json = false;
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
        if !matches!(key, "--request-json" | "--request") {
            return Err("含未知 draft-rehearsal 参数；不支持隐式应用或保存".into());
        }
        if input.is_some() {
            return Err("--request 与 --request-json 必须且只能选择一种".into());
        }
        let value = inline
            .or_else(|| iter.next().map(String::as_str))
            .ok_or_else(|| format!("参数 {key} 缺少值"))?;
        let bytes = if key == "--request" {
            let file =
                File::open(value).map_err(|error| format!("无法读取试演请求文件：{error}"))?;
            if !file
                .metadata()
                .map_err(|error| error.to_string())?
                .is_file()
            {
                return Err("试演请求必须是普通 JSON 文件".into());
            }
            let mut bytes = Vec::new();
            file.take((MAX_DRAFT_REHEARSAL_REQUEST_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            bytes
        } else {
            if value.len() > MAX_DRAFT_REHEARSAL_REQUEST_BYTES {
                return Err("试演请求超过 32 MiB 字节限制".into());
            }
            value.as_bytes().to_vec()
        };
        if bytes.len() > MAX_DRAFT_REHEARSAL_REQUEST_BYTES {
            return Err("试演请求超过 32 MiB 字节限制".into());
        }
        let value = worldline_core::parse_unique_json(&bytes)
            .map_err(|error| format!("试演 JSON 无效：{error}"))?;
        let request: DraftRehearsalRunRequest =
            serde_json::from_value(value).map_err(|error| format!("试演 DTO 无效：{error}"))?;
        request.validate()?;
        input = Some(request);
    }
    Ok(Args {
        path: path.ok_or("draft-rehearsal 需要工程目录或入口")?,
        request: input.ok_or("draft-rehearsal 需要明确的试演请求")?,
        json,
    })
}

fn failure(
    json_mode: bool,
    code: &str,
    message: &str,
    status: i32,
    out: &mut impl Write,
) -> Result<i32, String> {
    #[derive(serde::Serialize)]
    struct Failure<'a> {
        ok: bool,
        applied: bool,
        saved: bool,
        error: Detail<'a>,
    }
    #[derive(serde::Serialize)]
    struct Detail<'a> {
        code: &'a str,
        message: &'a str,
    }
    let value = Failure {
        ok: false,
        applied: false,
        saved: false,
        error: Detail { code, message },
    };
    let bytes = encode(&value).unwrap_or_else(|_| r#"{"ok":false,"applied":false,"saved":false,"error":{"code":"OUTPUT_LIMIT","message":"响应超过字节限制"}}"#.as_bytes().to_vec());
    write(json_mode, &bytes, status, out)
}

fn write(json_mode: bool, bytes: &[u8], status: i32, out: &mut impl Write) -> Result<i32, String> {
    if !json_mode {
        writeln!(out, "隔离正文草稿试演 · 未应用 · 未保存").map_err(|error| error.to_string())?;
    }
    out.write_all(bytes)
        .and_then(|_| writeln!(out))
        .map_err(|error| error.to_string())?;
    Ok(status)
}

fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, ()> {
    let mut buffer = Bounded(Vec::new());
    serde_json::to_writer(&mut buffer, value).map_err(|_| ())?;
    Ok(buffer.0)
}
struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) >= MAX_RESPONSE_BYTES {
            return Err(std::io::Error::other("试演响应超额"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

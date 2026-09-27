use super::*;

pub(super) fn cmd_reader_export(
    args: &ReaderExportArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let selection: ReaderExportSelection = match serde_json::from_str(&args.selection_json) {
        Ok(selection) => selection,
        Err(error) => {
            let payload = json!({
                "ok": false,
                "operation": match args.operation {
                    ReaderExportOperation::Preview => "preview",
                    ReaderExportOperation::Apply => "apply",
                },
                "error": { "code": "INVALID_SELECTION", "message": format!("阅读包选择 DTO 无效：{error}") },
                "plan": null,
            });
            if args.json {
                writeln!(out, "{payload}").map_err(|error| error.to_string())?;
                return Ok(2);
            }
            return Err(format!("阅读包选择 DTO 无效：{error}"));
        }
    };
    let project = match Project::open(&args.path) {
        Ok(project) => project,
        Err(error) => {
            let payload = json!({
                "ok": false,
                "operation": "open",
                "error": { "code": "IO_ERROR", "message": error },
                "plan": null,
            });
            if args.json {
                writeln!(out, "{payload}").map_err(|error| error.to_string())?;
                return Ok(2);
            }
            return Err(format!("无法打开工程：{error}"));
        }
    };
    let preview = match project.preview_reader_export(&selection) {
        Ok(preview) => preview,
        Err(error) => {
            let payload = json!({
                "ok": false,
                "operation": if args.operation == ReaderExportOperation::Preview { "preview" } else { "apply" },
                "error": { "code": "PREVIEW_FAILED", "message": error },
                "plan": null,
                "workspace_diagnostics": project.authoring_diagnostics(),
                "read_only": !project.authoring_diagnostics().is_empty(),
            });
            if args.json {
                writeln!(out, "{payload}").map_err(|error| error.to_string())?;
                return Ok(1);
            }
            writeln!(out, "阅读包预览失败：{error}").map_err(|error| error.to_string())?;
            return Ok(1);
        }
    };
    let workspace_diagnostics = project.authoring_diagnostics();
    match args.operation {
        ReaderExportOperation::Preview => {
            let payload = json!({
                "ok": true,
                "operation": "preview",
                "plan": preview,
                "baseline": preview.content_baseline,
                "workspace_diagnostics": workspace_diagnostics,
                "read_only": !workspace_diagnostics.is_empty(),
            });
            if args.json {
                writeln!(out, "{payload}").map_err(|error| error.to_string())?;
            } else {
                writeln!(out, "阅读包预览完成；摘要 {}", preview.plan_digest)
                    .map_err(|error| error.to_string())?;
            }
            Ok(0)
        }
        ReaderExportOperation::Apply => {
            let expected = args.plan_digest.as_deref().expect("parser requires digest");
            if expected != preview.plan_digest {
                let payload = json!({
                    "ok": false,
                    "operation": "apply",
                    "error": { "code": "STALE_PLAN", "message": "阅读包预览已过期，请重新预览并核对选择" },
                    "plan": preview,
                    "workspace_diagnostics": workspace_diagnostics,
                    "read_only": !workspace_diagnostics.is_empty(),
                });
                if args.json {
                    writeln!(out, "{payload}").map_err(|error| error.to_string())?;
                } else {
                    writeln!(out, "阅读包预览已过期，请重新预览")
                        .map_err(|error| error.to_string())?;
                }
                return Ok(1);
            }
            let destination = args.output.as_deref().expect("parser requires output path");
            match project.export_reader_site(&selection, expected, destination) {
                Ok(()) => {
                    let payload = json!({
                        "ok": true,
                        "operation": "apply",
                        "plan": preview,
                        "output": destination,
                        "workspace_diagnostics": workspace_diagnostics,
                        "read_only": !workspace_diagnostics.is_empty(),
                    });
                    if args.json {
                        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
                    } else {
                        writeln!(out, "阅读包已写入 {}", destination.display())
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(0)
                }
                Err(error) => {
                    let code = if error.contains("预览已过期") {
                        "STALE_PLAN"
                    } else {
                        "EXPORT_FAILED"
                    };
                    let payload = json!({
                        "ok": false,
                        "operation": "apply",
                        "error": { "code": code, "message": error },
                        "plan": preview,
                        "workspace_diagnostics": workspace_diagnostics,
                        "read_only": !workspace_diagnostics.is_empty(),
                    });
                    if args.json {
                        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
                    } else {
                        writeln!(out, "阅读包写入失败：{}", payload["error"]["message"])
                            .map_err(|error| error.to_string())?;
                    }
                    Ok(1)
                }
            }
        }
    }
}

pub(super) fn parse_reader_export_args(args: &[String]) -> Result<ReaderExportArgs, String> {
    let operation = match args.first().map(String::as_str) {
        Some("preview") => ReaderExportOperation::Preview,
        Some("apply") => ReaderExportOperation::Apply,
        Some(other) => {
            return Err(format!(
                "未知 reader-export 操作 `{other}`(可用: preview / apply)"
            ))
        }
        None => return Err("reader-export 需要 preview|apply 和工程目录".into()),
    };
    let path = PathBuf::from(
        args.get(1)
            .ok_or("reader-export 操作需要一个工程目录或入口")?,
    );
    let mut selection_json = None;
    let mut plan_digest = None;
    let mut output = None;
    let mut json = false;
    let mut iter = args[2..].iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" => {
                if inline.is_some() {
                    return Err("--json 不接受值".into());
                }
                json = true;
            }
            "--selection-json" | "--plan-digest" | "--out" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or_else(|| format!("参数 `{key}` 需要一个值"))?;
                let slot = match key {
                    "--selection-json" => &mut selection_json,
                    "--plan-digest" => &mut plan_digest,
                    "--out" => &mut output,
                    _ => unreachable!(),
                };
                if slot.replace(value).is_some() {
                    return Err(format!("参数 `{key}` 只能提供一次"));
                }
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => return Err(format!("未知 reader-export 参数 `{other}`")),
        }
    }
    if operation == ReaderExportOperation::Preview && (plan_digest.is_some() || output.is_some()) {
        return Err("reader-export preview 不接受 --plan-digest 或 --out".into());
    }
    if operation == ReaderExportOperation::Apply && (plan_digest.is_none() || output.is_none()) {
        return Err("reader-export apply 需要 --plan-digest 和 --out".into());
    }
    Ok(ReaderExportArgs {
        path,
        operation,
        selection_json: selection_json.ok_or("reader-export 需要 `--selection-json` DTO")?,
        plan_digest,
        output: output.map(PathBuf::from),
        json,
    })
}

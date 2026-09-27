use super::*;

pub(super) fn cmd_localization(
    args: &LocalizationArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let selection: LocalizationSelection = match serde_json::from_str(&args.selection_json) {
        Ok(selection) => selection,
        Err(error) => {
            return localization_failure(
                args,
                out,
                "INVALID_SELECTION",
                format!("本地化选择 DTO 无效：{error}"),
                None,
                None,
                &[],
            )
        }
    };
    let mut project = match Project::open(&args.path) {
        Ok(project) => project,
        Err(error) => {
            return localization_failure(
                args,
                out,
                "IO_ERROR",
                format!("无法打开工程：{error}"),
                None,
                None,
                &[],
            )
        }
    };
    match args.direction {
        LocalizationDirection::Export => {
            let diagnostics = project.authoring_diagnostics();
            let read_only = !diagnostics.is_empty();
            cmd_localization_export(args, &project, &selection, diagnostics, read_only, out)
        }
        LocalizationDirection::Import => {
            let package_path = args.package.as_deref().expect("parser requires package");
            let exchange = match read_localization_exchange(package_path) {
                Ok(exchange) => exchange,
                Err((is_io, error)) => {
                    let diagnostics = project.authoring_diagnostics();
                    return localization_failure(
                        args,
                        out,
                        if is_io { "IO_ERROR" } else { "INVALID_PACKAGE" },
                        error,
                        None,
                        None,
                        diagnostics,
                    );
                }
            };
            let diagnostics = project.authoring_diagnostics().to_vec();
            let read_only = !diagnostics.is_empty();
            cmd_localization_import(
                args,
                &mut project,
                &selection,
                &exchange,
                &diagnostics,
                read_only,
                out,
            )
        }
    }
}

pub(super) fn cmd_localization_export(
    args: &LocalizationArgs,
    project: &Project,
    selection: &LocalizationSelection,
    workspace_diagnostics: &[Diagnostic],
    read_only: bool,
    out: &mut impl Write,
) -> Result<i32, String> {
    let plan = match project.preview_localization_export(selection) {
        Ok(plan) => plan,
        Err(error) => {
            return localization_failure(
                args,
                out,
                "PREVIEW_FAILED",
                error,
                None,
                None,
                workspace_diagnostics,
            )
        }
    };
    let baseline = plan.content_baseline.clone();
    if args.operation == LocalizationOperation::Preview {
        let payload = json!({
            "ok": true,
            "operation": "preview",
            "plan": plan,
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": read_only,
        });
        return localization_response(args.json, out, payload, "本地化导出预览完成", 0);
    }

    let expected = args.plan_digest.as_deref().expect("parser requires digest");
    if expected != plan.plan_digest {
        return localization_failure(
            args,
            out,
            "STALE_PLAN",
            "本地化导出预览已过期，请重新预览".into(),
            Some(json!(plan)),
            Some(&baseline),
            workspace_diagnostics,
        );
    }
    if !plan.can_export {
        let message = plan
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "本地化导出未通过校验".into());
        return localization_failure(
            args,
            out,
            "EXPORT_REJECTED",
            message,
            Some(json!(plan)),
            Some(&baseline),
            workspace_diagnostics,
        );
    }
    let destination = args.output.as_deref().expect("parser requires output");
    match project.export_localization(selection, expected, destination) {
        Ok(applied) => {
            let payload = json!({
                "ok": true,
                "operation": "apply",
                "plan": applied,
                "baseline": baseline,
                "output": destination,
                "workspace_diagnostics": workspace_diagnostics,
                "read_only": read_only,
            });
            localization_response(
                args.json,
                out,
                payload,
                &format!("本地化交换包已写入 {}", destination.display()),
                0,
            )
        }
        Err(error) => {
            let code = if error.contains("过期") {
                "STALE_PLAN"
            } else {
                "EXPORT_FAILED"
            };
            localization_failure(
                args,
                out,
                code,
                error,
                Some(json!(plan)),
                Some(&baseline),
                workspace_diagnostics,
            )
        }
    }
}

pub(super) fn cmd_localization_import(
    args: &LocalizationArgs,
    project: &mut Project,
    selection: &LocalizationSelection,
    exchange: &LocalizationExchange,
    workspace_diagnostics: &[Diagnostic],
    read_only: bool,
    out: &mut impl Write,
) -> Result<i32, String> {
    let plan = match project.preview_localization_import(selection, exchange) {
        Ok(plan) => plan,
        Err(error) => {
            return localization_failure(
                args,
                out,
                "PREVIEW_FAILED",
                error,
                None,
                None,
                workspace_diagnostics,
            )
        }
    };
    let baseline = plan.content_baseline.clone();
    if args.operation == LocalizationOperation::Preview {
        let payload = json!({
            "ok": true,
            "operation": "preview",
            "plan": plan,
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": read_only,
        });
        return localization_response(args.json, out, payload, "本地化导入预览完成", 0);
    }

    let expected = args.plan_digest.as_deref().expect("parser requires digest");
    if expected != plan.plan_digest {
        return localization_failure(
            args,
            out,
            "STALE_PLAN",
            "本地化导入预览已过期，请重新预览".into(),
            Some(json!(plan)),
            Some(&baseline),
            workspace_diagnostics,
        );
    }
    if !plan.can_apply {
        let message = plan
            .diagnostics
            .first()
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "本地化导入未通过校验".into());
        return localization_failure(
            args,
            out,
            "IMPORT_REJECTED",
            message,
            Some(json!(plan)),
            Some(&baseline),
            workspace_diagnostics,
        );
    }
    match project.apply_localization_import(selection, exchange, expected) {
        Ok(result) => {
            let diagnostics = project.authoring_diagnostics();
            let payload = json!({
                "ok": true,
                "operation": "apply",
                "plan": result.plan,
                "changed_files": result.changed_files,
                "baseline": result.baseline,
                "new_baseline": result.new_baseline,
                "workspace_diagnostics": diagnostics,
                "read_only": !diagnostics.is_empty(),
            });
            localization_response(args.json, out, payload, "本地化译文已导入", 0)
        }
        Err(error) => {
            let code = if error.contains("过期") || error.contains("基线") {
                "STALE_PLAN"
            } else {
                "IMPORT_REJECTED"
            };
            localization_failure(
                args,
                out,
                code,
                error,
                Some(json!(plan)),
                Some(&baseline),
                workspace_diagnostics,
            )
        }
    }
}

pub(super) fn read_localization_exchange(
    path: &Path,
) -> Result<LocalizationExchange, (bool, String)> {
    let bytes = std::fs::read(path).map_err(|error| {
        (
            true,
            format!("无法读取本地化交换包 {}：{error}", path.display()),
        )
    })?;
    LocalizationExchange::from_json_bytes(&bytes).map_err(|error| (false, error))
}

pub(super) fn localization_failure(
    args: &LocalizationArgs,
    out: &mut impl Write,
    code: &str,
    message: String,
    plan: Option<Value>,
    baseline: Option<&str>,
    workspace_diagnostics: &[Diagnostic],
) -> Result<i32, String> {
    let operation = match args.operation {
        LocalizationOperation::Preview => "preview",
        LocalizationOperation::Apply => "apply",
    };
    let payload = json!({
        "ok": false,
        "operation": operation,
        "error": { "code": code, "message": message },
        "plan": plan,
        "baseline": baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": !workspace_diagnostics.is_empty(),
    });
    let exit_code = if matches!(code, "INVALID_SELECTION" | "IO_ERROR") {
        2
    } else {
        1
    };
    localization_response(args.json, out, payload, &message, exit_code)
}

pub(super) fn localization_response(
    json_output: bool,
    out: &mut impl Write,
    payload: Value,
    message: &str,
    exit_code: i32,
) -> Result<i32, String> {
    if json_output {
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(out, "{message}").map_err(|error| error.to_string())?;
    }
    Ok(exit_code)
}

pub(super) fn parse_localization_args(args: &[String]) -> Result<LocalizationArgs, String> {
    let direction = match args.first().map(String::as_str) {
        Some("export") => LocalizationDirection::Export,
        Some("import") => LocalizationDirection::Import,
        Some(other) => {
            return Err(format!(
                "未知 localization 操作 `{other}`(可用: export / import)"
            ))
        }
        None => return Err("localization 需要 export|import、preview|apply 和工程目录".into()),
    };
    let operation = match args.get(1).map(String::as_str) {
        Some("preview") => LocalizationOperation::Preview,
        Some("apply") => LocalizationOperation::Apply,
        Some(other) => {
            return Err(format!(
                "未知 localization 阶段 `{other}`(可用: preview / apply)"
            ))
        }
        None => return Err("localization 操作需要 preview|apply".into()),
    };
    let path = PathBuf::from(
        args.get(2)
            .ok_or("localization 操作需要一个工程目录或入口")?,
    );
    let mut selection_json = None;
    let mut plan_digest = None;
    let mut package = None;
    let mut output = None;
    let mut json = false;
    let mut iter = args[3..].iter();
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
            "--selection-json" | "--plan-digest" | "--package" | "--out" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or_else(|| format!("参数 `{key}` 需要一个值"))?;
                let slot = match key {
                    "--selection-json" => &mut selection_json,
                    "--plan-digest" => &mut plan_digest,
                    "--package" => &mut package,
                    "--out" => &mut output,
                    _ => unreachable!(),
                };
                if slot.replace(value).is_some() {
                    return Err(format!("参数 `{key}` 只能提供一次"));
                }
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => return Err(format!("未知 localization 参数 `{other}`")),
        }
    }
    let has_plan = plan_digest.is_some();
    let has_package = package.is_some();
    let has_output = output.is_some();
    match (direction, operation) {
        (LocalizationDirection::Export, LocalizationOperation::Preview)
            if !has_plan && !has_package && !has_output => {}
        (LocalizationDirection::Export, LocalizationOperation::Apply)
            if has_plan && !has_package && has_output => {}
        (LocalizationDirection::Import, LocalizationOperation::Preview)
            if !has_plan && has_package && !has_output => {}
        (LocalizationDirection::Import, LocalizationOperation::Apply)
            if has_plan && has_package && !has_output => {}
        (LocalizationDirection::Export, LocalizationOperation::Preview) => {
            return Err(
                "localization export preview 不接受 --plan-digest、--package 或 --out".into(),
            );
        }
        (LocalizationDirection::Export, LocalizationOperation::Apply) => {
            return Err(
                "localization export apply 需要 --plan-digest 和 --out，且不接受 --package".into(),
            );
        }
        (LocalizationDirection::Import, LocalizationOperation::Preview) => {
            return Err(
                "localization import preview 需要 --package，且不接受 --plan-digest 或 --out"
                    .into(),
            );
        }
        (LocalizationDirection::Import, LocalizationOperation::Apply) => {
            return Err(
                "localization import apply 需要 --package 和 --plan-digest，且不接受 --out".into(),
            );
        }
    }
    Ok(LocalizationArgs {
        path,
        direction,
        operation,
        selection_json: selection_json.ok_or("localization 需要 `--selection-json` DTO")?,
        plan_digest,
        package: package.map(PathBuf::from),
        output: output.map(PathBuf::from),
        json,
    })
}

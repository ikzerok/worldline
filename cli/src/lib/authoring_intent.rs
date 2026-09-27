use super::*;

pub(super) fn write_authoring_intent_failure(
    args: &AuthoringIntentArgs,
    failure: AuthoringIntentFailure<'_>,
    out: &mut impl Write,
) -> Result<i32, String> {
    if args.json {
        let payload = json!({
            "ok": false,
            "error": {"code": failure.code, "message": failure.message},
            "language_version": failure.result.map(|result| result.options.language_version.as_str()),
            "baseline": failure.baseline,
            "diagnostics": failure.result.map_or_else(Vec::new, |result| result.diagnostics.clone()),
            "workspace_diagnostics": failure.workspace_diagnostics,
            "read_only": !failure.workspace_diagnostics.is_empty(),
        });
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(out, "{}: {}", failure.code, failure.message)
            .map_err(|error| error.to_string())?;
        if let Some(result) = failure.result {
            for diagnostic in &result.diagnostics {
                writeln!(out, "{diagnostic}").map_err(|error| error.to_string())?;
            }
        }
        for diagnostic in failure.workspace_diagnostics {
            writeln!(out, "{diagnostic}").map_err(|error| error.to_string())?;
        }
    }
    Ok(failure.exit_code)
}

pub(super) fn authoring_intent_error_code(message: &str) -> &'static str {
    if message.contains("基线已过期") {
        "STALE_BASELINE"
    } else if message.contains("外部修改") || message.contains("保存事务冲突") {
        "CONFLICT"
    } else if message.contains("只读") || message.contains("必需能力") {
        "READ_ONLY"
    } else if message.contains("1.10") {
        "LANGUAGE_VERSION_REQUIRED"
    } else {
        "INTENT_REJECTED"
    }
}

pub(super) fn cmd_authoring_intent(
    args: &AuthoringIntentArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let intent: AuthoringIntent = match serde_json::from_str(&args.intent_json) {
        Ok(intent) => intent,
        Err(error) => {
            return write_authoring_intent_failure(
                args,
                AuthoringIntentFailure {
                    code: "INVALID_INTENT",
                    message: &format!("组合意图 DTO 无效：{error}"),
                    result: None,
                    baseline: None,
                    workspace_diagnostics: &[],
                    exit_code: 2,
                },
                out,
            )
        }
    };
    let mut project = match Project::open(&args.path) {
        Ok(project) => project,
        Err(error) => {
            return write_authoring_intent_failure(
                args,
                AuthoringIntentFailure {
                    code: "IO_ERROR",
                    message: &error,
                    result: None,
                    baseline: None,
                    workspace_diagnostics: &[],
                    exit_code: 2,
                },
                out,
            )
        }
    };
    let conflicts = match project.refresh() {
        Ok(conflicts) => conflicts,
        Err(error) => {
            return write_authoring_intent_failure(
                args,
                AuthoringIntentFailure {
                    code: "IO_ERROR",
                    message: &format!("刷新工程失败：{error}"),
                    result: None,
                    baseline: None,
                    workspace_diagnostics: project.authoring_diagnostics(),
                    exit_code: 2,
                },
                out,
            )
        }
    };
    let before = project.compile();
    let baseline = project.content_baseline();
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    if !conflicts.is_empty() {
        return write_authoring_intent_failure(
            args,
            AuthoringIntentFailure {
                code: "CONFLICT",
                message: "工程存在外部修改冲突，请刷新并重新预览",
                result: Some(&before),
                baseline: Some(&baseline),
                workspace_diagnostics: &workspace_diagnostics,
                exit_code: 1,
            },
            out,
        );
    }
    if intent.expected_baseline != baseline {
        return write_authoring_intent_failure(
            args,
            AuthoringIntentFailure {
                code: "STALE_BASELINE",
                message: &format!("工程基线已变化，请重新预览；当前基线为 {baseline}"),
                result: Some(&before),
                baseline: Some(&baseline),
                workspace_diagnostics: &workspace_diagnostics,
                exit_code: 1,
            },
            out,
        );
    }
    if before.has_errors() {
        return write_authoring_intent_failure(
            args,
            AuthoringIntentFailure {
                code: "COMPILE_FAILED",
                message: "当前工程存在错误诊断，组合意图未应用",
                result: Some(&before),
                baseline: Some(&baseline),
                workspace_diagnostics: &workspace_diagnostics,
                exit_code: 1,
            },
            out,
        );
    }
    if !workspace_diagnostics.is_empty() {
        return write_authoring_intent_failure(
            args,
            AuthoringIntentFailure {
                code: "READ_ONLY",
                message: "工程清单或展示文档包含当前工具不支持的能力，只能只读查看",
                result: Some(&before),
                baseline: Some(&baseline),
                workspace_diagnostics: &workspace_diagnostics,
                exit_code: 1,
            },
            out,
        );
    }
    let result = match args.operation {
        AuthoringIntentOperation::Preview => project.preview_authoring_intent(&intent),
        AuthoringIntentOperation::Apply => project.apply_authoring_intent(&intent),
    };
    let result = match result {
        Ok(result) => result,
        Err(message) => {
            let code = authoring_intent_error_code(&message);
            return write_authoring_intent_failure(
                args,
                AuthoringIntentFailure {
                    code,
                    message: &message,
                    result: Some(&before),
                    baseline: Some(&baseline),
                    workspace_diagnostics: &workspace_diagnostics,
                    exit_code: 1,
                },
                out,
            );
        }
    };
    if args.operation == AuthoringIntentOperation::Apply {
        if let Err(message) = project.save() {
            return write_authoring_intent_failure(
                args,
                AuthoringIntentFailure {
                    code: "CONFLICT",
                    message: &message,
                    result: Some(&before),
                    baseline: Some(&baseline),
                    workspace_diagnostics: &workspace_diagnostics,
                    exit_code: 1,
                },
                out,
            );
        }
    }
    let current_baseline = if args.operation == AuthoringIntentOperation::Apply {
        project.content_baseline()
    } else {
        baseline
    };
    let payload = json!({
        "ok": true,
        "operation": args.operation.as_str(),
        "target": result.target,
        "reference_impact": result.reference_impact,
        "changed_files": result.changed_files,
        "baseline": current_baseline,
        "new_baseline": result.new_baseline,
        "language_version": before.options.language_version.as_str(),
        "diagnostics": before.diagnostics,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if args.json {
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(
            out,
            "组合意图 {} 成功：{}:{}，影响 {} 个文件",
            args.operation.as_str(),
            result.target.kind,
            result.target.id,
            result.changed_files.len(),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(0)
}

pub(super) fn parse_authoring_intent_args(args: &[String]) -> Result<AuthoringIntentArgs, String> {
    let mut iter = args.iter();
    let operation = match iter
        .next()
        .ok_or("authoring-intent 需要 preview / apply 和目录")?
        .as_str()
    {
        "preview" => AuthoringIntentOperation::Preview,
        "apply" => AuthoringIntentOperation::Apply,
        other => {
            return Err(format!(
                "未知 authoring-intent 操作 `{other}`(可用: preview / apply)"
            ))
        }
    };
    let path = PathBuf::from(
        iter.next()
            .ok_or("authoring-intent 操作需要一个目录或入口")?,
    );
    let mut intent_json = None;
    let mut json = false;
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
            "--intent-json" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--intent-json` 需要 JSON DTO")?;
                if intent_json.replace(value).is_some() {
                    return Err("参数 `--intent-json` 只能提供一次".into());
                }
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => return Err(format!("未知 authoring-intent 参数 `{other}`")),
        }
    }
    Ok(AuthoringIntentArgs {
        path,
        operation,
        intent_json: intent_json.ok_or("authoring-intent 需要 `--intent-json` DTO")?,
        json,
    })
}

use super::*;

pub(super) fn markdown_import_error_code(message: &str) -> &'static str {
    if message.contains("预览已过期") {
        "STALE_PLAN"
    } else if message.contains("基线已过期") || message.contains("基线已变化") {
        "STALE_BASELINE"
    } else if message.contains("缺少损失确认") || message.contains("缺少语言升级确认")
    {
        "CONFIRMATION_REQUIRED"
    } else if message.contains("外部修改") || message.contains("目标已存在") {
        "CONFLICT"
    } else if message.contains("只读") || message.contains("必需能力") {
        "READ_ONLY"
    } else if message.contains("候选源码产生新编译错误") {
        "COMPILE_FAILED"
    } else {
        "MARKDOWN_IMPORT_REJECTED"
    }
}

pub(super) fn write_markdown_import_failure(
    args: &MarkdownImportArgs,
    code: &str,
    message: &str,
    baseline: Option<&str>,
    workspace_diagnostics: &[Diagnostic],
    out: &mut impl Write,
) -> Result<i32, String> {
    if args.json {
        let payload = json!({
            "ok": false,
            "operation": args.operation.as_str(),
            "error": {"code": code, "message": message},
            "baseline": baseline,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
        });
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(out, "{code}: {message}").map_err(|error| error.to_string())?;
    }
    Ok(if matches!(code, "IO_ERROR" | "INVALID_ARGUMENT") {
        2
    } else {
        1
    })
}

pub(super) fn cmd_markdown_import(
    args: &MarkdownImportArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let id_overrides = match args.id_map_json.as_deref() {
        Some(value) => {
            match serde_json::from_str::<std::collections::BTreeMap<String, String>>(value) {
                Ok(value) => value,
                Err(error) => {
                    return write_markdown_import_failure(
                        args,
                        "INVALID_ARGUMENT",
                        &format!(
                        "`--id-map-json` 必须是 source-relative-path 到 ID 的 JSON 对象：{error}"
                    ),
                        None,
                        &[],
                        out,
                    )
                }
            }
        }
        None => std::collections::BTreeMap::new(),
    };
    let mut project = match Project::open(&args.path) {
        Ok(project) => project,
        Err(error) => {
            return write_markdown_import_failure(args, "IO_ERROR", &error, None, &[], out)
        }
    };
    let baseline = project.content_baseline();
    let workspace_diagnostics = project.authoring_diagnostics().to_vec();
    let request = MarkdownImportRequest {
        source_root: args.source.clone(),
        expected_baseline: args.baseline.clone(),
        id_overrides,
        namespace: args.namespace.clone(),
        accept_losses: args.accept_losses,
        allow_language_upgrade: args.allow_language_upgrade,
    };
    let (plan, changed_files, new_baseline) = match args.operation {
        MarkdownImportOperation::Preview => match project.preview_markdown_import(&request) {
            Ok(plan) => (plan, Vec::new(), None),
            Err(message) => {
                let code = markdown_import_error_code(&message);
                return write_markdown_import_failure(
                    args,
                    code,
                    &message,
                    Some(&baseline),
                    &workspace_diagnostics,
                    out,
                );
            }
        },
        MarkdownImportOperation::Apply => {
            let digest = args
                .plan_digest
                .as_deref()
                .expect("parser requires plan digest");
            match project.apply_markdown_import(&request, digest) {
                Ok(result) => (result.plan, result.changed_files, Some(result.new_baseline)),
                Err(message) => {
                    let code = markdown_import_error_code(&message);
                    return write_markdown_import_failure(
                        args,
                        code,
                        &message,
                        Some(&baseline),
                        &workspace_diagnostics,
                        out,
                    );
                }
            }
        }
    };
    let payload = json!({
        "ok": true,
        "operation": args.operation.as_str(),
        "plan": plan,
        "changed_files": changed_files,
        "baseline": baseline,
        "new_baseline": new_baseline,
        "workspace_diagnostics": workspace_diagnostics,
        "read_only": false,
    });
    if args.json {
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(
            out,
            "Markdown 导入 {}：{} 页，{} 个链接，{} 个附件，{} 项损失；计划 {}",
            args.operation.as_str(),
            plan.pages.len(),
            plan.links.len(),
            plan.attachments.len(),
            plan.losses.len(),
            plan.plan_digest,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(0)
}

pub(super) fn parse_markdown_import_args(args: &[String]) -> Result<MarkdownImportArgs, String> {
    if args.first().map(String::as_str) != Some("import") {
        return Err("markdown 需要 import preview|apply 和工程目录".into());
    }
    let operation = match args.get(1).map(String::as_str) {
        Some("preview") => MarkdownImportOperation::Preview,
        Some("apply") => MarkdownImportOperation::Apply,
        Some(other) => {
            return Err(format!(
                "未知 markdown import 操作 `{other}`(可用: preview / apply)"
            ))
        }
        None => return Err("markdown import 需要 preview|apply 和工程目录".into()),
    };
    let path = PathBuf::from(args.get(2).ok_or("markdown import 操作需要一个工程目录")?);
    let mut source = None;
    let mut baseline = None;
    let mut id_map_json = None;
    let mut namespace = None;
    let mut plan_digest = None;
    let mut accept_losses = false;
    let mut allow_language_upgrade = false;
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
            "--accept-losses" => {
                if inline.is_some() || accept_losses {
                    return Err("--accept-losses 不接受值且只能提供一次".into());
                }
                accept_losses = true;
            }
            "--allow-language-upgrade" => {
                if inline.is_some() || allow_language_upgrade {
                    return Err("--allow-language-upgrade 不接受值且只能提供一次".into());
                }
                allow_language_upgrade = true;
            }
            "--source" | "--baseline" | "--id-map-json" | "--namespace" | "--plan-digest" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or_else(|| format!("参数 `{key}` 需要一个值"))?;
                let slot = match key {
                    "--source" => &mut source,
                    "--baseline" => &mut baseline,
                    "--id-map-json" => &mut id_map_json,
                    "--namespace" => &mut namespace,
                    "--plan-digest" => &mut plan_digest,
                    _ => unreachable!(),
                };
                if slot.replace(value).is_some() {
                    return Err(format!("参数 `{key}` 只能提供一次"));
                }
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => return Err(format!("未知 markdown import 参数 `{other}`")),
        }
    }
    if operation == MarkdownImportOperation::Apply && plan_digest.is_none() {
        return Err("markdown import apply 需要 `--plan-digest`".into());
    }
    if operation == MarkdownImportOperation::Preview && plan_digest.is_some() {
        return Err("markdown import preview 不接受 `--plan-digest`".into());
    }
    if operation == MarkdownImportOperation::Preview && (accept_losses || allow_language_upgrade) {
        return Err("preview 阶段不接受损失或语言升级确认；请审阅后在 apply 阶段确认".into());
    }
    Ok(MarkdownImportArgs {
        path,
        source: source.ok_or("markdown import 需要 `--source` 目录")?.into(),
        baseline: baseline.ok_or("markdown import 需要 `--baseline` 工程基线")?,
        operation,
        id_map_json,
        namespace,
        plan_digest,
        accept_losses,
        allow_language_upgrade,
        json,
    })
}

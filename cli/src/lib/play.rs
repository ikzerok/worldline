use super::parse_files::parse_language_version;
use super::support::*;
use super::*;

pub(super) fn cmd_play(
    f: &FileArgs,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    let mut story = match f.load.as_deref() {
        Some(p) => {
            let save = std::fs::read_to_string(p)
                .map_err(|e| format!("无法读取存档 {}: {e}", p.display()))?;
            match Story::load(&result.program, &result.analysis, &save) {
                Ok(story) => story,
                Err(error) => return play_start_failure(f, out, format!("存档载入失败:{error}")),
            }
        }
        None => match f.seed.map_or_else(
            || Story::new(&result.program, &result.analysis),
            |seed| Story::new_with_seed(&result.program, &result.analysis, seed),
        ) {
            Ok(story) => story,
            Err(error) => return play_start_failure(f, out, format!("故事启动失败:{error}")),
        },
    };
    let code = if f.json {
        play_json(
            &mut story,
            f.save.as_deref(),
            f.choice_presentation,
            out,
            input,
        )
    } else {
        play_human(&mut story, f.save.as_deref(), out, input)
    }?;
    if let Some(path) = &f.trace_output {
        let trace = serde_json::to_string_pretty(&story.replay_trace())
            .map_err(|error| format!("trace 生成失败:{error}"))?;
        std::fs::write(path, trace)
            .map_err(|error| format!("无法写入 trace {}: {error}", path.display()))?;
    }
    Ok(code)
}

pub(super) fn cmd_replay(args: &ReplayArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&args.path, args.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if result.has_errors() {
        return compile_failed(&result.diagnostics, args.json, out);
    }
    let trace: ReplayTrace = serde_json::from_str(&args.trace_json)
        .map_err(|error| format!("trace DTO 无效:{error}"))?;
    let replay = ReplayTrace::replay(
        &result.program,
        &result.analysis,
        &trace,
        args.budget,
        &worldline_runtime::ReplayCancellation::new(),
    )
    .map_err(|error| format!("重放请求无效:{error}"))?;
    let failed = !matches!(replay.status, ReplayStatus::Replayed { .. });
    if args.json {
        writeln!(out, "{}", json!(replay)).map_err(|error| error.to_string())?;
    } else {
        writeln!(out, "{:?}", replay.status).map_err(|error| error.to_string())?;
    }
    Ok(if failed { 1 } else { 0 })
}

pub(super) fn play_start_failure(
    f: &FileArgs,
    out: &mut impl Write,
    message: String,
) -> Result<i32, String> {
    if f.json {
        let payload = json!({
            "type": "run_error",
            "message": message,
            "node": Value::Null,
            "line": Value::Null,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{message}").map_err(|e| e.to_string())?;
    }
    Ok(1)
}

/// 人类模式:剧情文本 + 编号选项菜单,stdin 读 1 起序号。
pub(super) fn play_human(
    story: &mut Story,
    save: Option<&Path>,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<i32, String> {
    let mut buf = String::new();
    let code = loop {
        match story.continue_story() {
            Ok(outputs) => {
                let mut ended = false;
                for o in outputs {
                    match o {
                        Output::Text {
                            content, new_line, ..
                        } => {
                            if new_line {
                                writeln!(out).map_err(|e| e.to_string())?;
                            }
                            write!(out, "{content}").map_err(|e| e.to_string())?;
                        }
                        Output::Ended => ended = true,
                    }
                }
                out.flush().map_err(|e| e.to_string())?;
                if ended {
                    writeln!(out).map_err(|e| e.to_string())?;
                    writeln!(out, "—— 世界线收束,故事结束 ——").map_err(|e| e.to_string())?;
                    break 0;
                }
            }
            Err(e) => {
                writeln!(out, "运行错误:{e}").map_err(|er| er.to_string())?;
                break 1;
            }
        }
        let choices = story.choices().to_vec();
        if choices.is_empty() {
            continue;
        }
        writeln!(out).map_err(|e| e.to_string())?;
        for c in story.choice_presentations() {
            if let Some(index) = c.index {
                writeln!(out, "  {}) {}", index + 1, c.label).map_err(|e| e.to_string())?;
            } else {
                writeln!(
                    out,
                    "  [锁定] {}：{}",
                    c.label,
                    c.disabled_reason.as_deref().unwrap_or("")
                )
                .map_err(|e| e.to_string())?;
            }
        }
        write!(out, "> ").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        buf.clear();
        if input.read_line(&mut buf).map_err(|e| e.to_string())? == 0 {
            writeln!(out).map_err(|e| e.to_string())?;
            writeln!(out, "(输入结束,退出)").map_err(|e| e.to_string())?;
            break 0;
        }
        let sel: Option<usize> = buf.trim().parse::<usize>().ok().map(|n| n.wrapping_sub(1));
        match sel.and_then(|i| choices.get(i).map(|_| i)) {
            Some(i) => {
                if let Err(e) = story.choose(i) {
                    writeln!(out, "{e}").map_err(|er| er.to_string())?;
                }
            }
            None => {
                writeln!(out, "(输入序号无效)").map_err(|e| e.to_string())?;
            }
        }
    };
    write_save(story, save)?;
    Ok(code)
}

/// 选项列表的机器视图(index 0 起,规范 agent-protocol.md §2.4)。
pub(super) fn choices_json(story: &Story) -> Vec<serde_json::Value> {
    story
        .choices()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut choice = json!(c);
            choice["index"] = json!(i);
            choice
        })
        .collect()
}

/// JSON 模式(规范 agent-protocol.md §2.4):每回合一行事件;
/// 暂停时从 stdin 读一行 0 起序号(取值 = choices[].index)。
pub(super) fn play_json(
    story: &mut Story,
    save: Option<&Path>,
    choice_presentation: bool,
    out: &mut impl Write,
    input: &mut impl BufRead,
) -> Result<i32, String> {
    let mut buf = String::new();
    let code = loop {
        match story.continue_story() {
            Ok(outputs) => {
                let ended = outputs.iter().any(|o| matches!(o, Output::Ended));
                let outs: Vec<serde_json::Value> = outputs
                    .iter()
                    .map(|o| serde_json::to_value(o).expect("Output 序列化不失败"))
                    .collect();
                let mut payload = json!({
                    "type": if ended { "ended" } else { "turn" },
                    "outputs": outs,
                    "choices": choices_json(story),
                    "state": story.state_view(),
                });
                if choice_presentation {
                    payload["choice_presentation"] = json!(story.choice_presentations());
                }
                writeln!(out, "{payload}").map_err(|e| e.to_string())?;
                if ended {
                    break 0;
                }
            }
            Err(e) => {
                let payload = json!({
                    "type": "run_error",
                    "message": e.message,
                    "node": e.node,
                    "line": e.line,
                });
                writeln!(out, "{payload}").map_err(|er| er.to_string())?;
                break 1;
            }
        }
        if story.choices().is_empty() {
            continue;
        }
        out.flush().map_err(|e| e.to_string())?;
        buf.clear();
        if input.read_line(&mut buf).map_err(|e| e.to_string())? == 0 {
            let payload = json!({ "type": "eof", "state": story.state_view() });
            writeln!(out, "{payload}").map_err(|e| e.to_string())?;
            break 0;
        }
        match buf.trim().parse::<usize>() {
            Ok(i) if i < story.choices().len() => {
                if let Err(e) = story.choose(i) {
                    let payload = json!({
                        "type": "run_error",
                        "message": e.message,
                        "node": e.node,
                        "line": e.line,
                    });
                    writeln!(out, "{payload}").map_err(|er| er.to_string())?;
                    break 1;
                }
            }
            _ => {
                let payload = json!({ "type": "invalid_choice", "message": "(输入序号无效)" });
                writeln!(out, "{payload}").map_err(|e| e.to_string())?;
            }
        }
    };
    write_save(story, save)?;
    Ok(code)
}

/// `--save=<path>`:退出前把当前状态写为存档 JSON(暂停态语义见 spec/semantics.md §7)。
pub(super) fn write_save(story: &Story, save: Option<&Path>) -> Result<(), String> {
    let Some(p) = save else {
        return Ok(());
    };
    let json = story.save().map_err(|e| format!("存档生成失败:{e}"))?;
    std::fs::write(p, json).map_err(|e| format!("无法写入存档 {}: {e}", p.display()))
}

pub(super) fn parse_replay_args(args: &[String]) -> Result<ReplayArgs, String> {
    let mut path = None;
    let mut trace_json = None;
    let mut max_steps = ReplayBudget::default().max_steps;
    let mut time_budget_ms = ReplayBudget::default().time_budget_ms;
    let mut json = false;
    let mut language_version = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (key, inline) = arg
            .split_once('=')
            .map(|(key, value)| (key, Some(value)))
            .unwrap_or((arg.as_str(), None));
        match key {
            "--json" if inline.is_none() => json = true,
            "--trace-json" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--trace-json` 需要 JSON DTO")?;
                if trace_json.replace(value).is_some() {
                    return Err("参数 `--trace-json` 只能提供一次".into());
                }
            }
            "--max-steps" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--max-steps` 需要非负整数")?;
                max_steps = value
                    .parse()
                    .map_err(|_| "参数 `--max-steps` 需要非负整数")?;
            }
            "--time-budget-ms" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--time-budget-ms` 需要非负整数")?;
                time_budget_ms = value
                    .parse()
                    .map_err(|_| "参数 `--time-budget-ms` 需要非负整数")?;
            }
            "--language-version" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--language-version` 需要一个值")?;
                language_version = Some(parse_language_version(&value)?);
            }
            _ if key.starts_with("--") => return Err(format!("未知参数 {key}")),
            _ => {
                if path.replace(PathBuf::from(arg)).is_some() {
                    return Err("只能提供一个故事文件".into());
                }
            }
        }
    }
    Ok(ReplayArgs {
        path: path.ok_or("子命令 `replay` 需要一个 .wl 故事文件")?,
        trace_json: trace_json.ok_or("子命令 `replay` 需要 --trace-json")?,
        budget: ReplayBudget::new(max_steps, time_budget_ms),
        json,
        language_version,
    })
}

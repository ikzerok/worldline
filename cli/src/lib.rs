//! wl 命令行库:check / play / graph / timeline / catalog 的实现。
//! 以库形式暴露,供 main.rs 与集成测试共用。
//! `--json` 机器输出契约见 `worldline/spec/agent-protocol.md`。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::json;
use worldline_core::{compile_path, CompileResult, Diagnostic, Severity};
use worldline_runtime::{Output, Story};

/// 单个故事文件的公共参数。
struct FileArgs {
    path: PathBuf,
    json: bool,
    load: Option<PathBuf>,
    save: Option<PathBuf>,
}

struct CatalogArgs {
    file: FileArgs,
    tag: Option<String>,
    recursive: bool,
    kind: Option<String>,
}

/// 返回值 = 进程退出码。
pub fn run(args: &[String], out: &mut impl Write, input: &mut impl BufRead) -> Result<i32, String> {
    let Some(cmd) = args.first() else {
        return Err("缺少子命令".into());
    };
    let rest = &args[1..];
    match cmd.as_str() {
        "check" => {
            let f = parse_file_args(cmd, rest, false)?;
            cmd_check(&f, out)
        }
        "graph" => {
            let f = parse_file_args(cmd, rest, false)?;
            cmd_graph(&f, out)
        }
        "timeline" => {
            let f = parse_file_args(cmd, rest, false)?;
            cmd_timeline(&f, out)
        }
        "catalog" => cmd_catalog(&parse_catalog_args(rest)?, out),
        "play" => {
            let f = parse_file_args(cmd, rest, true)?;
            cmd_play(&f, out, input)
        }
        other => Err(format!(
            "未知子命令 `{other}`(可用:check / play / graph / timeline / catalog)"
        )),
    }
}

/// 解析 `<文件.wl> [--json]`;`--load=`/`--save=` 仅 play 接受。
fn parse_file_args(cmd: &str, args: &[String], session_flags: bool) -> Result<FileArgs, String> {
    let mut path: Option<PathBuf> = None;
    let mut json = false;
    let mut load = None;
    let mut save = None;
    for a in args {
        match a.as_str() {
            "--json" => json = true,
            other if session_flags && other.starts_with("--load=") => {
                load = Some(PathBuf::from(other.trim_start_matches("--load=")));
            }
            other if session_flags && other.starts_with("--save=") => {
                save = Some(PathBuf::from(other.trim_start_matches("--save=")));
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => {
                if path.replace(PathBuf::from(other)).is_some() {
                    return Err("只能提供一个故事文件".into());
                }
            }
        }
    }
    let Some(path) = path else {
        return Err(format!("子命令 `{cmd}` 需要一个 .wl 故事文件"));
    };
    Ok(FileArgs {
        path,
        json,
        load,
        save,
    })
}

fn parse_catalog_args(args: &[String]) -> Result<CatalogArgs, String> {
    let mut file_args = Vec::new();
    let mut tag = None;
    let mut kind = None;
    let mut recursive = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tag" | "--kind" => {
                let value = args
                    .next()
                    .filter(|v| !v.trim().is_empty() && !v.starts_with("--"))
                    .ok_or_else(|| format!("参数 `{arg}` 需要一个值"))?;
                let filter = if arg == "--tag" { &mut tag } else { &mut kind };
                if filter.replace(value.clone()).is_some() {
                    return Err(format!("参数 `{arg}` 只能提供一次"));
                }
            }
            "--recursive" => recursive = true,
            _ => file_args.push(arg.clone()),
        }
    }
    Ok(CatalogArgs {
        file: parse_file_args("catalog", &file_args, false)?,
        tag,
        recursive,
        kind,
    })
}

fn compile_or_fail(path: &Path, out: &mut impl Write) -> Option<CompileResult> {
    match compile_path(path) {
        Ok(r) => Some(r),
        Err(e) => {
            let _ = writeln!(out, "wl: 无法读取 {}: {e}", path.display());
            None
        }
    }
}

/// 编译存在 error 时的统一出口:JSON 模式输出 compile_failed 事件,人类模式输出提示。
fn compile_failed(diags: &[Diagnostic], json: bool, out: &mut impl Write) -> Result<i32, String> {
    if json {
        let payload = json!({ "type": "compile_failed", "diagnostics": diags });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        print_errors_hint(diags, out);
        for d in diags.iter().filter(|d| d.severity == Severity::Error) {
            let _ = writeln!(out, "{d}");
        }
    }
    Ok(1)
}

fn cmd_check(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(result) = compile_or_fail(&f.path, out) else {
        return Ok(2);
    };
    if f.json {
        let payload = json!({
            "ok": !result.has_errors(),
            "stats": &result.analysis.stats,
            "diagnostics": result.diagnostics,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        return Ok(if result.has_errors() { 1 } else { 0 });
    }
    let s = result.analysis.stats;
    writeln!(
        out,
        "{}: {} 线 / {} 角色 / {} 事件 / {} 场景 / {} 选择 / {} 字",
        f.path.display(),
        s.storylines,
        s.characters,
        s.events,
        s.scenes,
        s.choices,
        s.words
    )
    .map_err(|e| e.to_string())?;
    if result.diagnostics.is_empty() {
        writeln!(out, "诊断:无问题 ✓").map_err(|e| e.to_string())?;
    } else {
        let (mut e, mut w, mut h) = (0, 0, 0);
        for d in &result.diagnostics {
            match d.severity {
                Severity::Error => e += 1,
                Severity::Warning => w += 1,
                Severity::Hint => h += 1,
            }
            writeln!(out, "{d}").map_err(|er| er.to_string())?;
        }
        writeln!(
            out,
            "共 {} 条(错误 {e},警告 {w},提示 {h})",
            result.diagnostics.len()
        )
        .map_err(|er| er.to_string())?;
    }
    Ok(if result.has_errors() { 1 } else { 0 })
}

fn cmd_graph(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(result) = compile_or_fail(&f.path, out) else {
        return Ok(2);
    };
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    if f.json {
        let payload = json!({ "graph": &result.analysis.graph });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        write!(out, "{}", result.analysis.graph.to_mermaid()).map_err(|e| e.to_string())?;
    }
    Ok(0)
}

/// `wl timeline`:故事线泳道 + 漂流过程流图(Mermaid flowchart LR)。
fn cmd_timeline(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(result) = compile_or_fail(&f.path, out) else {
        return Ok(2);
    };
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    if f.json {
        let payload = json!({
            "stats": &result.analysis.stats,
            "anchors": &result.analysis.anchors,
            "timeline": &result.analysis.timeline,
            "graph": &result.analysis.graph,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let s = result.analysis.stats;
    writeln!(
        out,
        "%% 世界线时间线:{} 条故事线 · {} 次事件 · {} 个锚点声明",
        s.storylines,
        s.events,
        result.analysis.anchors.len()
    )
    .map_err(|e| e.to_string())?;
    write!(
        out,
        "{}",
        result.analysis.timeline.to_mermaid(&result.analysis.graph)
    )
    .map_err(|e| e.to_string())?;
    Ok(0)
}

/// `wl catalog`:直接投影 core 的对象目录与标签查询结果。
fn cmd_catalog(args: &CatalogArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(result) = compile_or_fail(&args.file.path, out) else {
        return Ok(2);
    };
    let catalog = &result.analysis.catalog;
    let mut matches = match args.tag.as_deref() {
        Some(tag) => {
            if !catalog.tags.contains_key(tag) {
                return Err(format!("未知标签 `{tag}`"));
            }
            catalog.query(tag, args.recursive)
        }
        None => catalog.objects.clone(),
    };
    if let Some(kind) = &args.kind {
        matches.retain(|object| &object.target.kind == kind);
    }
    let ok = !result.has_errors();
    if args.file.json {
        let payload = json!({
            "ok": ok,
            "catalog": catalog,
            "matches": matches,
            "diagnostics": result.diagnostics,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}: {} 个标签 / {} 个素材 / {} 个命中对象",
            args.file.path.display(),
            catalog.tags.len(),
            catalog.assets.len(),
            matches.len(),
        )
        .map_err(|e| e.to_string())?;
        for object in matches {
            writeln!(
                out,
                "{} {}  {}  ({}:{})",
                object.target.kind, object.target.id, object.display, object.file, object.line,
            )
            .map_err(|e| e.to_string())?;
        }
        for diagnostic in &result.diagnostics {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

fn print_errors_hint(diags: &[Diagnostic], out: &mut impl Write) {
    let _ = writeln!(
        out,
        "存在 {} 个错误,先运行 `wl check` 修复后再导出关系图",
        diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    );
}

fn cmd_play(f: &FileArgs, out: &mut impl Write, input: &mut impl BufRead) -> Result<i32, String> {
    let Some(result) = compile_or_fail(&f.path, out) else {
        return Ok(2);
    };
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    let mut story = match f.load.as_deref() {
        Some(p) => {
            let json = std::fs::read_to_string(p)
                .map_err(|e| format!("无法读取存档 {}: {e}", p.display()))?;
            Story::load(&result.program, &result.analysis, &json)
                .map_err(|e| format!("存档载入失败:{e}"))?
        }
        None => Story::new(&result.program, &result.analysis)
            .map_err(|e| format!("故事启动失败:{e}"))?,
    };
    if f.json {
        play_json(&mut story, f.save.as_deref(), out, input)
    } else {
        play_human(&mut story, f.save.as_deref(), out, input)
    }
}

/// 人类模式:剧情文本 + 编号选项菜单,stdin 读 1 起序号。
fn play_human(
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
        for (i, c) in choices.iter().enumerate() {
            writeln!(out, "  {}) {}", i + 1, c.label).map_err(|e| e.to_string())?;
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
fn choices_json(story: &Story) -> Vec<serde_json::Value> {
    story
        .choices()
        .iter()
        .enumerate()
        .map(|(i, c)| json!({ "index": i, "label": c.label, "line": c.line, "offset": c.offset }))
        .collect()
}

/// JSON 模式(规范 agent-protocol.md §2.4):每回合一行事件;
/// 暂停时从 stdin 读一行 0 起序号(取值 = choices[].index)。
fn play_json(
    story: &mut Story,
    save: Option<&Path>,
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
                let payload = json!({
                    "type": if ended { "ended" } else { "turn" },
                    "outputs": outs,
                    "choices": choices_json(story),
                    "state": story.state_view(),
                });
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
fn write_save(story: &Story, save: Option<&Path>) -> Result<(), String> {
    let Some(p) = save else {
        return Ok(());
    };
    let json = story.save().map_err(|e| format!("存档生成失败:{e}"))?;
    std::fs::write(p, json).map_err(|e| format!("无法写入存档 {}: {e}", p.display()))
}

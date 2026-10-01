use super::support::*;
use super::*;

pub(super) fn cmd_check(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if f.json {
        let payload = json!({
            "ok": !result.has_errors() && !snapshot.read_only,
            "stats": &result.analysis.stats,
            "language_version": result.options.language_version.as_str(),
            "diagnostics": result.diagnostics,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        return Ok(if result.has_errors() || snapshot.read_only {
            1
        } else {
            0
        });
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
    if result.diagnostics.is_empty() && snapshot.workspace_diagnostics.is_empty() {
        writeln!(out, "诊断:无问题 ✓").map_err(|e| e.to_string())?;
    } else {
        let (mut e, mut w, mut h) = (0, 0, 0);
        for d in snapshot
            .workspace_diagnostics
            .iter()
            .chain(result.diagnostics.iter())
        {
            match d.severity {
                Severity::Error => e += 1,
                Severity::Warning => w += 1,
                Severity::Hint => h += 1,
            }
            writeln!(out, "{d}").map_err(|er| er.to_string())?;
        }
        writeln!(
            out,
            "共 {} 条(错误 {e},警告 {w},提示 {h}){}",
            result.diagnostics.len() + snapshot.workspace_diagnostics.len(),
            if snapshot.read_only {
                "；工作区只读"
            } else {
                ""
            }
        )
        .map_err(|er| er.to_string())?;
    }
    Ok(if result.has_errors() || snapshot.read_only {
        1
    } else {
        0
    })
}

pub(super) fn cmd_graph(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if result.has_errors() {
        return compile_failed(&result.diagnostics, f.json, out);
    }
    if f.json {
        let payload = json!({
            "graph": &result.analysis.graph,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        write!(out, "{}", result.analysis.graph.to_mermaid()).map_err(|e| e.to_string())?;
    }
    Ok(0)
}

/// `wl timeline`:故事线泳道 + 漂流过程流图(Mermaid flowchart LR)。
pub(super) fn cmd_timeline(f: &FileArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&f.path, f.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
    if result.has_errors() && !f.json {
        return compile_failed(&result.diagnostics, false, out);
    }
    if f.json {
        let mut payload = json!({
            "ok": !result.has_errors(),
            "diagnostics": result.diagnostics,
            "language_version": result.options.language_version.as_str(),
            "stats": &result.analysis.stats,
            "anchors": &result.analysis.anchors,
            "timeline": &result.analysis.timeline,
            "graph": &result.analysis.graph,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        if result.has_errors() {
            payload["type"] = json!("compile_failed");
        }
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
        return Ok(if result.has_errors() { 1 } else { 0 });
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

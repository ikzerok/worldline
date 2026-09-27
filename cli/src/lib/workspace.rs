use super::support::*;
use super::*;

pub(super) fn cmd_workspace(args: &WorkspaceArgs, out: &mut impl Write) -> Result<i32, String> {
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(error) => return write_query_failure(&args.path, args.json, &error, out),
    };
    let read_only = !snapshot.workspace_diagnostics.is_empty();
    let ok = !snapshot.result.has_errors() && !read_only;
    if args.json {
        let mut payload = query_payload_base(&snapshot);
        payload.insert("ok".into(), json!(ok));
        payload.insert("stats".into(), json!(snapshot.result.analysis.stats));
        writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
    } else {
        let stats = snapshot.result.analysis.stats;
        writeln!(
            out,
            "{}: {} 线 / {} 角色 / {} 事件 / {} 场景 / {} 选择 / {} 字",
            args.path.display(),
            stats.storylines,
            stats.characters,
            stats.events,
            stats.scenes,
            stats.choices,
            stats.words,
        )
        .map_err(|e| e.to_string())?;
        for diagnostic in snapshot
            .result
            .diagnostics
            .iter()
            .chain(snapshot.workspace_diagnostics.iter())
        {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
        if read_only {
            writeln!(out, "工作区只读：存在工作区诊断").map_err(|e| e.to_string())?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

pub(super) fn map_references(index: &worldline_core::MapIndex) -> Vec<Value> {
    index
        .placements_by_target
        .iter()
        .map(|(target, placements)| json!({"target": target, "placements": placements}))
        .collect()
}

pub(super) fn cmd_maps(args: &MapsArgs, out: &mut impl Write) -> Result<i32, String> {
    let snapshot = match open_workspace_snapshot(&args.path) {
        Ok(snapshot) => snapshot,
        Err(error) => return write_query_failure(&args.path, args.json, &error, out),
    };
    let ok = !snapshot.result.has_errors();
    if args.json {
        let mut payload = query_payload_base(&snapshot);
        payload.insert("ok".into(), json!(ok));
        payload.insert("maps".into(), json!(&snapshot.map_index.maps));
        payload.insert(
            "references".into(),
            json!(map_references(&snapshot.map_index)),
        );
        writeln!(out, "{}", Value::Object(payload)).map_err(|e| e.to_string())?;
    } else {
        writeln!(
            out,
            "{}: {} 张地图",
            args.path.display(),
            snapshot.map_index.maps.len()
        )
        .map_err(|e| e.to_string())?;
        for (id, map) in &snapshot.map_index.maps {
            writeln!(
                out,
                "{id}  {}  ({} 个标记)",
                map.title,
                map.placements.len()
            )
            .map_err(|e| e.to_string())?;
        }
        for reference in map_references(&snapshot.map_index) {
            writeln!(out, "引用: {reference}").map_err(|e| e.to_string())?;
        }
        for diagnostic in snapshot
            .result
            .diagnostics
            .iter()
            .chain(snapshot.workspace_diagnostics.iter())
        {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

pub(super) fn parse_workspace_args(args: &[String]) -> Result<WorkspaceArgs, String> {
    let operation = args.first().ok_or("workspace 需要 check 和目录")?;
    if operation != "check" {
        return Err(format!("未知 workspace 操作 `{operation}`(可用: check)"));
    }
    let mut path = None;
    let mut json = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json = true,
            value if value.starts_with("--") => return Err(format!("未知参数 {value}")),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("workspace check 只能提供一个目录".into());
                }
            }
        }
    }
    Ok(WorkspaceArgs {
        path: path.ok_or("workspace check 需要一个目录")?,
        json,
    })
}

pub(super) fn parse_maps_args(args: &[String]) -> Result<MapsArgs, String> {
    let operation = args.first().ok_or("maps 需要 list 和目录")?;
    if operation != "list" {
        return Err(format!("未知 maps 操作 `{operation}`(可用: list)"));
    }
    let mut path = None;
    let mut json = false;
    for arg in &args[1..] {
        match arg.as_str() {
            "--json" => json = true,
            value if value.starts_with("--") => return Err(format!("未知参数 {value}")),
            value => {
                if path.replace(PathBuf::from(value)).is_some() {
                    return Err("maps list 只能提供一个目录".into());
                }
            }
        }
    }
    Ok(MapsArgs {
        path: path.ok_or("maps list 需要一个目录")?,
        json,
    })
}

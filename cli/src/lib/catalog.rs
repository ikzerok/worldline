use super::parse_files::parse_file_args;
use super::support::*;
use super::*;

/// `wl catalog`:直接投影 core 的对象目录与标签查询结果。
pub(super) fn cmd_catalog(args: &CatalogArgs, out: &mut impl Write) -> Result<i32, String> {
    let Some(snapshot) = compile_or_fail(&args.file.path, args.file.language_version, out) else {
        return Ok(2);
    };
    let result = &snapshot.result;
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
        let mut payload = json!({
            "ok": ok,
            "catalog": catalog,
            "matches": matches,
            "diagnostics": result.diagnostics,
            "workspace_diagnostics": snapshot.workspace_diagnostics,
            "read_only": snapshot.read_only,
        });
        if result.options.language_version == LanguageVersion::V1_10 || !catalog.entities.is_empty()
        {
            payload["language_version"] = json!(result.options.language_version.as_str());
        }
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
        for diagnostic in &snapshot.workspace_diagnostics {
            writeln!(out, "{diagnostic}").map_err(|e| e.to_string())?;
        }
        if snapshot.read_only {
            writeln!(out, "工作区只读：清单包含当前工具不支持的能力").map_err(|e| e.to_string())?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

pub(super) fn parse_catalog_args(args: &[String]) -> Result<CatalogArgs, String> {
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

use super::*;

pub(super) fn compile_input(
    path: &Path,
    language_version: Option<LanguageVersion>,
) -> std::io::Result<CompileSnapshot> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    let has_manifest = root.join(".world").join("project.json").is_file();
    if let Some(version) = language_version {
        let result = compile_path_with_options(path, CompileOptions::new(version))?;
        if has_manifest {
            let project = Project::open(path).map_err(std::io::Error::other)?;
            let workspace_diagnostics = project.authoring_diagnostics().to_vec();
            return Ok(CompileSnapshot {
                result,
                read_only: !workspace_diagnostics.is_empty(),
                workspace_diagnostics,
            });
        }
        return Ok(CompileSnapshot::plain(result));
    }
    // 工程清单是目录模式的唯一隐式版本来源。单文件旧调用继续走
    // compile_path，避免把相邻目录的清单意外应用到独立入口。
    if has_manifest {
        let mut project = Project::open(path).map_err(std::io::Error::other)?;
        let result = project.compile();
        let workspace_diagnostics = project.authoring_diagnostics().to_vec();
        return Ok(CompileSnapshot {
            result,
            read_only: !workspace_diagnostics.is_empty(),
            workspace_diagnostics,
        });
    }
    compile_path(path).map(CompileSnapshot::plain)
}

pub(super) fn compile_or_fail(
    path: &Path,
    language_version: Option<LanguageVersion>,
    out: &mut impl Write,
) -> Option<CompileSnapshot> {
    match compile_input(path, language_version) {
        Ok(r) => Some(r),
        Err(e) => {
            let _ = writeln!(out, "wl: 无法读取 {}: {e}", path.display());
            None
        }
    }
}

pub(super) struct WorkspaceSnapshot {
    pub(super) result: CompileResult,
    pub(super) map_index: worldline_core::MapIndex,
    pub(super) workspace_diagnostics: Vec<Diagnostic>,
    pub(super) baseline: String,
}

pub(super) fn open_workspace_snapshot(path: &Path) -> Result<WorkspaceSnapshot, String> {
    let mut project = Project::open(path)?;
    let result = project.compile();
    let map_index = project.map_index();
    let mut workspace_diagnostics = project.authoring_diagnostics().to_vec();
    for diagnostic in &map_index.diagnostics {
        if !workspace_diagnostics.iter().any(|existing| {
            existing.severity == diagnostic.severity
                && existing.code == diagnostic.code
                && existing.message == diagnostic.message
                && existing.file == diagnostic.file
                && existing.span == diagnostic.span
        }) {
            workspace_diagnostics.push(diagnostic.clone());
        }
    }
    Ok(WorkspaceSnapshot {
        baseline: project.content_baseline(),
        result,
        map_index,
        workspace_diagnostics,
    })
}

pub(super) fn query_payload_base(snapshot: &WorkspaceSnapshot) -> serde_json::Map<String, Value> {
    let read_only = !snapshot.workspace_diagnostics.is_empty();
    serde_json::Map::from_iter([
        ("schema_version".into(), json!(1)),
        (
            "language_version".into(),
            json!(snapshot.result.options.language_version.as_str()),
        ),
        ("workspace_revision".into(), json!(snapshot.baseline)),
        ("diagnostics".into(), json!(snapshot.result.diagnostics)),
        (
            "workspace_diagnostics".into(),
            json!(snapshot.workspace_diagnostics),
        ),
        ("read_only".into(), json!(read_only)),
        ("truncated".into(), json!(false)),
        ("continuation".into(), Value::Null),
    ])
}

pub(super) fn write_query_failure(
    path: &Path,
    json_mode: bool,
    error: &str,
    out: &mut impl Write,
) -> Result<i32, String> {
    write_query_error(path, json_mode, "IO_ERROR", error, out, 2)
}

pub(super) fn write_query_error(
    path: &Path,
    json_mode: bool,
    code: &str,
    error: &str,
    out: &mut impl Write,
    exit_code: i32,
) -> Result<i32, String> {
    if json_mode {
        let payload = json!({
            "ok": false,
            "schema_version": 1,
            "error": {"code": code, "message": error},
            "diagnostics": [],
            "workspace_diagnostics": [],
            "read_only": false,
            "truncated": false,
            "continuation": Value::Null,
        });
        writeln!(out, "{payload}").map_err(|e| e.to_string())?;
    } else {
        writeln!(out, "{code}: {error}").map_err(|e| e.to_string())?;
    }
    let _ = path;
    Ok(exit_code)
}
pub(super) fn print_errors_hint(diags: &[Diagnostic], out: &mut impl Write) {
    let _ = writeln!(
        out,
        "存在 {} 个错误,先运行 `wl check` 修复后再导出关系图",
        diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    );
}
/// 编译存在 error 时的统一出口:JSON 模式输出 compile_failed 事件,人类模式输出提示。
pub(super) fn compile_failed(
    diags: &[Diagnostic],
    json: bool,
    out: &mut impl Write,
) -> Result<i32, String> {
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

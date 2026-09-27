use super::*;

pub(super) fn write_catalog_query_failure(
    args: &CatalogQueryArgs,
    failure: CatalogQueryFailure<'_>,
    out: &mut impl Write,
) -> Result<i32, String> {
    if args.json {
        let diagnostics = failure
            .result
            .map_or_else(Vec::new, |result| result.diagnostics.clone());
        let language_version = failure
            .result
            .map(|result| result.options.language_version.as_str());
        let payload = json!({
            "ok": false,
            "schema_version": 1,
            "language_version": language_version,
            "workspace_revision": failure.baseline,
            "diagnostics": diagnostics,
            "workspace_diagnostics": failure.workspace_diagnostics,
            "read_only": !failure.workspace_diagnostics.is_empty(),
            "error": {"code": failure.code, "message": failure.message},
            "query": null,
        });
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(out, "查询失败 [{}]: {}", failure.code, failure.message)
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

/// `wl catalog-query`:将 JSON 查询 DTO 与游标直接交给 core Project API。
pub(super) fn cmd_catalog_query(
    args: &CatalogQueryArgs,
    out: &mut impl Write,
) -> Result<i32, String> {
    let query: CatalogQuery = match serde_json::from_str(&args.query_json) {
        Ok(query) => query,
        Err(error) => {
            return write_catalog_query_failure(
                args,
                CatalogQueryFailure {
                    code: "INVALID_QUERY",
                    message: &format!("查询 DTO 不是有效 JSON：{error}"),
                    result: None,
                    baseline: None,
                    workspace_diagnostics: &[],
                    exit_code: 2,
                },
                out,
            )
        }
    };
    let cursor: Option<CatalogQueryCursor> = match args.cursor_json.as_deref() {
        Some(value) => match serde_json::from_str(value) {
            Ok(cursor) => Some(cursor),
            Err(error) => {
                return write_catalog_query_failure(
                    args,
                    CatalogQueryFailure {
                        code: "INVALID_CURSOR",
                        message: &format!("分页游标不是有效 JSON：{error}"),
                        result: None,
                        baseline: None,
                        workspace_diagnostics: &[],
                        exit_code: 2,
                    },
                    out,
                )
            }
        },
        None => None,
    };
    let mut project = match Project::open(&args.path) {
        Ok(project) => project,
        Err(error) => {
            return write_catalog_query_failure(
                args,
                CatalogQueryFailure {
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
    let result = project.compile();
    let baseline = project.content_baseline();
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
    if result.has_errors() {
        return write_catalog_query_failure(
            args,
            CatalogQueryFailure {
                code: "COMPILE_FAILED",
                message: "当前工程存在错误诊断，无法执行资料查询",
                result: Some(&result),
                baseline: Some(&baseline),
                workspace_diagnostics: &workspace_diagnostics,
                exit_code: 1,
            },
            out,
        );
    }
    let page = match cursor.as_ref() {
        Some(cursor) => project.continue_catalog_query(&query, cursor),
        None => project.query_catalog(&query, args.options),
    };
    let page = match page {
        Ok(page) => page,
        Err(error) => {
            return write_catalog_query_failure(
                args,
                CatalogQueryFailure {
                    code: error.code(),
                    message: &error.to_string(),
                    result: Some(&result),
                    baseline: Some(&baseline),
                    workspace_diagnostics: &workspace_diagnostics,
                    exit_code: 2,
                },
                out,
            )
        }
    };
    if args.json {
        let payload = json!({
            "ok": true,
            "schema_version": 1,
            "language_version": result.options.language_version.as_str(),
            "workspace_revision": baseline,
            "diagnostics": result.diagnostics,
            "workspace_diagnostics": workspace_diagnostics,
            "read_only": !workspace_diagnostics.is_empty(),
            "query": page,
        });
        writeln!(out, "{payload}").map_err(|error| error.to_string())?;
    } else {
        writeln!(
            out,
            "{}（{} 个命中，偏移 {}）",
            page.summary, page.total, page.offset
        )
        .map_err(|error| error.to_string())?;
        for item in &page.items {
            writeln!(
                out,
                "{}:{}  {}:{}  {}",
                item.target.kind,
                item.target.id,
                item.source.file,
                item.source.line,
                item.reasons.join("；"),
            )
            .map_err(|error| error.to_string())?;
        }
        if let Some(cursor) = &page.next {
            let cursor = serde_json::to_string(cursor).expect("查询游标可序列化");
            writeln!(out, "下一页游标：{cursor}").map_err(|error| error.to_string())?;
        }
    }
    Ok(0)
}

pub(super) fn parse_catalog_query_args(args: &[String]) -> Result<CatalogQueryArgs, String> {
    let mut path = None;
    let mut query_json = None;
    let mut cursor_json = None;
    let mut options = CatalogQueryOptions::default();
    let mut paging_options_set = false;
    let mut json = false;
    let mut iter = args.iter();
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
            "--query" | "--query-json" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--query` 需要 JSON DTO")?;
                if query_json.replace(value).is_some() {
                    return Err("参数 `--query` 只能提供一次".into());
                }
            }
            "--cursor" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or("参数 `--cursor` 需要 JSON 游标")?;
                if cursor_json.replace(value).is_some() {
                    return Err("参数 `--cursor` 只能提供一次".into());
                }
            }
            "--offset" | "--page-size" | "--max-candidates" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| iter.next().cloned())
                    .ok_or_else(|| format!("参数 `{key}` 需要非负整数"))?;
                let value = value
                    .parse::<usize>()
                    .map_err(|_| format!("参数 `{key}` 需要非负整数"))?;
                paging_options_set = true;
                match key {
                    "--offset" => options.offset = value,
                    "--page-size" => options.page_size = value,
                    "--max-candidates" => options.max_candidates = value,
                    _ => unreachable!(),
                }
            }
            other if other.starts_with("--") => return Err(format!("未知参数 {other}")),
            other => {
                if path.replace(PathBuf::from(other)).is_some() {
                    return Err("catalog-query 只能提供一个目录或入口".into());
                }
            }
        }
    }
    if cursor_json.is_some() && paging_options_set {
        return Err("使用 `--cursor` 时不能同时指定 offset、page-size 或 max-candidates".into());
    }
    Ok(CatalogQueryArgs {
        path: path.ok_or("catalog-query 需要一个目录或入口")?,
        query_json: query_json.ok_or("catalog-query 需要 `--query` JSON DTO")?,
        cursor_json,
        options,
        json,
    })
}

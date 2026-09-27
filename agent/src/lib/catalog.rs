use super::projects::{query_failure, query_payload_base, refreshed_workspace};
use super::*;
impl Server {
    pub(super) fn catalog_query(&mut self, params: &Value) -> Result<Value, ProtoError> {
        let query_value = params
            .get("query")
            .ok_or_else(|| ProtoError::new(-32602, "资料查询需要 `query` DTO"))?;
        let query: CatalogQuery = serde_json::from_value(query_value.clone())
            .map_err(|error| ProtoError::new(-32602, format!("无效资料查询 DTO：{error}")))?;
        let cursor = match params.get("cursor") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                serde_json::from_value::<CatalogQueryCursor>(value.clone()).map_err(|error| {
                    ProtoError::new(-32602, format!("无效资料查询游标：{error}"))
                })?,
            ),
        };
        let options = catalog_query_options(params, cursor.is_some())?;
        let has_project_id = params.get("project_id").is_some();
        let has_path = params.get("path").is_some();
        if has_project_id == has_path {
            return Err(ProtoError::new(
                -32602,
                "资料查询必须且只能提供 `project_id` 或 `path`",
            ));
        }
        if has_project_id {
            let project_id = param_str(params, "project_id")?;
            let unit = self.projects.get_mut(project_id).ok_or_else(|| {
                ProtoError::new(-32602, format!("未知 project_id `{project_id}`"))
            })?;
            return Ok(catalog_query_project(
                &mut unit.project,
                &query,
                cursor.as_ref(),
                options,
            ));
        }
        let path = param_str(params, "path")?;
        let mut project = match Project::open(Path::new(path)) {
            Ok(project) => project,
            Err(error) => {
                return Ok(catalog_query_failure(
                    "IO_ERROR",
                    error,
                    None,
                    None,
                    None,
                    &[],
                ))
            }
        };
        Ok(catalog_query_project(
            &mut project,
            &query,
            cursor.as_ref(),
            options,
        ))
    }
}

fn catalog_query_options(
    params: &Value,
    has_cursor: bool,
) -> Result<CatalogQueryOptions, ProtoError> {
    if has_cursor
        && ["offset", "page_size", "max_candidates"]
            .iter()
            .any(|key| params.get(key).is_some())
    {
        return Err(ProtoError::new(
            -32602,
            "使用 `cursor` 时不能同时指定 offset、page_size 或 max_candidates",
        ));
    }
    let mut options = CatalogQueryOptions::default();
    for (key, target) in [
        ("offset", &mut options.offset),
        ("page_size", &mut options.page_size),
        ("max_candidates", &mut options.max_candidates),
    ] {
        if let Some(value) = params.get(key) {
            let parsed = value
                .as_u64()
                .ok_or_else(|| ProtoError::new(-32602, format!("`{key}` 必须是非负整数")))?
                .try_into()
                .map_err(|_| ProtoError::new(-32602, format!("`{key}` 超出平台整数范围")))?;
            *target = parsed;
        }
    }
    Ok(options)
}

fn catalog_query_failure(
    code: &str,
    message: String,
    diagnostics: Option<&Vec<Diagnostic>>,
    baseline: Option<String>,
    language_version: Option<&str>,
    workspace_diagnostics: &[Diagnostic],
) -> Value {
    let mut response = query_failure(
        code,
        message,
        diagnostics,
        baseline,
        language_version,
        workspace_diagnostics,
    );
    response["query"] = Value::Null;
    response
}

fn catalog_query_project(
    project: &mut Project,
    query: &CatalogQuery,
    cursor: Option<&CatalogQueryCursor>,
    options: CatalogQueryOptions,
) -> Value {
    let snapshot = match refreshed_workspace(project) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let result = project.compile();
            return catalog_query_failure(
                "IO_ERROR",
                format!("刷新工程失败：{error}"),
                Some(&result.diagnostics),
                Some(project.content_baseline()),
                Some(result.options.language_version.as_str()),
                project.authoring_diagnostics(),
            );
        }
    };
    if snapshot.result.has_errors() {
        let mut response = catalog_query_failure(
            "COMPILE_FAILED",
            "当前工程存在错误诊断，无法执行资料查询".into(),
            Some(&snapshot.result.diagnostics),
            Some(snapshot.baseline.clone()),
            Some(snapshot.result.options.language_version.as_str()),
            &snapshot.workspace_diagnostics,
        );
        if !snapshot.conflicts.is_empty() {
            response["conflicts"] = json!(snapshot.conflicts);
        }
        return response;
    }
    let page = match cursor {
        Some(cursor) => project.continue_catalog_query(query, cursor),
        None => project.query_catalog(query, options),
    };
    let page = match page {
        Ok(page) => page,
        Err(error) => {
            let mut response = catalog_query_failure(
                error.code(),
                error.to_string(),
                Some(&snapshot.result.diagnostics),
                Some(snapshot.baseline.clone()),
                Some(snapshot.result.options.language_version.as_str()),
                &snapshot.workspace_diagnostics,
            );
            if !snapshot.conflicts.is_empty() {
                response["conflicts"] = json!(snapshot.conflicts);
            }
            return response;
        }
    };
    let mut payload = query_payload_base(&snapshot);
    payload.insert("ok".into(), json!(true));
    payload.insert("query".into(), json!(page));
    if !snapshot.conflicts.is_empty() {
        payload.insert("conflicts".into(), json!(snapshot.conflicts));
    }
    Value::Object(payload)
}

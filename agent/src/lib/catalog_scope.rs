//! catalog.scope 是单次只读快照，不保存服务器集合或扩大写权限。
use super::*;
use worldline_core::queries::DEFAULT_CATALOG_QUERY_CANDIDATES;

const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    let id = message.get("id");
    if id.is_some_and(|id| !fits(id, 3072)) {
        return Some(err(
            Value::Null,
            -32600,
            "查询范围请求标识超过3072字节",
            json!("request_id_exceeds_response_budget"),
        ));
    }
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(err(
            id.cloned().unwrap_or(Value::Null),
            -32600,
            "无效请求",
            Value::Null,
        ));
    }
    let empty = json!({});
    let result = server.catalog_scope(message.get("params").unwrap_or(&empty));
    let id = id?.clone();
    let response = match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => err(id.clone(), error.code, &error.message, Value::Null),
    };
    if fits(&response, MAX_RESPONSE_BYTES - 1) {
        return Some(response);
    }
    Some(
        json!({"jsonrpc":"2.0","id":id,"result":failure("BUDGET_EXCEEDED", "完整查询范围响应与行分隔超过32MiB预算；未截断范围")}),
    )
}
fn fits(value: &impl serde::Serialize, budget: usize) -> bool {
    struct Counter(usize);
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("catalog_scope_limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Counter(budget), value).is_ok()
}
impl Server {
    pub(super) fn catalog_scope(&mut self, params: &Value) -> Result<Value, ProtoError> {
        if !fits(params, MAX_INPUT_BYTES) {
            return Err(ProtoError::new(-32602, "查询范围参数超过4MiB预算"));
        }
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "查询范围参数必须为对象"))?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "project_id"
                    | "path"
                    | "query"
                    | "max_candidates"
                    | "offset"
                    | "page_size"
                    | "focus"
                    | "relation_options"
            )
        }) {
            return Err(ProtoError::new(-32602, "查询范围含未知参数"));
        }
        let query: CatalogQuery = serde_json::from_value(
            params
                .get("query")
                .ok_or_else(|| ProtoError::new(-32602, "查询范围需要 query DTO"))?
                .clone(),
        )
        .map_err(|e| ProtoError::new(-32602, format!("无效 query DTO：{e}")))?;
        let max_candidates = number(params, "max_candidates", DEFAULT_CATALOG_QUERY_CANDIDATES)?;
        let offset = number(params, "offset", 0)?;
        let page_size = number(params, "page_size", 50)?;
        let paging = CatalogQueryOptions {
            max_candidates,
            offset,
            page_size,
        };
        let focus: Option<TargetRef> = params
            .get("focus")
            .filter(|v| !v.is_null())
            .map(|value| {
                serde_json::from_value(value.clone())
                    .map_err(|e| ProtoError::new(-32602, format!("无效 focus：{e}")))
            })
            .transpose()?;
        let options: RelationQueryOptions = params
            .get("relation_options")
            .filter(|v| !v.is_null())
            .map(|value| {
                serde_json::from_value(value.clone())
                    .map_err(|e| ProtoError::new(-32602, format!("无效 relation_options：{e}")))
            })
            .transpose()?
            .unwrap_or_default();
        if focus.is_none() && params.get("relation_options").is_some() {
            return Err(ProtoError::new(-32602, "relation_options 需要 focus"));
        }
        if params.get("cursor").is_some() {
            return Err(ProtoError::new(-32602, "catalog.scope 不接受旧查询游标"));
        }
        if params.get("project_id").is_some() == params.get("path").is_some() {
            return Err(ProtoError::new(-32602, "必须且只能提供 project_id 或 path"));
        }
        if params.get("project_id").is_some() {
            let id = param_str(params, "project_id")?;
            let unit = self
                .projects
                .get(id)
                .ok_or_else(|| ProtoError::new(-32602, format!("未知 project_id `{id}`")))?;
            // A read must not call refresh: refresh may recover a pending disk transaction.
            return Ok(project_scope(
                &unit.project,
                &query,
                paging,
                focus.as_ref(),
                options,
                "applied_project_snapshot",
            ));
        }
        let path = param_str(params, "path")?;
        let project = match Project::open_read_only(Path::new(path)) {
            Ok(project) => project,
            Err(error) => return Ok(failure("IO_ERROR", &error)),
        };
        Ok(project_scope(
            &project,
            &query,
            paging,
            focus.as_ref(),
            options,
            "read_only_path_snapshot",
        ))
    }
}
fn project_scope(
    project: &Project,
    query: &CatalogQuery,
    paging: CatalogQueryOptions,
    focus: Option<&TargetRef>,
    options: RelationQueryOptions,
    source_mode: &'static str,
) -> Value {
    let scope = match project.catalog_scope_snapshot(query, paging.max_candidates) {
        Ok(scope) => scope,
        Err(error) => return failure(error.code(), &error.to_string()),
    };
    let page = match scope.query().page(paging.offset, paging.page_size) {
        Ok(page) => page,
        Err(error) => return failure(error.code(), &error.to_string()),
    };
    let relations = focus.map(|focus| scope.query_relations(focus, options));
    #[derive(serde::Serialize)]
    struct Payload<'a> {
        ok: bool,
        schema_version: u32,
        workspace_revision: &'a str,
        scope: &'a worldline_core::catalog_scope::CatalogScopeSnapshot,
        page: &'a worldline_core::queries::CatalogSnapshotPage,
        relations: &'a Option<worldline_core::relations::RelationQueryResult>,
        source_mode: &'static str,
        refreshed: bool,
        conflicts: Option<Vec<String>>,
    }
    let payload = Payload {
        ok: true,
        schema_version: 1,
        workspace_revision: &scope.query().snapshot,
        scope: &scope,
        page: &page,
        relations: &relations,
        source_mode,
        refreshed: false,
        conflicts: None,
    };
    if !fits(&payload, MAX_RESPONSE_BYTES - 4096) {
        return failure(
            "BUDGET_EXCEEDED",
            "完整范围与结果页超过响应预算；未截断范围",
        );
    }
    serde_json::to_value(payload).unwrap_or_else(|_| failure("ENCODING_FAILED", "查询范围无法编码"))
}
fn number(params: &Value, key: &str, default: usize) -> Result<usize, ProtoError> {
    params
        .get(key)
        .map(|value| {
            value
                .as_u64()
                .and_then(|v| v.try_into().ok())
                .ok_or_else(|| ProtoError::new(-32602, format!("{key} 必须为平台范围内非负整数")))
        })
        .transpose()
        .map(|v| v.unwrap_or(default))
}
fn failure(code: &str, message: &str) -> Value {
    json!({"ok":false,"schema_version":1,"scope":null,"error":{"code":code,"message":message}})
}

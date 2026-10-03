use super::*;
use serde::Deserialize;
use worldline_core::problems::{ProblemCursor, ProblemQuery, ProblemsOptions, ProblemsReport};

#[path = "problems_rpc.rs"]
mod rpc;
pub(super) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
pub(super) fn dispatch_rpc(server: &mut Server, message: &Value) -> Option<Value> {
    rpc::dispatch(server, message)
}

pub(super) struct CachedReport {
    report: ProblemsReport,
    conflicts: Vec<PathBuf>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Params {
    path: Option<String>,
    project_id: Option<String>,
    query: ProblemQuery,
    cursor: Option<ProblemCursor>,
    limit: usize,
    options: ProblemsOptions,
    related_id: Option<String>,
    refresh: bool,
}

impl Server {
    pub(super) fn project_problems(
        &mut self,
        value: &Value,
        response_budget: usize,
    ) -> Result<Value, ProtoError> {
        let params: Params = serde_json::from_value(value.clone())
            .map_err(|error| ProtoError::new(-32602, format!("工程问题参数无效：{error}")))?;
        if value.get("path").is_some() == value.get("project_id").is_some() {
            return Err(ProtoError::new(
                -32602,
                "工程问题必须且只能提供 path 或 project_id",
            ));
        }
        let key = if value.get("path").is_some() {
            "path"
        } else {
            "project_id"
        };
        if param_str(value, key)?.trim().is_empty() {
            return Err(ProtoError::new(-32602, "path 和 project_id 不能为空"));
        }
        if value.get("related_id").is_some()
            && params
                .related_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
        {
            return Err(ProtoError::new(-32602, "related_id 必须是非空字符串"));
        }
        if params.related_id.is_some() && params.query != ProblemQuery::default() {
            return Err(ProtoError::new(-32602, "related_id 不能搭配非空 query"));
        }
        if let Some(id) = params.project_id.as_deref() {
            let unit = self
                .projects
                .get_mut(id)
                .ok_or_else(|| ProtoError::new(-32602, format!("未知 project_id `{id}`")))?;
            let conflicts = match unit.project.refresh() {
                Ok(conflicts) => conflicts,
                Err(error) => {
                    unit.problems_report = None;
                    return Ok(failure("IO_ERROR", error));
                }
            };
            let baseline = unit.project.content_baseline();
            let observation = unit.project.problems_observation_key().ok();
            let reusable = !params.refresh
                && unit.problems_report.as_ref().is_some_and(|cache| {
                    cache.report.content_baseline == baseline
                        && !cache.report.source_observation.is_empty()
                        && !cache
                            .report
                            .reasons
                            .iter()
                            .any(|reason| reason == "external_observation_changed")
                        && observation.as_deref() == Some(cache.report.source_observation.as_str())
                        && cache.report.limits == params.options
                        && cache.conflicts == conflicts
                });
            if !reusable {
                // Do not use refreshed_workspace: it compiles and constructs unrelated indexes.
                unit.problems_report = None;
                let report = match unit.project.problems_report(&params.options) {
                    Ok(report) => report,
                    Err(error) => {
                        return Ok(with_conflicts(
                            failure(&error.code, error.message),
                            &conflicts,
                            response_budget,
                        ))
                    }
                };
                unit.problems_report = Some(CachedReport { report, conflicts });
            }
            let cache = unit
                .problems_report
                .as_ref()
                .expect("report built or reused");
            return Ok(response(
                &cache.report,
                &params,
                reusable,
                &cache.conflicts,
                response_budget,
            ));
        }
        let project =
            match Project::open(Path::new(params.path.as_deref().expect("validated path"))) {
                Ok(project) => project,
                Err(error) => return Ok(failure("IO_ERROR", error)),
            };
        Ok(match project.problems_report(&params.options) {
            Ok(report) => response(&report, &params, false, &[], response_budget),
            Err(error) => failure(&error.code, error.message),
        })
    }
}

fn response(
    report: &ProblemsReport,
    params: &Params,
    reused: bool,
    conflicts: &[PathBuf],
    response_budget: usize,
) -> Value {
    let mut limit = params.limit;
    loop {
        let page = match params.related_id.as_deref() {
            Some(id) => report
                .related_page(id, params.cursor.as_ref(), limit)
                .map(|page| json!(page)),
            None => report
                .query(&params.query, params.cursor.as_ref(), limit)
                .map(|page| json!(page)),
        };
        let page = match page {
            Ok(page) => page,
            Err(error) => {
                return with_conflicts(
                    failure(&error.code, error.message),
                    conflicts,
                    response_budget,
                )
            }
        };
        let count = page
            .get("entries")
            .or_else(|| page.get("locations"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let mut response = json!({"ok":true,"page":page,"report":{
            "schema_version":report.schema_version,"report_version":report.report_version,
            "content_baseline":report.content_baseline,"source_observation":report.source_observation,
            "language_version":report.language_version,
            "content_has_errors":report.content_has_errors,"read_only":report.read_only,
            "complete":report.complete,"truncated":report.truncated,"reasons":report.reasons,
            "coverage":report.coverage,"limits":report.limits,
            "compile_count":if reused { 0 } else { report.compile_count }}});
        if !conflicts.is_empty() {
            response["conflicts"] = json!(conflicts);
        }
        if response.to_string().len() <= response_budget {
            return response;
        }
        if count <= 1 {
            return failure(
                "BUDGET_EXCEEDED",
                "报告摘要与单条问题超过响应字节预算".into(),
            );
        }
        limit = count / 2;
    }
}

fn failure(code: &str, message: String) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message}})
}

fn with_conflicts(mut response: Value, conflicts: &[PathBuf], response_budget: usize) -> Value {
    if !conflicts.is_empty() {
        response["conflicts"] = json!(conflicts);
    }
    if response.to_string().len() > response_budget {
        return failure("BUDGET_EXCEEDED", "冲突列表超过响应字节预算".into());
    }
    response
}

#[cfg(test)]
#[path = "problems_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "problems_rpc_tests.rs"]
mod rpc_tests;

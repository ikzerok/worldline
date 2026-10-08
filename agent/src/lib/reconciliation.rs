//! 外改会话只缓存core证据；采纳后仍由project.save独立保存。
use super::*;
#[path = "reconciliation/budget.rs"]
mod budget;
pub(super) fn dispatch(server: &mut Server, message: &Value) -> Option<Value> {
    budget::dispatch(server, message)
}
use worldline_core::project::reconciliation::{
    ReconciliationPlan, ReconciliationRequest, ReconciliationSession,
};

#[derive(Default)]
pub(super) struct Cache {
    session: Option<ReconciliationSession>,
    plan: Option<ReconciliationPlan>,
}

impl Server {
    pub(super) fn reconciliation(
        &mut self,
        params: &Value,
        operation: &str,
    ) -> Result<Value, ProtoError> {
        budget::check_params(params)?;
        let allowed: &[&str] = match operation {
            "capture" => &["project_id"],
            "preview" => &["project_id", "request"],
            "apply" => &["project_id", "plan_digest"],
            _ => return Err(ProtoError::new(-32601, "未知外改操作")),
        };
        let object = params
            .as_object()
            .ok_or_else(|| ProtoError::new(-32602, "外改参数必须是对象"))?;
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(ProtoError::new(-32602, "外改操作包含未知参数"));
        }
        let request = if operation == "preview" {
            Some(
                serde_json::from_value::<ReconciliationRequest>(
                    params
                        .get("request")
                        .cloned()
                        .ok_or_else(|| ProtoError::new(-32602, "需要request DTO"))?,
                )
                .map_err(|_| ProtoError::new(-32602, "无效外改候选DTO"))?,
            )
        } else {
            None
        };
        let plan_digest = if operation == "apply" {
            let digest = param_str(params, "plan_digest")?;
            if digest.len() != 16 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(ProtoError::new(-32602, "plan_digest必须是16位十六进制摘要"));
            }
            Some(digest.to_owned())
        } else {
            None
        };
        let id = param_str(params, "project_id")?;
        let unit = self
            .projects
            .get_mut(id)
            .ok_or_else(|| ProtoError::new(-32602, "未知project_id"))?;
        let failure = |message: String| {
            json!({"ok":false,"operation":operation,"applied":false,"saved":false,
            "error":{"code":"RECONCILIATION_REJECTED","message":message}})
        };
        match operation {
            "capture" => match unit.project.capture_reconciliation() {
                Ok(session) => {
                    let response = match budget::encode(&SessionPayload {
                        ok: true,
                        operation,
                        session: &session,
                        applied: false,
                        saved: false,
                    }) {
                        Ok(value) => value,
                        Err(value) => return Ok(value),
                    };
                    unit.reconciliation = Cache {
                        session: Some(session),
                        plan: None,
                    };
                    Ok(response)
                }
                Err(error) => Ok(failure(error)),
            },
            "preview" => {
                let Some(session) = unit.reconciliation.session.as_ref() else {
                    return Ok(failure("请先显式捕获外改会话".into()));
                };
                let request = request.ok_or_else(|| ProtoError::new(-32602, "需要request"))?;
                match unit.project.preview_reconciliation(session, &request) {
                    Ok(plan) => {
                        let response = match budget::encode(&PlanPayload {
                            ok: true,
                            operation,
                            plan: &plan,
                            baseline: None,
                            applied: false,
                            saved: false,
                        }) {
                            Ok(value) => value,
                            Err(value) => return Ok(value),
                        };
                        unit.reconciliation.plan = Some(plan);
                        Ok(response)
                    }
                    Err(error) => Ok(failure(error)),
                }
            }
            "apply" => {
                let Some(plan) = unit.reconciliation.plan.as_ref() else {
                    return Ok(failure("请先完整预览候选".into()));
                };
                if plan_digest.as_deref() != Some(plan.plan_digest.as_str()) {
                    return Ok(failure("审阅计划摘要不一致".into()));
                }
                // 在实际采纳前检查完整结果预算；超大结果不能先改Project再报零修改。
                let response = match budget::encode(&PlanPayload {
                    ok: true,
                    operation,
                    plan,
                    baseline: Some(&plan.candidate_baseline),
                    applied: true,
                    saved: false,
                }) {
                    Ok(value) => value,
                    Err(value) => return Ok(value),
                };
                match unit.project.apply_reconciliation(plan) {
                    Ok(_applied) => {
                        unit.reconciliation = Cache::default();
                        unit.problems_report = None;
                        unit.scene_revision.content_generation =
                            unit.scene_revision.content_generation.wrapping_add(1);
                        unit.scene_revision.workspace_generation =
                            unit.scene_revision.workspace_generation.wrapping_add(1);
                        Ok(response)
                    }
                    Err(error) => Ok(failure(error)),
                }
            }
            _ => unreachable!(),
        }
    }
}

#[derive(serde::Serialize)]
struct SessionPayload<'a> {
    ok: bool,
    operation: &'a str,
    session: &'a ReconciliationSession,
    applied: bool,
    saved: bool,
}

#[derive(serde::Serialize)]
struct PlanPayload<'a> {
    ok: bool,
    operation: &'a str,
    plan: &'a ReconciliationPlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<&'a str>,
    applied: bool,
    saved: bool,
}

#[cfg(test)]
#[path = "reconciliation/tests.rs"]
mod tests;

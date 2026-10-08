use super::{DraftRehearsal, MAX_DRAFT_REHEARSAL_REQUEST_BYTES, MAX_DRAFT_REHEARSAL_RESULT_BYTES};
use crate::{
    ChoiceExplanation, ChoicePresentation, ContinuationOutcome, Output, ReplayBudget,
    ReplayCancellation, StateInspectionPage, StateInspectionQuery,
};
use serde::{Deserialize, Serialize};
use worldline_core::{
    draft_rehearsal::{DraftRehearsalRequest, DraftRehearsalScope},
    project::Project,
};

/// 一次隔离试演，choice_ids 仅表示调用者本次明确选择；不读取旧记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftRehearsalRunRequest {
    pub input: DraftRehearsalRequest,
    pub seed: u64,
    #[serde(default)]
    pub budget: ReplayBudget,
    #[serde(default)]
    pub choice_ids: Vec<String>,
    #[serde(default)]
    pub inspection: StateInspectionQuery,
}

impl DraftRehearsalRunRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.input.validate()?;
        if crate::route_comparison::encoded_size(self, MAX_DRAFT_REHEARSAL_REQUEST_BYTES).is_err() {
            return Err("试演完整请求超过32MiB".into());
        }
        if self.seed > 9_007_199_254_740_991 {
            return Err("机器试演 seed 超出 JSON 安全整数范围".into());
        }
        if self.budget.max_steps > 1_000_000 || self.budget.time_budget_ms > 30_000 {
            return Err("单次试演最多 1000000 步和 30000 毫秒；零额度立即暂停".into());
        }
        if self.choice_ids.len() > 256 || self.choice_ids.iter().any(|id| id.len() > 1024) {
            return Err("单次明确选择最多 256 项，每个 ID 最多 1024 字节".into());
        }
        if self.inspection.limit == 0
            || self.inspection.limit > 100
            || self.inspection.text.chars().count() > 256
        {
            return Err("状态查询每页须为 1–100 项，检索词最多 256 字符".into());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct DraftRehearsalRunResult {
    pub ok: bool,
    pub scope: Option<DraftRehearsalScope>,
    pub seed: u64,
    pub outputs: Vec<Output>,
    pub outputs_complete: bool,
    pub outcome: Option<ContinuationOutcome>,
    pub executed_steps: u64,
    pub choices_consumed: usize,
    pub choices: Vec<ChoicePresentation>,
    pub conditions: Vec<ChoiceExplanation>,
    pub state: Option<serde_json::Value>,
    pub inspection: Option<StateInspectionPage>,
    pub error: Option<String>,
}

/// 参数错误返回 Err；编译、基线、选择和故事失败是可审阅的 ok:false。
pub fn run_draft_rehearsal(
    project: &Project,
    request: &DraftRehearsalRunRequest,
    cancellation: &ReplayCancellation,
) -> Result<DraftRehearsalRunResult, String> {
    request.validate()?;
    let mut result = DraftRehearsalRunResult {
        ok: true,
        scope: None,
        seed: request.seed,
        outputs: Vec::new(),
        outputs_complete: true,
        outcome: None,
        executed_steps: 0,
        choices_consumed: 0,
        choices: Vec::new(),
        conditions: Vec::new(),
        state: None,
        inspection: None,
        error: None,
    };
    let snapshot = match project.compile_draft_rehearsal(&request.input) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            fail(&mut result, error);
            return Ok(result);
        }
    };
    result.scope = Some(snapshot.scope().clone());
    let mut session = match DraftRehearsal::new(snapshot, request.seed) {
        Ok(session) => session,
        Err(error) => {
            fail(&mut result, error.to_string());
            return Ok(result);
        }
    };
    let started = crate::execution::MonotonicInstant::now();
    loop {
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let budget = ReplayBudget::new(
            request
                .budget
                .max_steps
                .saturating_sub(result.executed_steps),
            request.budget.time_budget_ms.saturating_sub(elapsed),
        );
        match session.continue_bounded(budget, cancellation) {
            Ok(continued) => {
                result.executed_steps += continued.executed_steps;
                result.outcome = Some(continued.outcome);
                result.outputs.extend(continued.outputs);
            }
            Err(error) => {
                result.executed_steps += session.last_executed_steps();
                result.outputs_complete = session.output_complete();
                result.outcome = None;
                fail(&mut result, error.to_string());
                break;
            }
        }
        if result.outcome != Some(ContinuationOutcome::Choice) {
            break;
        }
        let Some(id) = request.choice_ids.get(result.choices_consumed) else {
            break;
        };
        if let Err(error) = session.choose_id(id) {
            fail(&mut result, error.to_string());
            break;
        }
        result.choices_consumed += 1;
    }
    if result.ok
        && result.outcome == Some(ContinuationOutcome::Ended)
        && result.choices_consumed < request.choice_ids.len()
    {
        fail(
            &mut result,
            "故事已结束，仍有本次明确选择未消费；未忽略多余选择".into(),
        );
    }
    if let Err(error) = session.view_guard() {
        result.outcome = None;
        fail(&mut result, error.to_string());
        result.outputs_complete = false;
        return Ok(result);
    }
    result.choices = session.choice_presentations().to_vec();
    result.conditions = session.choice_evidence().unwrap_or_default().to_vec();
    result.state = session.state_view().ok();
    match session.inspect_state(&request.inspection) {
        Ok(page) => result.inspection = Some(page),
        Err(error) => fail(&mut result, error.message),
    }
    if crate::route_comparison::encoded_size(&result, MAX_DRAFT_REHEARSAL_RESULT_BYTES).is_err() {
        result.outputs.clear();
        result.choices.clear();
        result.conditions.clear();
        result.state = None;
        result.inspection = None;
        result.outputs_complete = false;
        fail(
            &mut result,
            "试演整体响应超过8MiB，已停止；没有返回截断后冒充完整的证据".into(),
        );
    }
    Ok(result)
}

fn fail(result: &mut DraftRehearsalRunResult, message: String) {
    result.ok = false;
    result.error = Some(message);
}

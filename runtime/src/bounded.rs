//! 普通演练预算与可恢复结果，契约见 spec/bounded-execution.md。
use serde::{Deserialize, Serialize};

use super::execution::{MonotonicInstant, ReplayExecutionBudget, ReplayStop};
use super::{Output, ReplayBudget, ReplayCancellation, ReplayStatus, RunError, Story};

pub const BOUNDED_CONTINUE_CAPABILITY: &str = "runtime.bounded_continue.v1";
pub const DEFAULT_CONTINUATION_BUDGET: ReplayBudget = ReplayBudget::new(100_000, 250);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationOutcome {
    Choice,
    Ended,
    StepBudgetExceeded,
    TimeBudgetExceeded,
    Cancelled,
}

impl ContinuationOutcome {
    pub fn is_suspended(self) -> bool {
        !matches!(self, Self::Choice | Self::Ended)
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Choice => "等待选择",
            Self::Ended => "故事已结束",
            Self::StepBudgetExceeded => "演练达到步数预算，已暂停；可继续或提高预算，也可重新开始",
            Self::TimeBudgetExceeded => "演练达到时间预算，已暂停；可继续或提高预算，也可重新开始",
            Self::Cancelled => "演练已取消推进，当前位置保留；可继续或重新开始",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BoundedContinuation {
    pub outputs: Vec<Output>,
    pub outcome: ContinuationOutcome,
    pub executed_steps: u64,
}

impl<'p> Story<'p> {
    /// 每次继续独立计量；零额度表示立即暂停，不关闭保护。
    pub fn set_continuation_budget(&mut self, budget: ReplayBudget) {
        self.continuation_budget = budget;
    }

    pub fn continuation_budget(&self) -> ReplayBudget {
        self.continuation_budget
    }

    /// 取回旧 continue_story 因超限无法随 Err 返回的输出；只交付一次。
    pub fn take_interrupted_outputs(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.interrupted_outputs)
    }

    /// 保持旧 API 形状，但普通推进不再无界。新宿主应使用显式有界接口。
    pub fn continue_story(&mut self) -> Result<Vec<Output>, RunError> {
        let result =
            self.continue_story_bounded(self.continuation_budget, &ReplayCancellation::new())?;
        if result.outcome.is_suspended() {
            self.interrupted_outputs = result.outputs;
            return Err(self.continuation_error(result.outcome));
        }
        Ok(result.outputs)
    }

    pub fn continuation_error(&self, outcome: ContinuationOutcome) -> RunError {
        RunError {
            message: outcome.message().into(),
            node: self.current_node(),
            line: self
                .frames
                .last()
                .and_then(|frame| frame.stmts.get(frame.idx).map(super::util::stmt_line)),
        }
    }

    /// 在完整语句之间暂停，输出/效果/局部变量/RNG 不回滚、不重放。
    pub fn continue_story_bounded(
        &mut self,
        limits: ReplayBudget,
        cancellation: &ReplayCancellation,
    ) -> Result<BoundedContinuation, RunError> {
        let started = MonotonicInstant::now();
        let mut budget = ReplayExecutionBudget {
            limits,
            cancellation,
            started,
            steps: 0,
            slice: None,
            slice_started: started,
            slice_steps: 0,
            comparison_limit: None,
            output_usage: None,
        };
        let result = self.continue_story_inner(&mut budget)?;
        let outcome = match result.stop {
            Some(ReplayStop::Status(ReplayStatus::Cancelled)) => ContinuationOutcome::Cancelled,
            Some(ReplayStop::Status(ReplayStatus::TimeBudgetExceeded)) => {
                ContinuationOutcome::TimeBudgetExceeded
            }
            Some(_) => ContinuationOutcome::StepBudgetExceeded,
            None if self.is_paused() => ContinuationOutcome::Choice,
            None => ContinuationOutcome::Ended,
        };
        self.continuation_outputs
            .extend(result.outputs.iter().cloned());
        if !outcome.is_suspended() {
            let outputs = std::mem::take(&mut self.continuation_outputs);
            self.record_continuation(&outputs);
        }
        let mut outputs = self.take_interrupted_outputs();
        outputs.extend(result.outputs);
        Ok(BoundedContinuation {
            outputs,
            outcome,
            executed_steps: budget.steps,
        })
    }
}

//! 比较专用逐语句边界；普通演练不启用额外计量。
use crate::execution::{ContinueOutcome, ReplayExecutionBudget, ReplayStop};
use crate::{Output, Story};
impl Story<'_> {
    pub(crate) fn comparison_boundary(
        &self,
        budget: &mut ReplayExecutionBudget<'_>,
        outputs: &mut Vec<Output>,
        counted: &mut usize,
    ) -> Option<ReplayStop> {
        if let Some(limit) = budget.comparison_limit {
            if let Err(error) = crate::route_comparison::check_story(self, limit) {
                outputs.clear();
                return Some(ReplayStop::ComparisonRejected(error));
            }
        }
        if let Some(usage) = budget.output_usage.as_mut() {
            if !usage.include(&outputs[*counted..]) {
                outputs.clear();
                return Some(ReplayStop::OutputBudgetExceeded);
            }
        }
        *counted = outputs.len();
        None
    }
    pub(crate) fn comparison_outcome(
        &self,
        budget: &mut ReplayExecutionBudget<'_>,
        mut outputs: Vec<Output>,
        stop: Option<ReplayStop>,
        mut counted: usize,
    ) -> ContinueOutcome {
        let boundary = self.comparison_boundary(budget, &mut outputs, &mut counted);
        ContinueOutcome {
            outputs,
            stop: boundary.or(stop),
        }
    }
}

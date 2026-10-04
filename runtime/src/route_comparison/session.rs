use super::*;
use crate::{
    execution::{MonotonicInstant, ReplayExecutionBudget},
    replay_runner::validate_replay_trace,
    ReplayCancellation, ReplayOrigin, ReplayTrace, REPLAY_SCHEMA_VERSION,
};
use std::time::Duration;
use worldline_core::{CompileOptions, CompileResult};

/// 拥有恢复状态，不借用 CompileResult，适用于 native 与 WASM。
pub struct RouteComparisonSession {
    left: side::Side,
    right: side::Side,
    options: RouteComparisonOptions,
    cancellation: ReplayCancellation,
    started: MonotonicInstant,
    steps: u64,
    outputs: OutputUsage,
    source_snapshot: String,
    frozen_sources: std::collections::BTreeMap<std::path::PathBuf, String>,
    fingerprint: u64,
    compile_options: CompileOptions,
    left_next: bool,
    finished: bool,
}
impl RouteComparisonSession {
    pub fn new(
        snapshot: &CompileResult,
        left: ReplayTrace,
        right: ReplayTrace,
        options: RouteComparisonOptions,
        cancellation: ReplayCancellation,
    ) -> Result<Self, RouteComparisonError> {
        options.validate()?;
        if snapshot.has_errors() {
            return Err(RouteComparisonError::new(
                "invalid_snapshot",
                "当前稿含编译错误，不能比较路线",
            ));
        }
        let source_bytes = snapshot
            .sources
            .iter()
            .try_fold(0usize, |used, (path, text)| {
                used.checked_add(path.as_os_str().len())
                    .and_then(|used| used.checked_add(text.len()))
            });
        if snapshot.sources.len() > 4096
            || source_bytes.is_none_or(|bytes| bytes > 64 * 1024 * 1024)
        {
            return Err(RouteComparisonError::new(
                "invalid_snapshot",
                "比较源快照超过4096文件或64 MiB",
            ));
        }
        for trace in [&left, &right] {
            validate_trace(snapshot, trace, options)?;
        }
        Ok(Self {
            left: side::Side::new(left),
            right: side::Side::new(right),
            options,
            cancellation,
            started: MonotonicInstant::now(),
            steps: 0,
            outputs: OutputUsage::default(),
            source_snapshot: projection::snapshot_digest(snapshot),
            frozen_sources: snapshot.sources.clone(),
            fingerprint: snapshot.analysis.fingerprint,
            compile_options: snapshot.options,
            left_next: true,
            finished: false,
        })
    }
    pub fn advance(
        &mut self,
        snapshot: &CompileResult,
        slice: ReplayBudget,
    ) -> Result<Option<RouteComparisonResult>, RouteComparisonError> {
        if self.finished {
            return Err(RouteComparisonError::new(
                "session_finished",
                "路线对照会话已完成",
            ));
        }
        if snapshot.has_errors()
            || snapshot.options != self.compile_options
            || snapshot.analysis.fingerprint != self.fingerprint
            || snapshot.sources != self.frozen_sources
        {
            self.finished = true;
            return Err(RouteComparisonError::new(
                "snapshot_changed",
                "比较所用稿件或编译选项已变化，请重新比较",
            ));
        }
        if slice.max_steps == 0 || slice.time_budget_ms == 0 {
            return Ok(None);
        }
        let slice_started = MonotonicInstant::now();
        let start_steps = self.steps;
        while self.left.result.is_none() || self.right.result.is_none() {
            let stopping = self.cancellation.is_cancelled()
                || self.outputs.exhausted
                || self.steps >= self.options.budget.max_steps
                || self.started.elapsed()
                    >= Duration::from_millis(self.options.budget.time_budget_ms);
            if !stopping
                && (self.steps - start_steps >= slice.max_steps
                    || slice_started.elapsed() >= Duration::from_millis(slice.time_budget_ms))
            {
                return Ok(None);
            }
            let side =
                if self.left.result.is_none() && (self.left_next || self.right.result.is_some()) {
                    &mut self.left
                } else {
                    &mut self.right
                };
            self.left_next = !self.left_next;
            let remaining = slice
                .max_steps
                .saturating_sub(self.steps - start_steps)
                .min(128);
            let mut budget = ReplayExecutionBudget {
                limits: self.options.budget,
                cancellation: &self.cancellation,
                started: self.started,
                steps: self.steps,
                slice: Some(ReplayBudget::new(remaining, slice.time_budget_ms)),
                slice_started,
                slice_steps: 0,
                comparison_limit: Some(self.options.max_output_bytes),
                output_usage: Some(&mut self.outputs),
            };
            if let Err(error) = side.advance(snapshot, self.options, &mut budget) {
                self.finished = true;
                return Err(error);
            }
            self.steps = budget.steps;
        }
        self.finished = true;
        let mut alignment = projection::alignment(&self.left, &self.right);
        let mut left = self.left.result.take().expect("左侧已完成");
        let mut right = self.right.result.take().expect("右侧已完成");
        limits::check_report(&left, &right, alignment.first_difference.is_some())?;
        projection::verify_sources(snapshot, &mut alignment, &mut left, &mut right)?;
        let differences_complete = left.states.is_some()
            && right.states.is_some()
            && left.vars.is_some()
            && right.vars.is_some();
        let state_differences =
            projection::differences(left.states.as_ref(), right.states.as_ref());
        let variable_differences = projection::differences(left.vars.as_ref(), right.vars.as_ref());
        let result = RouteComparisonResult {
            schema_version: ROUTE_COMPARISON_SCHEMA_VERSION,
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            source_fingerprint: self.fingerprint,
            source_snapshot: self.source_snapshot.clone(),
            omitted: left.omitted || right.omitted,
            left,
            right,
            alignment,
            state_differences,
            variable_differences,
            differences_complete,
        };
        encoded_size(&result, self.options.max_output_bytes)?;
        Ok(Some(result))
    }
}
fn validate_trace(
    snapshot: &CompileResult,
    trace: &ReplayTrace,
    options: RouteComparisonOptions,
) -> Result<(), RouteComparisonError> {
    if trace.steps.len() > options.max_trace_steps
        || encoded_size(trace, options.max_trace_bytes).is_err()
    {
        return Err(RouteComparisonError::new(
            "input_limit",
            "路线trace超过输入步骤或字节限制",
        ));
    }
    validate_replay_trace(trace)
        .map_err(|error| RouteComparisonError::new("invalid_trace", error.message))?;
    if let ReplayOrigin::Checkpoint { checkpoint } = &trace.origin {
        if checkpoint.schema_version != REPLAY_SCHEMA_VERSION
            || checkpoint.runtime_version != env!("CARGO_PKG_VERSION")
            || checkpoint.fingerprint != snapshot.analysis.fingerprint
        {
            return Err(RouteComparisonError::new(
                "invalid_trace",
                "检查点schema/runtime版本或程序fingerprint不匹配",
            ));
        }
        worldline_core::parse_unique_json(checkpoint.state.as_bytes()).map_err(|error| {
            RouteComparisonError::new("invalid_trace", format!("检查点状态无效:{error}"))
        })?;
    }
    Ok(())
}
pub fn compare_routes(
    snapshot: &CompileResult,
    left: &ReplayTrace,
    right: &ReplayTrace,
    options: RouteComparisonOptions,
    cancellation: &ReplayCancellation,
) -> Result<RouteComparisonResult, RouteComparisonError> {
    options.validate()?;
    for trace in [left, right] {
        validate_trace(snapshot, trace, options)?;
    }
    let mut session = RouteComparisonSession::new(
        snapshot,
        left.clone(),
        right.clone(),
        options,
        cancellation.clone(),
    )?;
    loop {
        if let Some(result) = session.advance(snapshot, ReplayBudget::new(256, 16))? {
            return Ok(result);
        }
    }
}

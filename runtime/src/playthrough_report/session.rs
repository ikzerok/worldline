use super::*;
use crate::{
    action_capture::ActionCapture,
    execution::{MonotonicInstant, ReplayExecutionBudget},
    replay_runner::{
        replay_story, run_replay_slice, validate_replay_trace, ReplayCursor, ReplayProgress,
    },
    route_comparison::{check_story, encoded_size, OutputUsage},
    ReplayCancellation, ReplayCheckpoint, ReplayOrigin, ReplayStatus, ReplayTrace, Story,
    REPLAY_SCHEMA_VERSION,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use worldline_core::CompileResult;

pub struct PlaythroughReportSession {
    trace: ReplayTrace,
    options: PlaythroughReportOptions,
    cancellation: ReplayCancellation,
    started: MonotonicInstant,
    steps: u64,
    outputs: OutputUsage,
    frozen_sources: BTreeMap<PathBuf, String>,
    compile_options: CompileOptions,
    fingerprint: u64,
    sources: Option<sources::Sources>,
    cursor: Option<ReplayCursor>,
    checkpoint: Option<ReplayCheckpoint>,
    evidence: ActionCapture,
    inherited_nodes: usize,
    inherited_choices: u64,
    finished: bool,
}
impl PlaythroughReportSession {
    pub fn new(
        snapshot: &CompileResult,
        trace: ReplayTrace,
        options: PlaythroughReportOptions,
        cancellation: ReplayCancellation,
    ) -> Result<Self, PlaythroughReportError> {
        options.validate()?;
        if snapshot.has_errors() {
            return Err(PlaythroughReportError::new(
                "invalid_snapshot",
                "当前稿含编译错误，不能生成审阅记录",
            ));
        }
        validate_trace(snapshot, &trace, options)?;
        encoded_size(&snapshot.program.entry, options.max_output_bytes)?;
        let sources = sources::Sources::new(snapshot, options.max_output_bytes)?;
        encoded_size(&sources.manifest, options.max_output_bytes)?;
        Ok(Self {
            trace,
            options,
            cancellation,
            started: MonotonicInstant::now(),
            steps: 0,
            outputs: OutputUsage::default(),
            frozen_sources: snapshot.sources.clone(),
            compile_options: snapshot.options,
            fingerprint: snapshot.analysis.fingerprint,
            sources: Some(sources),
            cursor: None,
            checkpoint: None,
            evidence: ActionCapture::default(),
            inherited_nodes: 0,
            inherited_choices: 0,
            finished: false,
        })
    }
    pub fn advance(
        &mut self,
        snapshot: &CompileResult,
        slice: ReplayBudget,
    ) -> Result<Option<PlaythroughReport>, PlaythroughReportError> {
        if self.finished {
            return Err(PlaythroughReportError::new(
                "session_finished",
                "审阅记录验证已完成",
            ));
        }
        if snapshot.has_errors()
            || snapshot.options != self.compile_options
            || snapshot.analysis.fingerprint != self.fingerprint
            || snapshot.sources != self.frozen_sources
        {
            self.finished = true;
            return Err(PlaythroughReportError::new(
                "snapshot_changed",
                "验证稿件已变化，请重新生成审阅记录",
            ));
        }
        if self.cancellation.is_cancelled() && self.cursor.is_none() {
            return self.finish(snapshot, RouteStatus::Cancelled, false, false, None);
        }
        if self.trace.initial_observation.is_none() {
            return self.finish(snapshot, RouteStatus::IncompleteTrace, false, false, None);
        }
        if slice.max_steps == 0 || slice.time_budget_ms == 0 {
            return Ok(None);
        }
        let restored = match &self.checkpoint {
            Some(checkpoint) => {
                Story::from_checkpoint(&snapshot.program, &snapshot.analysis, checkpoint)
            }
            None => replay_story(&snapshot.program, &snapshot.analysis, &self.trace),
        };
        let mut story = match restored {
            Ok(story) => story,
            Err(_)
                if self.checkpoint.is_some()
                    || matches!(self.trace.origin, ReplayOrigin::Checkpoint { .. }) =>
            {
                self.finished = true;
                return Err(PlaythroughReportError::new(
                    "invalid_trace",
                    "检查点不能在当前稿恢复",
                ));
            }
            Err(_) => return self.finish(snapshot, RouteStatus::StoryFailed, false, false, None),
        };
        if self.cursor.is_some() {
            story.action_capture = std::mem::take(&mut self.evidence);
        } else {
            // 只要操作次数，不复制变量名/值，也不导出任意 state JSON。
            story.action_capture.enable_variable_writes(0, 0);
            story.action_capture.report_outputs = Some(OutputSources::default());
            check_story(&story, self.options.max_output_bytes)?;
            if matches!(self.trace.origin, ReplayOrigin::Checkpoint { .. }) {
                self.inherited_nodes = story.visits().len();
                self.inherited_choices = story
                    .choice_coverage
                    .values()
                    .map(|c| u64::from(c.count))
                    .sum();
            }
            let mut cursor = ReplayCursor::new(&story);
            cursor.report = Some(ReportObserver::new(
                self.sources.take().expect("来源已建立"),
                self.options.max_output_bytes,
            ));
            self.cursor = Some(cursor);
        }
        let mut budget = ReplayExecutionBudget {
            limits: self.options.budget,
            cancellation: &self.cancellation,
            started: self.started,
            steps: self.steps,
            slice: Some(slice),
            slice_started: MonotonicInstant::now(),
            slice_steps: 0,
            comparison_limit: Some(self.options.max_output_bytes),
            output_usage: Some(&mut self.outputs),
        };
        let progress = run_replay_slice(
            &self.trace,
            self.cursor.as_mut().expect("游标已建立"),
            &mut story,
            &mut budget,
        );
        self.steps = budget.steps;
        match progress {
            ReplayProgress::Yielded => {
                crate::route_comparison::check_checkpoint(&story, self.options.max_output_bytes)?;
                self.checkpoint = Some(story.checkpoint().map_err(|_| {
                    PlaythroughReportError::new("output_limit", "验证检查点不能在额度内保存")
                })?);
                self.evidence = std::mem::take(&mut story.action_capture);
                Ok(None)
            }
            ReplayProgress::Rejected(error) => {
                self.finished = true;
                Err(error.into())
            }
            ReplayProgress::OutputBudgetExceeded => self.finish(
                snapshot,
                RouteStatus::OutputBudgetExceeded,
                false,
                false,
                None,
            ),
            ReplayProgress::Finished(end) => {
                let (status, complete, divergence) = match end.status {
                    ReplayStatus::Replayed { complete, .. } => {
                        (RouteStatus::Replayed, complete, None)
                    }
                    ReplayStatus::Diverged { step_index, .. } => {
                        (RouteStatus::Diverged, false, Some(step_index))
                    }
                    ReplayStatus::StepBudgetExceeded => {
                        (RouteStatus::StepBudgetExceeded, false, None)
                    }
                    ReplayStatus::TimeBudgetExceeded => {
                        (RouteStatus::TimeBudgetExceeded, false, None)
                    }
                    ReplayStatus::Cancelled => (RouteStatus::Cancelled, false, None),
                    ReplayStatus::IncompleteTrace => (RouteStatus::IncompleteTrace, false, None),
                    ReplayStatus::StoryFailed { .. } => (RouteStatus::StoryFailed, false, None),
                };
                self.finish(snapshot, status, story.is_ended(), complete, divergence)
            }
        }
    }
    fn finish(
        &mut self,
        snapshot: &CompileResult,
        status: RouteStatus,
        ended: bool,
        complete: bool,
        divergence_step: Option<usize>,
    ) -> Result<Option<PlaythroughReport>, PlaythroughReportError> {
        self.finished = true;
        let observer = self
            .cursor
            .as_mut()
            .and_then(|cursor| cursor.report.take())
            .unwrap_or_else(|| {
                ReportObserver::new(
                    self.sources.take().expect("来源已建立"),
                    self.options.max_output_bytes,
                )
            });
        let origin = match &self.trace.origin {
            ReplayOrigin::Entry { seed } => RouteOriginSummary {
                kind: "entry".into(),
                seed: crate::util::normalize_seed(*seed),
                checkpoint_fingerprint: None,
                checkpoint_digest: None,
            },
            ReplayOrigin::Checkpoint { checkpoint } => RouteOriginSummary {
                kind: "checkpoint".into(),
                seed: crate::util::normalize_seed(checkpoint.seed),
                checkpoint_fingerprint: Some(checkpoint.fingerprint),
                checkpoint_digest: Some(sources::digest(checkpoint.state.as_bytes())),
            },
        };
        let mut report = PlaythroughReport {
            schema_version: PLAYTHROUGH_REPORT_SCHEMA_VERSION,
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            compile_options: snapshot.options,
            limits: self.options,
            generated_at_unix_ms: now(),
            source_fingerprint: self.fingerprint,
            original_fingerprint: self.trace.fingerprint,
            source_snapshot: observer.sources.snapshot,
            source_base: "applied_sources_common_directory".into(),
            source_manifest: observer.sources.manifest,
            origin,
            entry: snapshot.program.entry.clone(),
            inherited_visited_nodes: self.inherited_nodes,
            inherited_selected_choices: self.inherited_choices,
            status,
            ended,
            complete,
            executed_steps: self.steps,
            verified_choices: observer.verified_choices,
            divergence_step,
            observations: observer.observations,
            pending_choice: observer.pending_choice,
            markdown: String::new(),
        };
        let encoded = encoded_size(&report, self.options.max_output_bytes)?;
        report.markdown = markdown::render(
            &report,
            self.options.max_output_bytes.saturating_sub(encoded),
        )?;
        encoded_size(&report, self.options.max_output_bytes)?;
        Ok(Some(report))
    }
}
fn validate_trace(
    snapshot: &CompileResult,
    trace: &ReplayTrace,
    options: PlaythroughReportOptions,
) -> Result<(), PlaythroughReportError> {
    if trace.steps.len() > options.max_trace_steps
        || encoded_size(trace, options.max_trace_bytes).is_err()
    {
        return Err(PlaythroughReportError::new(
            "input_limit",
            "试玩trace超过输入步骤或字节限制",
        ));
    }
    validate_replay_trace(trace).map_err(|_| {
        PlaythroughReportError::new(
            "invalid_trace",
            "试玩trace的schema/runtime版本或检查点身份不兼容",
        )
    })?;
    if let ReplayOrigin::Checkpoint { checkpoint } = &trace.origin {
        if checkpoint.schema_version != REPLAY_SCHEMA_VERSION
            || checkpoint.runtime_version != env!("CARGO_PKG_VERSION")
            || checkpoint.fingerprint != snapshot.analysis.fingerprint
            || worldline_core::parse_unique_json(checkpoint.state.as_bytes()).is_err()
        {
            return Err(PlaythroughReportError::new(
                "invalid_trace",
                "检查点结构、版本或当前稿指纹不匹配",
            ));
        }
    }
    Ok(())
}
pub fn generate_playthrough_report(
    snapshot: &CompileResult,
    trace: &ReplayTrace,
    options: PlaythroughReportOptions,
    cancellation: &ReplayCancellation,
) -> Result<PlaythroughReport, PlaythroughReportError> {
    options.validate()?;
    validate_trace(snapshot, trace, options)?;
    let mut session =
        PlaythroughReportSession::new(snapshot, trace.clone(), options, cancellation.clone())?;
    loop {
        if let Some(report) = session.advance(snapshot, ReplayBudget::new(256, 16))? {
            return Ok(report);
        }
    }
}
fn now() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    use std::time::{SystemTime, UNIX_EPOCH};
    #[cfg(target_arch = "wasm32")]
    use web_time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| d.as_millis().try_into().ok())
}

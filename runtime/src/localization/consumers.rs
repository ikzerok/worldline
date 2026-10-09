//! 已交付的路线比较与审阅记录共享同一冻结 locale，不另起源文运行。
use super::PresentationContext;
use crate::{
    PlaythroughReport, PlaythroughReportError, PlaythroughReportOptions, PlaythroughReportSession,
    ReplayBudget, ReplayCancellation, ReplayTrace, RouteComparisonError, RouteComparisonOptions,
    RouteComparisonResult, RouteComparisonSession,
};
use worldline_core::{localization::LocalizationPresentationSnapshot, CompileResult};

impl RouteComparisonSession {
    pub fn new_with_presentation(
        snapshot: &CompileResult,
        left: ReplayTrace,
        right: ReplayTrace,
        options: RouteComparisonOptions,
        cancellation: ReplayCancellation,
        presentation: &LocalizationPresentationSnapshot,
    ) -> Result<Self, RouteComparisonError> {
        Self::new_with_context(
            snapshot,
            left,
            right,
            options,
            cancellation,
            Some(PresentationContext::new(presentation)),
        )
    }
}

pub fn compare_routes_with_presentation(
    snapshot: &CompileResult,
    left: &ReplayTrace,
    right: &ReplayTrace,
    options: RouteComparisonOptions,
    cancellation: &ReplayCancellation,
    presentation: &LocalizationPresentationSnapshot,
) -> Result<RouteComparisonResult, RouteComparisonError> {
    let mut session = RouteComparisonSession::new_with_presentation(
        snapshot,
        left.clone(),
        right.clone(),
        options,
        cancellation.clone(),
        presentation,
    )?;
    loop {
        if let Some(result) = session.advance(snapshot, ReplayBudget::new(256, 16))? {
            return Ok(result);
        }
    }
}

impl PlaythroughReportSession {
    pub fn new_with_presentation(
        snapshot: &CompileResult,
        trace: ReplayTrace,
        options: PlaythroughReportOptions,
        cancellation: ReplayCancellation,
        presentation: &LocalizationPresentationSnapshot,
    ) -> Result<Self, PlaythroughReportError> {
        Self::new_with_context(
            snapshot,
            trace,
            options,
            cancellation,
            Some(PresentationContext::new(presentation)),
        )
    }
}

pub fn generate_playthrough_report_with_presentation(
    snapshot: &CompileResult,
    trace: &ReplayTrace,
    options: PlaythroughReportOptions,
    cancellation: &ReplayCancellation,
    presentation: &LocalizationPresentationSnapshot,
) -> Result<PlaythroughReport, PlaythroughReportError> {
    let mut session = PlaythroughReportSession::new_with_presentation(
        snapshot,
        trace.clone(),
        options,
        cancellation.clone(),
        presentation,
    )?;
    loop {
        if let Some(result) = session.advance(snapshot, ReplayBudget::new(256, 16))? {
            return Ok(result);
        }
    }
}

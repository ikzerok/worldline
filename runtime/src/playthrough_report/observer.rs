use super::{
    capture::RawSource, sources::Sources, PlaythroughChoice, PlaythroughObservation,
    PlaythroughText,
};
use crate::{
    route_comparison::encoded_size, ChoiceIdentity, ReplayObservation, RouteComparisonError, Story,
};

pub(crate) struct ReportObserver {
    pub(super) sources: Sources,
    pub(super) observations: Vec<PlaythroughObservation>,
    pub(super) pending_choice: Option<PlaythroughChoice>,
    pub(super) verified_choices: usize,
    max_bytes: usize,
    bytes: usize,
    state_actions: u64,
    variable_writes: u64,
}
impl ReportObserver {
    pub(super) fn new(sources: Sources, maximum: usize) -> Self {
        Self {
            sources,
            observations: Vec::new(),
            pending_choice: None,
            verified_choices: 0,
            max_bytes: maximum,
            bytes: 0,
            state_actions: 0,
            variable_writes: 0,
        }
    }
    pub(crate) fn selected(
        &mut self,
        choice: ChoiceIdentity,
        source: Option<RawSource>,
        localization_status: Option<worldline_core::localization::LocalizationStatus>,
    ) -> Result<(), RouteComparisonError> {
        encoded_size(&choice, self.max_bytes.saturating_sub(self.bytes))?;
        self.pending_choice = Some(PlaythroughChoice {
            localization_status,
            id: choice.id,
            node: choice.node,
            label: choice.label,
            source: source.as_ref().and_then(|s| self.sources.location(s)),
        });
        self.verified_choices += 1;
        Ok(())
    }
    /// `actual` 只能来自同次解释器观察且已通过 observations_match。
    pub(crate) fn record(
        &mut self,
        actual: &ReplayObservation,
        story: &mut Story<'_>,
    ) -> Result<(), RouteComparisonError> {
        if self.observations.len() >= 4097 {
            return Err(limit());
        }
        let capture = story
            .action_capture
            .report_outputs
            .as_mut()
            .expect("报告捕获已开启");
        if capture.exhausted {
            return Err(limit());
        }
        // 真实观察先借用计量，避免克隆大段正文后才发现超额。
        let mut projected =
            encoded_size(&actual.outputs, self.max_bytes.saturating_sub(self.bytes))?;
        let mut sources = capture.take().into_iter();
        let mut texts = Vec::new();
        for output in &actual.outputs {
            if output.get("type").and_then(|v| v.as_str()) != Some("text") {
                continue;
            }
            let raw_source = sources.next().flatten();
            let speaker = output.get("speaker").and_then(|value| {
                Some(worldline_core::TargetRef::new(
                    value.get("kind")?.as_str()?,
                    value.get("id")?.as_str()?,
                ))
            });
            let speaker_label = speaker
                .as_ref()
                .and_then(|speaker| story.catalog.object(speaker))
                .map(|object| object.display.as_str());
            // 同一观察的重复speaker标签与每段source也累计；不能逐段通过后大量复制。
            projected = projected.saturating_add(128);
            if let Some(label) = speaker_label {
                projected = projected.saturating_add(encoded_size(
                    label,
                    self.max_bytes
                        .saturating_sub(self.bytes)
                        .saturating_sub(projected),
                )?);
            }
            if let Some(file) = raw_source
                .as_ref()
                .and_then(|source| self.sources.paths.get(&source.file))
            {
                projected = projected.saturating_add(encoded_size(
                    file,
                    self.max_bytes
                        .saturating_sub(self.bytes)
                        .saturating_sub(projected),
                )?);
            }
            if projected > self.max_bytes.saturating_sub(self.bytes) {
                return Err(limit());
            }
            texts.push(PlaythroughText {
                localization_status: output
                    .get("localization")
                    .and_then(|value| value.get("status"))
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
                content: output
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .into(),
                new_line: output
                    .get("new_line")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                speaker_label: speaker_label.map(str::to_owned),
                speaker,
                source: raw_source.as_ref().and_then(|s| self.sources.location(s)),
            });
        }
        let state_actions = story.state_action_evidence().total_actions;
        let variable_writes = story.variable_write_evidence().total_writes;
        let observation = PlaythroughObservation {
            index: self.observations.len(),
            choice: self.pending_choice.take(),
            texts,
            ended: story.is_ended(),
            state_actions: state_actions.saturating_sub(self.state_actions),
            variable_writes: variable_writes.saturating_sub(self.variable_writes),
        };
        self.bytes = self.bytes.saturating_add(encoded_size(
            &observation,
            self.max_bytes.saturating_sub(self.bytes),
        )?);
        self.state_actions = state_actions;
        self.variable_writes = variable_writes;
        self.observations.push(observation);
        Ok(())
    }
}
fn limit() -> RouteComparisonError {
    RouteComparisonError::new("output_limit", "审阅记录观察或来源超过额度")
}

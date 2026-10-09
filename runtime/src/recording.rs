//! 实际运行观察与检查点；检查器基线只在真实记录时推进。
use super::*;
impl<'p> Story<'p> {
    pub(crate) fn observation(&self, outputs: &[Output]) -> ReplayObservation {
        let choices = self
            .paused
            .as_ref()
            .into_iter()
            .flat_map(|pause| pause.explanations.iter())
            .filter(|explanation| explanation.available)
            .map(|explanation| explanation.choice.clone())
            .collect();
        ReplayObservation {
            outputs: outputs
                .iter()
                .map(|output| serde_json::to_value(output).unwrap_or(serde_json::Value::Null))
                .collect(),
            choices,
            choice_presentation: if self.presentation.is_some()
                || choices::uses_presentation(self.program)
            {
                self.choice_presentations()
                    .iter()
                    .map(|v| serde_json::to_value(v).unwrap())
                    .collect()
            } else {
                Vec::new()
            },
            state: self.state_view(),
        }
    }

    pub(crate) fn record_continuation(&mut self, outputs: &[Output]) {
        let observation = self.observation(outputs);
        let mut recorded = false;
        if self.trace.initial_observation.is_none() {
            self.trace.initial_observation = Some(observation.clone());
            recorded = true;
        } else if let Some(step) = self.trace.steps.last_mut() {
            if step.observation.is_none() {
                step.observation = Some(observation);
                recorded = true;
            }
        }
        self.trace.complete = self.is_ended();
        if recorded {
            self.inspection_record();
        }
    }

    /// 创建绑定 runtime/schema 与程序 fingerprint 的调试检查点。
    pub fn checkpoint(&self) -> Result<ReplayCheckpoint, RunError> {
        let mut state: serde_json::Value = serde_json::from_str(&self.save()?)
            .map_err(|error| RunError::new(format!("检查点状态编码失败:{error}")))?;
        // 暂停组条件和标签的随机表达式在初次呈现时已消耗 RNG；恢复时从组开始状态
        // 重算，保证同一个检查点重新呈现同一组选择。
        if let Some(pause) = self.paused.as_ref().filter(|_| self.presentation.is_none()) {
            state["rng"] = serde_json::json!(pause.rng_before);
        }
        let state = serde_json::to_string(&state)
            .map_err(|error| RunError::new(format!("检查点序列化失败:{error}")))?;
        Ok(ReplayCheckpoint {
            presentation: self.presentation_identity().cloned(),
            schema_version: REPLAY_SCHEMA_VERSION,
            runtime_version: env!("CARGO_PKG_VERSION").into(),
            fingerprint: self.fingerprint,
            seed: self.seed,
            state,
        })
    }

    /// 从严格匹配版本和 fingerprint 的检查点恢复 Story。
    pub fn from_checkpoint(
        program: &'p Program,
        analysis: &'p Analysis,
        checkpoint: &ReplayCheckpoint,
    ) -> Result<Self, RunError> {
        Self::from_checkpoint_with_context(program, analysis, checkpoint, None)
    }

    pub(crate) fn from_checkpoint_with_context(
        program: &'p Program,
        analysis: &'p Analysis,
        checkpoint: &ReplayCheckpoint,
        presentation: Option<localization::PresentationContext>,
    ) -> Result<Self, RunError> {
        localization::require_identity(checkpoint.presentation.as_ref(), presentation.as_ref())?;
        if checkpoint.schema_version != REPLAY_SCHEMA_VERSION {
            return Err(RunError::new("检查点 schema_version 不兼容"));
        }
        if checkpoint.runtime_version != env!("CARGO_PKG_VERSION") {
            return Err(RunError::new("检查点 runtime_version 不兼容"));
        }
        if checkpoint.fingerprint != analysis.fingerprint {
            return Err(RunError::new("检查点程序 fingerprint 不匹配"));
        }
        let story = Self::load_with_context(program, analysis, &checkpoint.state, presentation)?;
        if story.seed != normalize_seed(checkpoint.seed) {
            return Err(RunError::new("检查点 seed 与 runtime 状态不一致"));
        }
        Ok(story)
    }
}

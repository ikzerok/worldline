//! 安全拥有型会话；self_cell 在依赖内部封装自引用，不在应用中伪造生命周期。
use crate::{
    AccessCoverage, AnchorRecord, BoundedContinuation, ChoiceExplanation, ChoicePresentation,
    ChoiceView, Output, ReplayBudget, ReplayCancellation, ReplayCheckpoint, ReplayTrace, RunError,
    StateRecord, Story, Value,
};
use std::collections::{BTreeMap, HashMap};
use worldline_core::{Analysis, Program};

struct StoryOwner {
    program: Program,
    analysis: Analysis,
    #[cfg(test)]
    _probe: Option<std::sync::Arc<()>>,
}

self_cell::self_cell! {
    struct StoryCell {
        owner: StoryOwner,
        #[covariant]
        dependent: Story,
    }
}

/// 一份编译数据与借用该数据的真实 Story；移动或销毁不会泄漏内部借用。
pub struct OwnedStory(StoryCell);

impl OwnedStory {
    pub fn new_localized(
        program: Program,
        analysis: Analysis,
        snapshot: &worldline_core::localization::LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        Self::new_with_presentation(program, analysis, crate::util::seed_now(), snapshot)
    }

    pub fn new_with_presentation(
        program: Program,
        analysis: Analysis,
        seed: u64,
        snapshot: &worldline_core::localization::LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        let presentation = crate::localization::PresentationContext::new(snapshot);
        presentation.validate(&program, &analysis)?;
        let owner = StoryOwner {
            program,
            analysis,
            #[cfg(test)]
            _probe: None,
        };
        StoryCell::try_new(owner, |owner| {
            Story::new_with_context(&owner.program, &owner.analysis, seed, Some(presentation))
        })
        .map(Self)
    }

    pub fn from_checkpoint_with_presentation(
        program: Program,
        analysis: Analysis,
        checkpoint: &ReplayCheckpoint,
        snapshot: &worldline_core::localization::LocalizationPresentationSnapshot,
    ) -> Result<Self, RunError> {
        let presentation = crate::localization::PresentationContext::new(snapshot);
        let owner = StoryOwner {
            program,
            analysis,
            #[cfg(test)]
            _probe: None,
        };
        StoryCell::try_new(owner, |owner| {
            Story::from_checkpoint_with_context(
                &owner.program,
                &owner.analysis,
                checkpoint,
                Some(presentation),
            )
        })
        .map(Self)
    }

    pub fn presentation_identity(&self) -> Option<&crate::RuntimeLocalizationIdentity> {
        self.as_story().presentation_identity()
    }

    pub fn new_with_seed(
        program: Program,
        analysis: Analysis,
        seed: u64,
    ) -> Result<Self, RunError> {
        let owner = StoryOwner {
            program,
            analysis,
            #[cfg(test)]
            _probe: None,
        };
        StoryCell::try_new(owner, |owner| {
            Story::new_with_seed(&owner.program, &owner.analysis, seed)
        })
        .map(Self)
    }

    pub fn from_checkpoint(
        program: Program,
        analysis: Analysis,
        checkpoint: &ReplayCheckpoint,
    ) -> Result<Self, RunError> {
        let owner = StoryOwner {
            program,
            analysis,
            #[cfg(test)]
            _probe: None,
        };
        StoryCell::try_new(owner, |owner| {
            Story::from_checkpoint(&owner.program, &owner.analysis, checkpoint)
        })
        .map(Self)
    }

    /// 借用不超过拥有型会话；编译期验证 Story 的协变性。
    pub fn as_story(&self) -> &Story<'_> {
        self.0.borrow_dependent()
    }

    pub fn is_ended(&self) -> bool {
        self.as_story().is_ended()
    }
    pub fn is_paused(&self) -> bool {
        self.as_story().is_paused()
    }
    pub fn choices(&self) -> &[ChoiceView] {
        self.as_story().choices()
    }
    pub fn choice_presentations(&self) -> &[ChoicePresentation] {
        self.as_story().choice_presentations()
    }
    pub fn choice_evidence(&self) -> Option<&[ChoiceExplanation]> {
        self.as_story().choice_evidence()
    }
    pub fn turns(&self) -> u32 {
        self.as_story().turns()
    }
    pub fn vars(&self) -> &HashMap<String, Value> {
        self.as_story().vars()
    }
    pub fn visits(&self) -> &HashMap<String, u32> {
        self.as_story().visits()
    }
    pub fn current_node(&self) -> Option<String> {
        self.as_story().current_node()
    }
    pub fn storyline(&self) -> &str {
        self.as_story().storyline()
    }
    pub fn anchors(&self) -> &[AnchorRecord] {
        self.as_story().anchors()
    }
    pub fn states(&self) -> &BTreeMap<String, Vec<String>> {
        self.as_story().states()
    }
    pub fn state_history(&self) -> &[StateRecord] {
        self.as_story().state_history()
    }
    pub fn perm_list(&self) -> Vec<String> {
        self.as_story().perm_list()
    }
    pub fn met_list(&self) -> Vec<String> {
        self.as_story().met_list()
    }
    pub fn state_view(&self) -> serde_json::Value {
        self.as_story().state_view()
    }
    pub fn access_coverage(&self) -> AccessCoverage {
        self.as_story().access_coverage()
    }
    pub fn replay_trace(&self) -> ReplayTrace {
        self.as_story().replay_trace()
    }
    pub fn save(&self) -> Result<String, RunError> {
        self.as_story().save()
    }
    pub fn checkpoint(&self) -> Result<ReplayCheckpoint, RunError> {
        self.as_story().checkpoint()
    }
    pub fn explain_choices(&self) -> Result<Vec<ChoiceExplanation>, RunError> {
        self.as_story().explain_choices()
    }
    pub fn restart(&mut self) -> Result<(), RunError> {
        self.0.with_dependent_mut(|_, story| story.restart())
    }
    pub fn choose(&mut self, index: usize) -> Result<(), RunError> {
        self.0.with_dependent_mut(|_, story| story.choose(index))
    }
    pub fn choose_id(&mut self, id: &str) -> Result<(), RunError> {
        self.0.with_dependent_mut(|_, story| story.choose_id(id))
    }
    pub fn choose_presentation(&mut self, index: usize) -> Result<(), RunError> {
        self.0
            .with_dependent_mut(|_, story| story.choose_presentation(index))
    }
    pub fn continue_story(&mut self) -> Result<Vec<Output>, RunError> {
        self.0.with_dependent_mut(|_, story| story.continue_story())
    }
    pub fn continue_story_bounded(
        &mut self,
        budget: ReplayBudget,
        cancellation: &ReplayCancellation,
    ) -> Result<BoundedContinuation, RunError> {
        self.0
            .with_dependent_mut(|_, story| story.continue_story_bounded(budget, cancellation))
    }
    pub(crate) fn continue_draft_bounded(
        &mut self,
        budget: ReplayBudget,
        cancellation: &ReplayCancellation,
        limits: &mut crate::draft_rehearsal::DraftRehearsalLimits,
    ) -> Result<BoundedContinuation, RunError> {
        self.0.with_dependent_mut(|_, story| {
            story.continue_draft_bounded(budget, cancellation, limits)
        })
    }
    pub fn start_trace_from_here(&mut self) -> Result<(), RunError> {
        self.0
            .with_dependent_mut(|_, story| story.start_trace_from_here())
    }
    pub fn set_continuation_budget(&mut self, budget: ReplayBudget) {
        self.0
            .with_dependent_mut(|_, story| story.set_continuation_budget(budget));
    }
    pub fn continuation_budget(&self) -> ReplayBudget {
        self.as_story().continuation_budget()
    }
    pub fn take_interrupted_outputs(&mut self) -> Vec<Output> {
        self.0
            .with_dependent_mut(|_, story| story.take_interrupted_outputs())
    }
}

#[cfg(test)]
mod tests;

mod diff;
mod impacts;

pub(crate) use diff::review_checkpoint_text;
pub(super) use diff::review_differences;
pub(super) use impacts::proposal_reference_impacts;

pub(super) const MAX_REVIEW_DIFFERENCES: usize = 256;

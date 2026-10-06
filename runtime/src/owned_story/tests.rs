use super::*;
use std::sync::Arc;
use worldline_core::{compile_source, compile_source_with_options, CompileOptions};

#[test]
fn owner_is_released_on_success_failure_and_replacement() {
    let compiled = compile_source("owner.wl", "event start\n  choice \"继续\"\n    -> END\n");
    let mut current = None;
    let mut previous: Option<std::sync::Weak<()>> = None;
    for _ in 0..50 {
        let probe = Arc::new(());
        let weak = Arc::downgrade(&probe);
        let cell = StoryCell::try_new(
            StoryOwner {
                program: compiled.program.clone(),
                analysis: compiled.analysis.clone(),
                _probe: Some(probe),
            },
            |owner| Story::new_with_seed(&owner.program, &owner.analysis, 42),
        )
        .unwrap();
        current.replace(OwnedStory(cell));
        if let Some(previous) = previous {
            assert_eq!(previous.strong_count(), 0);
        }
        current.as_mut().unwrap().continue_story().unwrap();
        assert!(current.as_ref().unwrap().is_paused());
        assert_eq!(weak.strong_count(), 1);
        previous = Some(weak);
    }
    drop(current);
    assert_eq!(previous.unwrap().strong_count(), 0);

    let empty = compile_source("empty.wl", "");
    let probe = Arc::new(());
    let weak = Arc::downgrade(&probe);
    let result = StoryCell::try_new(
        StoryOwner {
            program: empty.program,
            analysis: empty.analysis,
            _probe: Some(probe),
        },
        |owner| Story::new_with_seed(&owner.program, &owner.analysis, 42),
    );
    assert!(result.is_err());
    assert_eq!(weak.strong_count(), 0);
}

#[test]
fn moved_owned_story_matches_borrowed_rng_fragments_effects_and_once() {
    let source = concat!(
        "let n = 0\ntag calm\nworld setting\nstate mood on world setting with []\n",
        "fragment inner()\n  choice once \"看潮 {rnd(1, 99)}\"\n    set n = n + 1\n    return\n",
        "fragment outer()\n  call inner()\n  return\n",
        "event start\n  effect on enter\n    become mood add calm\n",
        "  effect on exit\n    become mood remove calm\n",
        "  call outer()\n  call outer()\n  潮声 {n}\n  -> END\n",
    );
    let c = compile_source_with_options("owned.wl", source, CompileOptions::v1_13());
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    let mut borrowed = Story::new_with_seed(&c.program, &c.analysis, 73).unwrap();
    let owned = OwnedStory::new_with_seed(c.program.clone(), c.analysis.clone(), 73).unwrap();
    let mut moved = vec![owned];
    moved.reserve(1);
    moved.push(OwnedStory::new_with_seed(c.program.clone(), c.analysis.clone(), 9).unwrap());
    let owned = &mut moved[0];
    for _ in 0..20 {
        let left = borrowed.continue_story().unwrap();
        let right = owned.continue_story().unwrap();
        assert_eq!(
            serde_json::to_value(left).unwrap(),
            serde_json::to_value(right).unwrap()
        );
        assert_eq!(borrowed.state_view(), owned.state_view());
        assert_eq!(borrowed.save().unwrap(), owned.save().unwrap());
        assert_eq!(borrowed.replay_trace(), owned.replay_trace());
        if borrowed.is_ended() {
            return;
        }
        let id = borrowed.choices()[0].id.clone();
        borrowed.choose_id(&id).unwrap();
        owned.choose_id(&id).unwrap();
    }
    panic!("故事未结束");
}

#[test]
fn owned_budget_cancel_checkpoint_and_story_error_keep_original_semantics() {
    let c = compile_source(
        "budget.wl",
        "event start\n  第一行\n  第二行\n  choice \"结束\"\n    -> END\n",
    );
    let mut owned = OwnedStory::new_with_seed(c.program.clone(), c.analysis.clone(), 5).unwrap();
    let token = ReplayCancellation::new();
    token.cancel();
    let before = owned.save().unwrap();
    let cancelled = owned
        .continue_story_bounded(ReplayBudget::new(1, 1000), &token)
        .unwrap();
    assert_eq!(cancelled.outcome, crate::ContinuationOutcome::Cancelled);
    assert_eq!(owned.save().unwrap(), before);
    loop {
        let result = owned
            .continue_story_bounded(ReplayBudget::new(1, 1000), &ReplayCancellation::new())
            .unwrap();
        if result.outcome == crate::ContinuationOutcome::Choice {
            break;
        }
        assert_eq!(
            result.outcome,
            crate::ContinuationOutcome::StepBudgetExceeded
        );
    }
    let checkpoint = owned.checkpoint().unwrap();
    let restored =
        OwnedStory::from_checkpoint(c.program.clone(), c.analysis.clone(), &checkpoint).unwrap();
    assert_eq!(restored.state_view(), owned.state_view());
    let changed = compile_source("changed.wl", "event other\n  不同正文\n  -> END\n");
    assert!(OwnedStory::from_checkpoint(changed.program, changed.analysis, &checkpoint).is_err());

    let c = compile_source(
        "error.wl",
        "let divisor = 0\nevent start\n  值 {1 / divisor}\n",
    );
    let mut borrowed = Story::new_with_seed(&c.program, &c.analysis, 5).unwrap();
    let mut owned = OwnedStory::new_with_seed(c.program.clone(), c.analysis.clone(), 5).unwrap();
    assert_eq!(
        borrowed.continue_story().unwrap_err().message,
        owned.continue_story().unwrap_err().message
    );
    assert_eq!(borrowed.state_view(), owned.state_view());
}

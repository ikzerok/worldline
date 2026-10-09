use super::*;
use worldline_runtime::{
    OwnedStory, ReplayBudget, ReplayCancellation, ReplaySession, ReplayStatus, ReplayTrace,
};

#[test]
fn localized_save_checkpoint_and_replay_preserve_random_pause_and_completion() {
    let mut f = localized_fixture("persistence");
    let c = f.project.compile();
    let presentation = f.presentation(Policy::Strict);
    let mut story =
        Story::new_with_presentation(&c.program, &c.analysis, 2718, &presentation).unwrap();
    story.continue_story().unwrap();
    let saved = story.save().unwrap();
    let saved_value: Value = serde_json::from_str(&saved).unwrap();
    assert_ne!(saved_value["rng"], saved_value["presentation_pause_rng"]);
    assert!(saved_value["required_features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "runtime.localization.v1"));
    let checkpoint = story.checkpoint().unwrap();
    assert!(Story::load(&c.program, &c.analysis, &saved).is_err());
    assert!(Story::from_checkpoint(&c.program, &c.analysis, &checkpoint).is_err());
    let mut restored =
        Story::load_with_presentation(&c.program, &c.analysis, &saved, &presentation).unwrap();
    let mut resumed = Story::from_checkpoint_with_presentation(
        &c.program,
        &c.analysis,
        &checkpoint,
        &presentation,
    )
    .unwrap();
    assert_eq!(story.state_view(), restored.state_view());
    assert_eq!(story.state_view(), resumed.state_view());
    assert_eq!(
        json!(story.choice_presentations()),
        json!(restored.choice_presentations())
    );
    assert_eq!(rng(&story), rng(&restored));
    assert_eq!(rng(&story), rng(&resumed));
    let choice = story.choices()[0].id.clone();
    for current in [&mut story, &mut restored, &mut resumed] {
        current.choose_id(&choice).unwrap();
        current.continue_story().unwrap();
    }
    assert_eq!(story.state_view(), restored.state_view());
    assert_eq!(story.state_view(), resumed.state_view());
    let trace = story.replay_trace();
    assert!(!trace
        .initial_observation
        .as_ref()
        .unwrap()
        .choice_presentation
        .is_empty());
    assert!(ReplayTrace::replay(
        &c.program,
        &c.analysis,
        &trace,
        ReplayBudget::default(),
        &ReplayCancellation::new()
    )
    .is_err());
    let result = ReplayTrace::replay_with_presentation(
        &c.program,
        &c.analysis,
        &trace,
        ReplayBudget::default(),
        &ReplayCancellation::new(),
        &presentation,
    )
    .unwrap();
    assert!(matches!(
        result.status,
        ReplayStatus::Replayed {
            ended: true,
            complete: true
        }
    ));
    assert_eq!(result.current_state, story.state_view());
    let mut session = ReplaySession::new_with_presentation(
        trace,
        ReplayBudget::default(),
        ReplayCancellation::new(),
        &presentation,
    )
    .unwrap();
    let mut completed = None;
    for _ in 0..100 {
        completed = session
            .advance(&c.program, &c.analysis, ReplayBudget::new(1, 1000))
            .unwrap();
        if completed.is_some() {
            break;
        }
    }
    let completed = completed.expect("cooperative replay reaches END");
    assert!(matches!(
        completed.status,
        ReplayStatus::Replayed { complete: true, .. }
    ));
    assert_eq!(completed.current_state, story.state_view());
    let mut owned = OwnedStory::from_checkpoint_with_presentation(
        c.program.clone(),
        c.analysis.clone(),
        &checkpoint,
        &presentation,
    )
    .unwrap();
    assert_eq!(rng(owned.as_story()), saved_value["rng"]);
    owned.choose_id(&choice).unwrap();
    owned.continue_story().unwrap();
    assert_eq!(owned.state_view(), story.state_view());
}

#[test]
fn checkpoint_origin_with_locale_is_verified_through_end() {
    let mut f = localized_fixture("checkpoint-trace");
    let c = f.project.compile();
    let presentation = f.presentation(Policy::Strict);
    let mut story =
        Story::new_with_presentation(&c.program, &c.analysis, 42, &presentation).unwrap();
    story.continue_story().unwrap();
    story.start_trace_from_here().unwrap();
    let choice = story.choices()[0].id.clone();
    story.choose_id(&choice).unwrap();
    story.continue_story().unwrap();
    let trace = story.replay_trace();
    let result = ReplayTrace::replay_with_presentation(
        &c.program,
        &c.analysis,
        &trace,
        ReplayBudget::default(),
        &ReplayCancellation::new(),
        &presentation,
    )
    .unwrap();
    assert!(matches!(
        result.status,
        ReplayStatus::Replayed { complete: true, .. }
    ));
    assert_eq!(result.current_state, story.state_view());
}

#[test]
fn changed_translation_or_policy_refuses_persistence_even_with_same_program_fingerprint() {
    let mut f = localized_fixture("identity-mismatch");
    let c = f.project.compile();
    let before = f.presentation(Policy::Strict);
    let mut story = Story::new_with_presentation(&c.program, &c.analysis, 101, &before).unwrap();
    story.continue_story().unwrap();
    let saved = story.save().unwrap();
    let checkpoint = story.checkpoint().unwrap();
    let trace = story.replay_trace();
    f.translate("done", vec![text("修正后 "), placeholder("p0")]);
    let after = f.presentation(Policy::Strict);
    assert_eq!(
        f.project.compile().analysis.fingerprint,
        c.analysis.fingerprint
    );
    assert_ne!(before.presentation_digest(), after.presentation_digest());
    assert!(Story::load_with_presentation(&c.program, &c.analysis, &saved, &after).is_err());
    assert!(
        Story::from_checkpoint_with_presentation(&c.program, &c.analysis, &checkpoint, &after)
            .is_err()
    );
    assert!(ReplaySession::new_with_presentation(
        trace.clone(),
        ReplayBudget::default(),
        ReplayCancellation::new(),
        &after
    )
    .is_err());
    assert!(ReplayTrace::replay_with_presentation(
        &c.program,
        &c.analysis,
        &trace,
        ReplayBudget::default(),
        &ReplayCancellation::new(),
        &after
    )
    .is_err());
    let fallback = f.presentation(Policy::SourceFallback);
    assert_ne!(after.presentation_digest(), fallback.presentation_digest());
    assert!(Story::load_with_presentation(&c.program, &c.analysis, &saved, &fallback).is_err());
    assert_eq!(story.save().unwrap(), saved);
}

#[test]
fn source_locations_move_without_replaying_old_location_metadata() {
    let mut f = localized_fixture("relocated-lines");
    let c = f.project.compile();
    let before = f.presentation(Policy::Strict);
    let mut story = Story::new_with_presentation(&c.program, &c.analysis, 23, &before).unwrap();
    story.continue_story().unwrap();
    let id = story.choices()[0].id.clone();
    story.choose_id(&id).unwrap();
    story.continue_story().unwrap();
    let trace = story.replay_trace();
    f.project
        .set_text(&f.root.join("world.wl"), format!("\n\n{SOURCE}"))
        .unwrap();
    let moved = f.project.compile();
    let after = f.presentation(Policy::Strict);
    assert_eq!(before.presentation_digest(), after.presentation_digest());
    let result = ReplayTrace::replay_with_presentation(
        &moved.program,
        &moved.analysis,
        &trace,
        ReplayBudget::default(),
        &ReplayCancellation::new(),
        &after,
    )
    .unwrap();
    assert!(matches!(
        result.status,
        ReplayStatus::Replayed { complete: true, .. }
    ));
    assert_ne!(
        before.entries()[0].source.line,
        after.entries()[0].source.line
    );
    assert!(Story::new_with_presentation(&moved.program, &moved.analysis, 23, &before).is_err());
}

#[test]
fn localized_cancel_budget_and_corrupted_pause_rng_do_not_add_display_evaluation() {
    let mut f = localized_fixture("cancel-budget");
    let c = f.project.compile();
    let presentation = f.presentation(Policy::Strict);
    let mut story =
        Story::new_with_presentation(&c.program, &c.analysis, 88, &presentation).unwrap();
    let before = story.save().unwrap();
    let cancelled = ReplayCancellation::new();
    cancelled.cancel();
    let result = story
        .continue_story_bounded(ReplayBudget::default(), &cancelled)
        .unwrap();
    assert_eq!(
        result.outcome,
        worldline_runtime::ContinuationOutcome::Cancelled
    );
    assert_eq!(story.save().unwrap(), before);
    while !story.is_paused() {
        story
            .continue_story_bounded(ReplayBudget::new(1, 1000), &ReplayCancellation::new())
            .unwrap();
    }
    let mut invalid: Value = serde_json::from_str(&story.save().unwrap()).unwrap();
    invalid["presentation_pause_rng"] = json!(1);
    assert!(Story::load_with_presentation(
        &c.program,
        &c.analysis,
        &invalid.to_string(),
        &presentation
    )
    .is_err());
    invalid
        .as_object_mut()
        .unwrap()
        .remove("presentation_pause_rng");
    assert!(Story::load_with_presentation(
        &c.program,
        &c.analysis,
        &invalid.to_string(),
        &presentation
    )
    .is_err());
}

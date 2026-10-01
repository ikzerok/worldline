//! 正式回归：普通执行预算、可恢复边界与旧接口兼容。
use serde_json::Value;
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{
    ContinuationOutcome, Output, ReplayBudget, ReplayCancellation, ReplayStatus, ReplayTrace,
    Story, DEFAULT_CONTINUATION_BUDGET,
};

fn compile(source: &str, options: CompileOptions) -> CompileResult {
    let result = compile_source_with_options("bounded.wl", source, options);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn json_save(story: &Story<'_>) -> Value {
    serde_json::from_str(&story.save().unwrap()).unwrap()
}
fn advance(story: &mut Story<'_>, steps: u64) -> worldline_runtime::BoundedContinuation {
    story
        .continue_story_bounded(
            ReplayBudget::new(steps, u64::MAX),
            &ReplayCancellation::new(),
        )
        .unwrap()
}

#[test]
fn ordinary_legacy_continue_is_bounded_and_default_is_configurable() {
    let c = compile(
        "event start\n  -> next\nevent next\n  -> start\n",
        CompileOptions::v1_9(),
    );
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    assert_eq!(s.continuation_budget(), DEFAULT_CONTINUATION_BUDGET);
    let error = s.continue_story().unwrap_err();
    assert!(error.message.contains("预算"));
    assert!(!s.is_ended());
    assert!(!s.is_paused());
    s.set_continuation_budget(ReplayBudget::new(1, u64::MAX));
    let before: u32 = s.visits().values().sum();
    assert!(s.continue_story().is_err());
    assert_eq!(s.visits().values().sum::<u32>(), before + 1);
}

#[test]
fn partial_legacy_outputs_are_delivered_once_and_glue_crosses_the_boundary() {
    let c = compile(
        "event start\n  左边。~\n  右边。\n  -> END\n",
        CompileOptions::v1_9(),
    );
    for take in [false, true] {
        let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
        s.set_continuation_budget(ReplayBudget::new(1, u64::MAX));
        assert!(s.continue_story().is_err());
        let mut output = if take {
            s.take_interrupted_outputs()
        } else {
            Vec::new()
        };
        s.set_continuation_budget(ReplayBudget::new(100, u64::MAX));
        output.extend(s.continue_story().unwrap());
        assert_eq!(output.len(), 3);
        assert!(
            matches!(&output[0], Output::Text { content, new_line: true, .. } if content == "左边。")
        );
        assert!(
            matches!(&output[1], Output::Text { content, new_line: false, .. } if content == "右边。")
        );
        assert!(s.take_interrupted_outputs().is_empty());
    }
}

#[test]
fn zero_step_time_and_cancellation_are_distinct_and_preserve_exact_state() {
    let c = compile(
        "event start\n  {rnd(1,100)}\n  -> END\n",
        CompileOptions::v1_9(),
    );
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 9).unwrap();
    let before = json_save(&s);
    assert_eq!(
        advance(&mut s, 0).outcome,
        ContinuationOutcome::StepBudgetExceeded
    );
    let cancel = ReplayCancellation::new();
    let result = s
        .continue_story_bounded(ReplayBudget::new(100, 0), &cancel)
        .unwrap();
    assert_eq!(result.outcome, ContinuationOutcome::TimeBudgetExceeded);
    cancel.cancel();
    let result = s
        .continue_story_bounded(ReplayBudget::new(0, 0), &cancel)
        .unwrap();
    assert_eq!(result.outcome, ContinuationOutcome::Cancelled);
    assert_eq!(result.executed_steps, 0);
    assert!(result.outputs.is_empty());
    assert_eq!(json_save(&s), before);
    assert_eq!(advance(&mut s, 100).outcome, ContinuationOutcome::Ended);
    assert_eq!(
        s.continue_story_bounded(ReplayBudget::new(0, 0), &cancel)
            .unwrap()
            .outcome,
        ContinuationOutcome::Ended
    );
}

const SOURCE: &str = r#"
world w
tag initial
tag marked
tag done
state mood on world w with initial
fragment piece()
  local roll: num = rnd(1, 100)
  左。~
  右:{roll}。
  choice once "记下"
    become mood add marked as "一次"
    return
  choice "隐藏" if false
    -> END
  choice "锁定" enable false disabled "无钥匙"
    -> END
  choice "跳过"
    return
event start
  effect on enter if rnd(1, 1) == 1
    become mood add initial as "进入"
  effect on exit if rnd(1, 1) == 1
    become mood remove initial as "离开"
  call piece()
  call piece()
  -> finish
event finish
  effect on enter
    become mood add initial as "末进入"
  effect on done
    become mood add done as "自然完成"
  effect on exit
    become mood remove initial as "末离开"
  完成。
"#;

fn run(c: &CompileResult, steps: u64, reload: bool) -> (Value, Value, ReplayTrace) {
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 19).unwrap();
    let mut outputs = Vec::new();
    for _ in 0..500 {
        let result = advance(&mut s, steps);
        outputs.extend(result.outputs);
        if result.outcome == ContinuationOutcome::Ended {
            return (
                serde_json::to_value(outputs).unwrap(),
                json_save(&s),
                s.replay_trace(),
            );
        }
        if reload {
            let before = json_save(&s);
            s = Story::load(&c.program, &c.analysis, &s.save().unwrap()).unwrap();
            assert_eq!(
                json_save(&s),
                before,
                "save/load must not repeat effects or RNG"
            );
        }
        if result.outcome == ContinuationOutcome::Choice {
            let before = json_save(&s);
            assert_eq!(advance(&mut s, 0).outcome, ContinuationOutcome::Choice);
            assert_eq!(
                json_save(&s),
                before,
                "choice lookup must not execute anything"
            );
            s.choose(0).unwrap();
        }
    }
    panic!("bounded finite story did not finish");
}

#[test]
fn every_statement_can_resume_or_reload_without_repeating_effects_once_rng_or_fragment_outputs() {
    let c = compile(SOURCE, CompileOptions::v1_12());
    let (expected_output, expected_save, expected_trace) = run(&c, 10_000, false);
    for steps in [1, 2, 3, 7] {
        for reload in [false, true] {
            let (output, save, trace) = run(&c, steps, reload);
            assert_eq!(output, expected_output);
            assert_eq!(save, expected_save);
            assert_eq!(save["turns"], 2);
            assert_eq!(save["state_history"].as_array().unwrap().len(), 6);
            if !reload {
                assert_eq!(trace, expected_trace);
                let result = ReplayTrace::replay(
                    &c.program,
                    &c.analysis,
                    &trace,
                    ReplayBudget::new(10_000, u64::MAX),
                    &ReplayCancellation::new(),
                )
                .unwrap();
                assert!(matches!(
                    result.status,
                    ReplayStatus::Replayed { ended: true, .. }
                ));
            }
        }
    }
}

#[test]
fn hidden_choice_fallthrough_and_long_finite_loop_finish_with_higher_budget() {
    let c = compile("let n = 0\nevent start\n  set n = n + 1\n  choice \"隐藏\" if false\n    -> END\n  if n < 200\n    -> start\n  -> END\n", CompileOptions::v1_9());
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    let first = advance(&mut s, 10);
    assert_eq!(first.outcome, ContinuationOutcome::StepBudgetExceeded);
    assert!(!s.is_paused());
    let before = json_save(&s);
    s = Story::load(&c.program, &c.analysis, &s.save().unwrap()).unwrap();
    assert_eq!(json_save(&s), before);
    assert_eq!(advance(&mut s, 10_000).outcome, ContinuationOutcome::Ended);
    assert_eq!(s.vars()["n"], worldline_runtime::Value::Num(200.0));
}

#[test]
fn legacy_save_shape_and_fingerprint_are_independent_of_budget() {
    let source = "event start\n  正文。\n  -> END\n";
    let baseline = compile(source, CompileOptions::v1_9());
    for options in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
        CompileOptions::v1_12(),
        CompileOptions::v1_13(),
    ] {
        let c = compile(source, options);
        assert_eq!(c.analysis.fingerprint, baseline.analysis.fingerprint);
        let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
        let before = json_save(&s);
        s.set_continuation_budget(ReplayBudget::new(0, 0));
        assert_eq!(json_save(&s), before);
        assert!(before.get("required_features").is_none());
        assert!(before.get("continuation_budget").is_none());
        let mut broken = before;
        broken["frames"][0]["idx"] = 999.into();
        assert!(Story::load(&c.program, &c.analysis, &broken.to_string()).is_err());
    }
}

#[test]
fn exact_boundary_end_and_choice_win_over_budget_and_real_errors_stay_errors() {
    for (source, outcome) in [
        ("event start\n  -> END\n", ContinuationOutcome::Ended),
        (
            "event start\n  choice \"继续\"\n    -> END\n",
            ContinuationOutcome::Choice,
        ),
    ] {
        let c = compile(source, CompileOptions::v1_9());
        let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
        let result = advance(&mut s, 1);
        assert_eq!(result.executed_steps, 1);
        assert_eq!(result.outcome, outcome);
    }
    let c = compile(
        "let n = 0\nevent start\n  set n = 1 / n\n  -> END\n",
        CompileOptions::v1_9(),
    );
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    let error = s
        .continue_story_bounded(ReplayBudget::new(1, u64::MAX), &ReplayCancellation::new())
        .unwrap_err();
    assert!(!error.message.contains("预算"));
    assert!(!error.message.contains("取消"));
}

#[test]
fn restart_clears_suspended_output_and_trace_before_repeating_seeded_story() {
    let c = compile(
        "event start\n  {rnd(1, 100)}\n  -> END\n",
        CompileOptions::v1_9(),
    );
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 11).unwrap();
    s.set_continuation_budget(ReplayBudget::new(1, u64::MAX));
    assert!(s.continue_story().is_err());
    assert!(s.replay_trace().initial_observation.is_none());
    s.restart().unwrap();
    assert!(s.take_interrupted_outputs().is_empty());
    s.set_continuation_budget(ReplayBudget::new(10, u64::MAX));
    let actual = s.continue_story().unwrap();
    let mut fresh = Story::new_with_seed(&c.program, &c.analysis, 11).unwrap();
    let expected = fresh.continue_story().unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(s.replay_trace(), fresh.replay_trace());
}

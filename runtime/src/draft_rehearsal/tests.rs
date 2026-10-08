use super::*;
use crate::{ContinuationOutcome, Output, Story, Value};
use worldline_core::{
    draft_rehearsal::DraftRehearsalRequest, manuscript::WritingBuffer, project::Project,
};

fn fixture(draft: &str) -> (Project, WritingBuffer) {
    let root = std::env::temp_dir().join(format!(
        "runtime-draft-rehearsal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut project = Project::new(&root);
    project
        .set_text(
            &project.entry.clone(),
            "event start\n  已应用正文\n  -> END\n".into(),
        )
        .unwrap();
    project.create_authoring_document(&root.join(".world/project.json"),
        br#"{"schema_version":1,"language_version":"1.13","required_features":["content.choice_presentation.v1"]}"#.to_vec()).unwrap();
    let mut buffer = project.open_source_writing_buffer(&project.entry).unwrap();
    buffer.replace_source(draft.into());
    (project, buffer)
}
fn session(project: &Project, buffer: &WritingBuffer, seed: u64) -> DraftRehearsal {
    let request = DraftRehearsalRequest::from_writing_buffers(
        project,
        std::slice::from_ref(buffer),
        vec![],
        false,
    )
    .unwrap();
    DraftRehearsal::new(project.compile_draft_rehearsal(&request).unwrap(), seed).unwrap()
}
fn text(outputs: Vec<Output>) -> String {
    outputs
        .into_iter()
        .filter_map(|output| match output {
            Output::Text { content, .. } => Some(content),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn independent_real_session_matches_rng_once_fragment_state_and_leaves_formal_story_unchanged() {
    let source = concat!("let n = 0\nfragment show()\n  choice once \"随机 {rnd(1,99)}\"\n",
        "    set n = n + 1\n    return\nevent start\n  未应用正文\n  call show()\n  call show()\n  结果 {n}\n  -> END\n");
    let (mut project, buffer) = fixture(source);
    let applied = project.compile();
    let formal = Story::new_with_seed(&applied.program, &applied.analysis, 73).unwrap();
    let formal_before = formal.save().unwrap();
    let baseline = project.content_baseline();
    let mut draft = session(&project, &buffer, 73);
    let actual = worldline_core::compile_source_with_options(
        &project.entry.display().to_string(),
        source,
        project.compile_options(),
    );
    let mut expected = Story::new_with_seed(&actual.program, &actual.analysis, 73).unwrap();
    for _ in 0..10 {
        let left = draft
            .continue_bounded(ReplayBudget::new(1000, 2000), &ReplayCancellation::new())
            .unwrap();
        let right = expected.continue_story().unwrap();
        assert_eq!(text(left.outputs), text(right));
        assert_eq!(draft.state_view().unwrap(), expected.state_view());
        if draft.is_ended() {
            break;
        }
        let id = draft.choice_presentations()[0].id.clone();
        draft.choose_id(&id).unwrap();
        expected.choose_id(&id).unwrap();
    }
    assert!(draft.is_ended());
    assert_eq!(formal.save().unwrap(), formal_before);
    assert_eq!(project.content_baseline(), baseline);
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("已应用正文"));
}

#[test]
fn cancel_zero_budget_resumption_errors_and_changed_draft_do_not_hot_replace() {
    let (project, mut buffer) =
        fixture("let n = 0\nevent start\n  新稿\n  set n = 2\n  choice \"结束\"\n    -> END\n");
    let mut draft = session(&project, &buffer, 2);
    let token = ReplayCancellation::new();
    token.cancel();
    assert_eq!(
        draft
            .continue_bounded(ReplayBudget::new(10, 2000), &token)
            .unwrap()
            .outcome,
        ContinuationOutcome::Cancelled
    );
    assert_eq!(
        draft
            .continue_bounded(ReplayBudget::new(0, 2000), &ReplayCancellation::new())
            .unwrap()
            .outcome,
        ContinuationOutcome::StepBudgetExceeded
    );
    let mut outputs = String::new();
    loop {
        let part = draft
            .continue_bounded(ReplayBudget::new(1, 2000), &ReplayCancellation::new())
            .unwrap();
        outputs.push_str(&text(part.outputs));
        if part.outcome == ContinuationOutcome::Choice {
            break;
        }
    }
    assert_eq!(outputs, "新稿");
    let page = draft
        .inspect_state(&StateInspectionQuery::default())
        .unwrap();
    assert_eq!(page.items[0].current.value, Some(Value::Num(2.0)));
    let old = draft.state_view().unwrap();
    buffer.replace_source("event start\n  再改新稿\n  -> END\n".into());
    assert!(draft
        .snapshot()
        .verify_current(&project, &[buffer], false)
        .is_err());
    assert_eq!(draft.state_view().unwrap(), old);
    assert!(draft.choose_id("a-choice-from-another-snapshot").is_err());
    assert_eq!(draft.state_view().unwrap(), old);
    let (project, buffer) = fixture("let d = 0\nevent start\n  错误 {1/d}\n");
    let mut draft = session(&project, &buffer, 2);
    assert!(draft
        .continue_bounded(ReplayBudget::new(100, 2000), &ReplayCancellation::new())
        .is_err());
    assert!(!draft.output_complete());
}

#[test]
fn machine_runner_classifies_parameters_and_domain_errors_without_persistent_trace() {
    let (project, buffer) = fixture("event start\n  当前机读草稿\n  choice \"完成\"\n    -> END\n");
    let mut request = DraftRehearsalRunRequest {
        input: DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], vec![], false)
            .unwrap(),
        seed: 7,
        budget: ReplayBudget::new(100, 2000),
        choice_ids: vec![],
        inspection: StateInspectionQuery::default(),
    };
    let result = run_draft_rehearsal(&project, &request, &ReplayCancellation::new()).unwrap();
    assert!(result.ok && result.outputs_complete);
    assert_eq!(text(result.outputs), "当前机读草稿");
    request.choice_ids.push(result.choices[0].id.clone());
    assert_eq!(
        run_draft_rehearsal(&project, &request, &ReplayCancellation::new())
            .unwrap()
            .outcome,
        Some(ContinuationOutcome::Ended)
    );
    request.input.content_baseline = "stale".into();
    assert!(
        !run_draft_rehearsal(&project, &request, &ReplayCancellation::new())
            .unwrap()
            .ok
    );
    request.budget.max_steps = 1_000_001;
    assert!(run_draft_rehearsal(&project, &request, &ReplayCancellation::new()).is_err());
}

#[test]
fn per_statement_output_gate_stops_before_unbounded_accumulation() {
    let (project, buffer) = fixture(&format!(
        "event start\n  choice \"进入循环\"\n    -> loop\nevent loop\n  {}\n  -> loop\n",
        "字".repeat(1024)
    ));
    let mut request = DraftRehearsalRunRequest {
        input: DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], vec![], false)
            .unwrap(),
        seed: 1,
        budget: ReplayBudget::new(100_000, 30_000),
        choice_ids: vec![],
        inspection: StateInspectionQuery::default(),
    };
    let first = run_draft_rehearsal(&project, &request, &ReplayCancellation::new()).unwrap();
    assert_eq!(first.outcome, Some(ContinuationOutcome::Choice));
    request.choice_ids.push(first.choices[0].id.clone());
    let result = run_draft_rehearsal(&project, &request, &ReplayCancellation::new()).unwrap();
    assert!(!result.ok && !result.outputs_complete);
    assert!(result.outcome.is_none(), "输出预算失败不能沿用旧等待选择");
    assert!(result.executed_steps < 100_000);
    assert!(serde_json::to_vec(&result).unwrap().len() <= MAX_DRAFT_REHEARSAL_RESULT_BYTES);
    request.seed = u64::MAX;
    assert!(request.validate().is_err());
}

#[test]
fn machine_runtime_failure_clears_old_outcome_but_unknown_choice_keeps_real_waiting_state() {
    let (project, buffer) = fixture(concat!(
        "let divisor = 0\nevent start\n  等待前正文\n  choice \"除零分支\"\n",
        "    当前计算 {1 / divisor}\n    -> END\n",
    ));
    let mut request = DraftRehearsalRunRequest {
        input: DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], vec![], false)
            .unwrap(),
        seed: 17,
        budget: ReplayBudget::new(100, 2000),
        choice_ids: vec![],
        inspection: StateInspectionQuery::default(),
    };
    let token = ReplayCancellation::new();
    let first = run_draft_rehearsal(&project, &request, &token).unwrap();
    assert!(first.ok);
    assert_eq!(first.outcome, Some(ContinuationOutcome::Choice));
    let actual_id = first.choices[0].id.clone();
    request.choice_ids = vec!["not-a-real-choice".into()];
    let rejected = run_draft_rehearsal(&project, &request, &token).unwrap();
    assert!(!rejected.ok && rejected.outputs_complete);
    assert_eq!(rejected.outcome, Some(ContinuationOutcome::Choice));
    assert_eq!(rejected.choices[0].id, actual_id);
    assert_eq!(rejected.choices_consumed, 0);
    assert_eq!(rejected.state, first.state);
    assert_eq!(
        rejected.inspection.unwrap().status,
        crate::InspectionStatus::Choice
    );
    request.choice_ids = vec![actual_id];
    let failed = run_draft_rehearsal(&project, &request, &token).unwrap();
    assert!(!failed.ok && !failed.outputs_complete);
    assert!(failed.error.as_ref().unwrap().contains("除以零"));
    assert!(failed.outcome.is_none(), "真实错误不能继承旧的等待选择");
    assert_eq!(failed.choices_consumed, 1);
    assert_eq!(text(failed.outputs), "等待前正文");
    assert_eq!(
        failed.inspection.unwrap().status,
        crate::InspectionStatus::Failed
    );
}

#[test]
fn machine_view_budget_failure_clears_prior_choice_and_omits_oversized_evidence() {
    let (project, buffer) = fixture(&format!(
        "event start\n  choice \"进入大选择\"\n    -> large\nevent large\n  choice \"不可选\" enable false disabled \"{}\"\n    -> END\n  choice \"可选\"\n    -> END\n",
        "字".repeat(MAX_DRAFT_REHEARSAL_OUTPUT_BYTES / 3 + 1),
    ));
    let mut request = DraftRehearsalRunRequest {
        input: DraftRehearsalRequest::from_writing_buffers(&project, &[buffer], vec![], false)
            .unwrap(),
        seed: 17,
        budget: ReplayBudget::new(100, 30_000),
        choice_ids: vec![],
        inspection: StateInspectionQuery::default(),
    };
    let token = ReplayCancellation::new();
    let first = run_draft_rehearsal(&project, &request, &token).unwrap();
    assert!(first.ok);
    assert_eq!(first.outcome, Some(ContinuationOutcome::Choice));
    request.choice_ids.push(first.choices[0].id.clone());
    let failed = run_draft_rehearsal(&project, &request, &token).unwrap();
    assert!(!failed.ok && !failed.outputs_complete);
    assert!(failed.error.as_ref().unwrap().contains("选择展示"));
    assert!(
        failed.outcome.is_none(),
        "实际视图超限后不能仍宣称等待旧选择"
    );
    assert_eq!(failed.choices_consumed, 1);
    assert!(failed.choices.is_empty() && failed.conditions.is_empty());
    assert!(failed.state.is_none() && failed.inspection.is_none());
    assert!(serde_json::to_vec(&failed).unwrap().len() <= MAX_DRAFT_REHEARSAL_RESULT_BYTES);
}

use worldline_core::{compile_source, CompileResult};
use worldline_runtime::{ChoiceExplanation, ConditionEvidence, EvidenceOutcome, Story, Value};

fn compile(source: &str) -> CompileResult {
    let result = compile_source("evidence.wl", source);
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    result
}
fn first_evidence(story: &Story<'_>) -> ConditionEvidence {
    story.choice_evidence().unwrap()[0]
        .condition
        .as_ref()
        .unwrap()
        .evidence
        .clone()
        .unwrap()
}

#[test]
fn nested_evidence_preserves_values_parentheses_and_old_expression() {
    let result = compile("let score = 3\nevent start\n  choice \"blocked\" if not ((score + 2 >= 5) and (score * 2 == 6))\n    -> END\n  choice \"finish\"\n    -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 42).unwrap();
    let predicted = story.explain_choices().unwrap();
    assert!(story.choice_evidence().is_none());
    story.continue_story().unwrap();
    let old = story.explain_choices().unwrap();
    assert_eq!(old[0].condition, predicted[0].condition);
    assert!(old[0].condition.as_ref().unwrap().evidence.is_none());
    let evidence = first_evidence(&story);
    assert_eq!(
        evidence.display_expression,
        "not ((((score + 2) >= 5) and ((score * 2) == 6)))"
    );
    assert!(matches!(
        evidence.nodes[0].outcome,
        EvidenceOutcome::Evaluated {
            value: Value::Bool(false)
        }
    ));
    assert!(evidence.nodes.iter().any(|n| n.label == "+"
        && n.outcome
            == EvidenceOutcome::Evaluated {
                value: Value::Num(5.0)
            }));
    assert!(evidence
        .nodes
        .iter()
        .all(|n| !matches!(n.outcome, EvidenceOutcome::NotEvaluated)));
    for (i, node) in evidence.nodes.iter().enumerate() {
        assert!(node.parent.is_none_or(|p| p < i));
    }
}

#[test]
fn repeated_reading_never_changes_rng_choices_state_trace_or_following_output() {
    let result = compile("event start\n  choice \"随机 {rnd(1, 100)}\" if false and rnd(1, 100) > 0\n    -> END\n  choice \"继续 {rnd(1, 100)}\" if true or rnd(1, 100) > 0\n    之后 {rnd(1, 100)}\n    -> END\n");
    let mut inspected = Story::new_with_seed(&result.program, &result.analysis, 17).unwrap();
    let mut control = Story::new_with_seed(&result.program, &result.analysis, 17).unwrap();
    inspected.continue_story().unwrap();
    control.continue_story().unwrap();
    let before_save = inspected.save().unwrap();
    let before_trace = inspected.replay_trace();
    let before_choices = serde_json::to_value(inspected.choices()).unwrap();
    for _ in 0..50 {
        let actual = inspected.choice_evidence().unwrap();
        assert!(actual[0]
            .condition
            .as_ref()
            .unwrap()
            .evidence
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|n| n.label == "rnd" && matches!(n.outcome, EvidenceOutcome::Evaluated { .. })));
        inspected.explain_choices().unwrap();
    }
    assert_eq!(before_save, inspected.save().unwrap());
    assert_eq!(before_trace, inspected.replay_trace());
    assert_eq!(
        before_choices,
        serde_json::to_value(inspected.choices()).unwrap()
    );
    assert!(!before_save.contains("evidence"));
    assert!(!serde_json::to_string(&before_trace)
        .unwrap()
        .contains("evidence"));
    inspected.choose(0).unwrap();
    control.choose(0).unwrap();
    assert!(inspected.choice_evidence().is_none());
    assert_eq!(
        serde_json::to_value(inspected.continue_story().unwrap()).unwrap(),
        serde_json::to_value(control.continue_story().unwrap()).unwrap()
    );
    assert_eq!(inspected.replay_trace(), control.replay_trace());
}

#[test]
fn failed_condition_caches_error_and_only_unexecuted_rhs_is_not_evaluated() {
    let result = compile(
        "event start\n  choice \"失败\" if (rnd(1, 9) / 0 > 0) and (rnd(1, 10) > 0)\n    -> END\n",
    );
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 7).unwrap();
    let error = story.continue_story().unwrap_err();
    assert_eq!(error.message, "除以零");
    let before = story.save().unwrap();
    let evidence = first_evidence(&story);
    assert!(matches!(
        evidence.nodes[0].outcome,
        EvidenceOutcome::Error { .. }
    ));
    let randoms: Vec<_> = evidence.nodes.iter().filter(|n| n.label == "rnd").collect();
    assert_eq!(randoms.len(), 2);
    assert!(matches!(
        randoms[0].outcome,
        EvidenceOutcome::Evaluated { .. }
    ));
    assert_eq!(randoms[1].outcome, EvidenceOutcome::NotEvaluated);
    for _ in 0..50 {
        assert_eq!(first_evidence(&story), evidence);
    }
    assert_eq!(story.save().unwrap(), before);
    assert_eq!(
        story.choice_evidence().unwrap()[0]
            .condition
            .as_ref()
            .unwrap()
            .result,
        None
    );
    story.restart().unwrap();
    assert!(story.choice_evidence().is_none());
}

#[test]
fn once_and_static_state_arguments_have_separate_meanings() {
    let result = compile("tag calm\nworld setting\nstate mood on world setting with calm\nevent start\n  choice once \"loop\" if has(mood, calm)\n    -> start\n  choice \"finish\"\n    -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 7).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let choice = &story.choice_evidence().unwrap()[0];
    assert!(!choice.available);
    assert!(choice.unavailable_reason.as_ref().unwrap().contains("once"));
    assert_eq!(choice.condition.as_ref().unwrap().result, Some(true));
    let evidence = first_evidence(&story);
    assert_eq!(evidence.nodes.len(), 1);
    assert_eq!(evidence.display_expression, "has(mood, calm)");
}

#[test]
fn recording_budget_omits_evidence_without_skipping_true_evaluation() {
    let condition = (0..140)
        .map(|_| "rnd(1, 1) == 1")
        .collect::<Vec<_>>()
        .join(" and ");
    let source = format!("event start\n  choice \"继续\" if {condition}\n    -> END\n");
    let result = compile(&source);
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 19).unwrap();
    let predicted = story.explain_choices().unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.choices().len(), 1);
    assert_eq!(story.explain_choices().unwrap(), predicted);
    let evidence = first_evidence(&story);
    assert!(evidence.omitted);
    assert!(evidence.nodes.len() <= 128);
    assert!(!evidence
        .nodes
        .iter()
        .any(|n| matches!(n.outcome, EvidenceOutcome::NotEvaluated)));
    assert!(serde_json::to_vec(&evidence).unwrap().len() < 64 * 1024);
}

#[test]
fn old_explanation_json_is_accepted_and_serializes_without_evidence() {
    let old = serde_json::json!({"choice":{"id":"x","node":"start","line":1,"offset":0,"label":"x"},"available":false,"condition":{"expression":"false","result":false,"error":null},"unavailable_reason":"条件求值为 false"});
    let parsed: ChoiceExplanation = serde_json::from_value(old.clone()).unwrap();
    assert!(parsed.condition.as_ref().unwrap().evidence.is_none());
    assert_eq!(serde_json::to_value(parsed).unwrap(), old);
}

#[test]
fn label_error_keeps_the_actual_condition_without_reevaluating_it() {
    let result = compile("event start\n  choice \"标签 {1 / 0}\" if rnd(1, 100) > 0\n    -> END\n");
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 7).unwrap();
    let error = story.continue_story().unwrap_err();
    assert_eq!(error.message, "除以零");
    let before = story.save().unwrap();
    let choices = story.choice_evidence().unwrap();
    assert!(!choices[0].available);
    assert!(choices[0]
        .unavailable_reason
        .as_ref()
        .unwrap()
        .starts_with("选择标签求值失败"));
    assert_eq!(choices[0].condition.as_ref().unwrap().result, Some(true));
    assert!(first_evidence(&story)
        .nodes
        .iter()
        .all(|node| matches!(node.outcome, EvidenceOutcome::Evaluated { .. })));
    assert_eq!(story.save().unwrap(), before);
}

#[test]
fn multibyte_static_call_display_reports_omission() {
    let permission = format!("aaa{}", "😀".repeat(600));
    let source = format!("event start\n  choice \"blocked\" if perm(\"{permission}\")\n    -> END\n  choice \"fallback\"\n    -> END\n");
    let result = compile(&source);
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 7).unwrap();
    story.continue_story().unwrap();
    let evidence = first_evidence(&story);
    assert!(evidence.display_expression.len() <= 2048);
    assert!(evidence.omitted);
    assert_eq!(
        evidence.nodes[0].outcome,
        EvidenceOutcome::Evaluated {
            value: Value::Bool(false)
        }
    );
    assert!(!evidence
        .nodes
        .iter()
        .any(|n| n.outcome == EvidenceOutcome::NotEvaluated));
}

#[test]
fn condition_and_group_evidence_budgets_bound_text_and_nodes_without_limiting_choices() {
    let condition = ["token == token"; 10].join(" and ");
    let mut source = format!("let token = \"{}\"\nevent start\n", "x".repeat(2048));
    for index in 0..40 {
        source.push_str(&format!(
            "  choice \"route {index}\" if {condition}\n    -> END\n"
        ));
    }
    let result = compile(&source);
    let mut story = Story::new_with_seed(&result.program, &result.analysis, 7).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.choices().len(), 40);
    let mut group_bytes = 0;
    let mut group_nodes = 0;
    for choice in story.choice_evidence().unwrap() {
        let evidence = choice
            .condition
            .as_ref()
            .unwrap()
            .evidence
            .as_ref()
            .unwrap();
        assert_eq!(choice.condition.as_ref().unwrap().result, Some(true));
        assert!(evidence.omitted);
        let bytes = evidence.display_expression.len()
            + evidence
                .nodes
                .iter()
                .map(|node| {
                    assert!(node.label.len() <= 2048);
                    node.label.len()
                        + match &node.outcome {
                            EvidenceOutcome::Evaluated {
                                value: Value::Str(s),
                            } => {
                                assert!(s.len() <= 2048);
                                s.len()
                            }
                            EvidenceOutcome::Error { message } => message.len(),
                            EvidenceOutcome::NotEvaluated => {
                                panic!("budget exhaustion is not skipped evaluation")
                            }
                            _ => 0,
                        }
                })
                .sum::<usize>();
        assert!(bytes <= 16 * 1024);
        assert!(evidence.nodes.len() <= 128);
        group_bytes += bytes;
        group_nodes += evidence.nodes.len();
    }
    assert!(group_bytes <= 64 * 1024);
    assert!(group_nodes <= 512);
}

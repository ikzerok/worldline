//! 来源扩展只读取实际缓存，默认解释形状与运行语义不变。
use worldline_core::evidence_source::{
    resolve_evidence_source, EvidenceSourceOwner, EvidenceSourcePrecision,
};
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};
use worldline_runtime::{EvidenceOutcome, Story};

fn compile(source: &str) -> CompileResult {
    let result = compile_source_with_options("evidence.wl", source, CompileOptions::v1_12());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}

#[test]
fn actual_choice_and_rule_sources_resolve_without_changing_default_json_or_rng() {
    let compiled = compile("rule allowed(n: num) -> bool = n > 3\nevent start\n  choice \"显示条件\" if allowed(2)\n    -> END\n  choice \"可选条件\" enable allowed(1) disabled \"不满足\"\n    -> END\n  choice \"继续\" if rnd(1, 9) > 0\n    -> END\n");
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 17).unwrap();
    assert!(story.choice_evidence().is_none());
    story.continue_story().unwrap();
    let before = (story.save().unwrap(), story.replay_trace());
    for _ in 0..8 {
        let actual = story.choice_evidence().unwrap();
        for (index, expected_line) in [3, 5, 7].into_iter().enumerate() {
            let source = actual[index].source.as_ref().unwrap();
            let target = resolve_evidence_source(&compiled, source).unwrap();
            assert_eq!(target.line, expected_line);
            assert_eq!(target.precision, EvidenceSourcePrecision::StatementHeader);
            assert!(compiled.sources[&target.path][target.range].starts_with("choice "));
        }
        for condition in [
            actual[0].condition.as_ref(),
            actual[1].enable_condition.as_ref(),
        ] {
            let evidence = condition.unwrap().evidence.as_ref().unwrap();
            let definition = evidence
                .nodes
                .iter()
                .find_map(|node| node.source.as_ref())
                .unwrap();
            assert_eq!(
                definition.owner,
                EvidenceSourceOwner::Rule {
                    name: "allowed".into()
                }
            );
            let target = resolve_evidence_source(&compiled, definition).unwrap();
            assert_eq!(target.line, 1);
            assert_eq!(
                &compiled.sources[&target.path][target.range],
                "rule allowed(n: num) -> bool = n > 3"
            );
        }
        let ordinary = serde_json::to_value(story.explain_choices().unwrap()).unwrap();
        assert!(ordinary
            .as_array()
            .unwrap()
            .iter()
            .all(|choice| choice.get("source").is_none()));
        assert!(!ordinary.to_string().contains("evidence"));
    }
    assert_eq!((story.save().unwrap(), story.replay_trace()), before);
    assert!(!before.0.contains("source"));
    assert!(!serde_json::to_string(&before.1)
        .unwrap()
        .contains("\"source\""));
}

#[test]
fn once_unconditional_and_fragment_choices_keep_the_actual_declaration_source() {
    let compiled = compile("fragment loop()\n  choice once \"一次\"\n    return\n  choice \"退出\"\n    -> END\nevent start\n  call loop()\n  call loop()\n  -> END\n");
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 2).unwrap();
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let once = &story.choice_evidence().unwrap()[0];
    assert!(once.unavailable_reason.as_deref().unwrap().contains("once"));
    assert!(once.condition.is_none());
    let source = once.source.as_ref().unwrap();
    assert_eq!(
        source.owner,
        EvidenceSourceOwner::Choice {
            node: "fragment:loop".into()
        }
    );
    assert_eq!(resolve_evidence_source(&compiled, source).unwrap().line, 2);
}

#[test]
fn rule_error_and_unexecuted_rhs_locate_the_same_real_definition() {
    let compiled = compile("rule broken(n: num) -> bool = (n / 0 > 1) and (n > 3)\nevent start\n  choice \"失败\" enable broken(2) disabled \"失败\"\n    -> END\n");
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 9).unwrap();
    assert!(story.continue_story().is_err());
    let before = story.save().unwrap();
    let evidence = story.choice_evidence().unwrap()[0]
        .enable_condition
        .as_ref()
        .unwrap()
        .evidence
        .as_ref()
        .unwrap();
    assert!(
        evidence
            .nodes
            .iter()
            .any(|node| matches!(node.outcome, EvidenceOutcome::Error { .. })
                && node.source.is_some())
    );
    assert!(evidence
        .nodes
        .iter()
        .any(|node| node.outcome == EvidenceOutcome::NotEvaluated && node.source.is_some()));
    for node in evidence.nodes.iter().filter(|node| node.source.is_some()) {
        assert_eq!(
            resolve_evidence_source(&compiled, node.source.as_ref().unwrap())
                .unwrap()
                .line,
            1
        );
    }
    assert_eq!(story.save().unwrap(), before);
}

#[test]
fn source_identity_and_paths_are_not_recovered_from_equal_labels_or_old_lines() {
    let compiled = compile(
        "event start\n  choice \"相同\" if false\n    -> END\n  choice \"相同\"\n    -> END\n",
    );
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 9).unwrap();
    story.continue_story().unwrap();
    let source = story.choice_evidence().unwrap()[0].source.clone().unwrap();
    for file in ["missing.wl", "moved/evidence.wl", "../outside.wl"] {
        let mut wrong = source.clone();
        wrong.file = file.into();
        assert!(resolve_evidence_source(&compiled, &wrong).is_err());
    }
    let mut wrong = source.clone();
    wrong.owner = EvidenceSourceOwner::Choice {
        node: "renamed".into(),
    };
    assert!(resolve_evidence_source(&compiled, &wrong).is_err());
    wrong = source;
    wrong.line = 3;
    assert!(resolve_evidence_source(&compiled, &wrong).is_err());
}

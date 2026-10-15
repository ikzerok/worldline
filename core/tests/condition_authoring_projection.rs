#[path = "condition_authoring/common.rs"]
mod common;
use common::*;
use worldline_core::{condition_authoring::*, TargetRef};

#[test]
fn advanced_invalid_and_supported_are_distinct_and_keep_complete_source() {
    let fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    for (source, expected) in [
        (
            "ready and not has(mood, alert)",
            ConditionSupport::Supported,
        ),
        (
            "3 <= score or title == \"中文\\\"\\\\🙂\"",
            ConditionSupport::Supported,
        ),
        ("ready == false", ConditionSupport::Supported),
        ("allowed()", ConditionSupport::Advanced),
        ("rnd(0, 1) > 0", ConditionSupport::Advanced),
        ("score + 1 > 0", ConditionSupport::Advanced),
        ("when(ready, true, false)", ConditionSupport::Advanced),
        ("ready and allowed()", ConditionSupport::Advanced),
        ("missing", ConditionSupport::Invalid),
        ("has(missing, open)", ConditionSupport::Invalid),
        ("has(mood, missing)", ConditionSupport::Invalid),
        ("ready > false", ConditionSupport::Invalid),
        ("title < \"z\"", ConditionSupport::Invalid),
        ("score == true", ConditionSupport::Invalid),
        ("ready and", ConditionSupport::Invalid),
    ] {
        let input = input(source);
        let projection = fixture
            .project
            .condition_draft_from_compile(&compiled, &input)
            .unwrap();
        assert_eq!(projection.source, source);
        assert_eq!(projection.support, expected, "{source}: {projection:?}");
        assert_eq!(
            projection.tree.is_some(),
            expected == ConditionSupport::Supported
        );
        if expected != ConditionSupport::Supported {
            assert!(fixture
                .project
                .preview_condition_draft(
                    &compiled,
                    &input,
                    &Some(ConditionNode::Bool { value: true })
                )
                .is_err());
        }
    }
}
#[test]
fn tree_keeps_binary_grouping_order_and_typed_literal_side() {
    let fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    let input = input("ready and (false or true)");
    let projection = fixture
        .project
        .condition_draft_from_compile(&compiled, &input)
        .unwrap();
    let Some(ConditionNode::And { left, right }) = projection.tree else {
        panic!("expected ordered and");
    };
    assert!(matches!(*left, ConditionNode::GlobalBool { .. }));
    assert!(matches!(*right, ConditionNode::Or { .. }));
    let tree = Some(ConditionNode::Compare {
        variable: "score".into(),
        op: ConditionComparison::Le,
        value: ConditionLiteral::Num(-3.5),
        literal_left: true,
    });
    let preview = fixture
        .project
        .preview_condition_draft(&compiled, &input, &tree)
        .unwrap();
    assert_eq!(preview.after_source, "-3.5 <= score");
    let tree = Some(ConditionNode::Compare {
        variable: "title".into(),
        op: ConditionComparison::Eq,
        value: ConditionLiteral::Str("中文🙂\n\"\\".into()),
        literal_left: false,
    });
    assert!(fixture
        .project
        .preview_condition_draft(&compiled, &input, &tree)
        .is_ok());
    let wrong = Some(ConditionNode::Compare {
        variable: "score".into(),
        op: ConditionComparison::Eq,
        value: ConditionLiteral::Bool(true),
        literal_left: false,
    });
    assert!(fixture
        .project
        .preview_condition_draft(&compiled, &input, &wrong)
        .is_err());
}
#[test]
fn parser_and_tree_budgets_retain_original_instead_of_placeholder() {
    let fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    for source in [
        format!("{}true", "not ".repeat(24)),
        format!("{}true{}", "(".repeat(65), ")".repeat(65)),
        " ".repeat(MAX_CONDITION_SOURCE_BYTES + 1),
    ] {
        let projection = fixture
            .project
            .condition_draft_from_compile(&compiled, &input(&source))
            .unwrap();
        assert_eq!(projection.support, ConditionSupport::BudgetExceeded);
        assert_eq!(projection.source, source);
        assert!(projection.tree.is_none());
    }
    let boundary = input(&format!("{}true", "not ".repeat(23)));
    let projection = fixture
        .project
        .condition_draft_from_compile(&compiled, &boundary)
        .unwrap();
    assert_eq!(projection.support, ConditionSupport::Supported);
    let mut tree = ConditionNode::Bool { value: true };
    for _ in 0..24 {
        tree = ConditionNode::Not {
            child: Box::new(tree),
        };
    }
    assert!(fixture
        .project
        .preview_condition_draft(&compiled, &input("ready"), &Some(tree))
        .is_err());
}
#[test]
fn typed_candidates_are_paged_and_state_owner_is_complete() {
    let fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    let mut query = ConditionCandidateQuery {
        schema_version: 1,
        kind: ConditionCandidateKind::Variable,
        text: String::new(),
        offset: 0,
        limit: 1,
    };
    let page = fixture
        .project
        .condition_candidates_from_compile(&compiled, &query)
        .unwrap();
    assert_eq!(page.total, 3);
    assert!(page.has_more);
    assert_eq!(page.items.len(), 1);
    query.kind = ConditionCandidateKind::State;
    let page = fixture
        .project
        .condition_candidates_from_compile(&compiled, &query)
        .unwrap();
    assert_eq!(page.items[0].target, TargetRef::new("state", "mood"));
    assert_eq!(page.items[0].owner, Some(TargetRef::new("world", "realm")));
    query.limit = 101;
    assert!(fixture
        .project
        .condition_candidates_from_compile(&compiled, &query)
        .is_err());
}
#[test]
fn versions_owner_and_strict_protocol_reject_without_language_upgrade() {
    for version in ["1.9", "1.10", "1.11", "1.12", "1.13"] {
        let fixture = Fixture::new(
            "let ready = true\nevent start\n  choice \"go\" if ready\n    -> END\n",
            version,
        );
        let compiled = fixture.compiled();
        let mut value = input("ready");
        value.slot = ConditionSlot::ChoiceEnable;
        value.owner.choice_line = Some(1);
        let projection = fixture
            .project
            .condition_draft_from_compile(&compiled, &value)
            .unwrap();
        assert_eq!(
            projection.support == ConditionSupport::Supported,
            version == "1.12" || version == "1.13"
        );
        value.owner.target.kind = "fragment".into();
        assert!(fixture
            .project
            .condition_draft_from_compile(&compiled, &value)
            .is_err());
    }
    for json in [
        r#"{"schema_version":2,"owner":{"target":{"kind":"event","id":"start"},"choice_line":null},"slot":"event_after"}"#,
        r#"{"schema_version":1,"schema_version":1,"owner":{"target":{"kind":"event","id":"start"},"choice_line":null},"slot":"event_after"}"#,
        r#"{"schema_version":1,"owner":{"target":{"id":"start"},"choice_line":null},"slot":"event_after"}"#,
        r#"{"schema_version":1,"owner":{"target":{"kind":"event","id":"start","extra":1},"choice_line":null},"slot":"event_after"}"#,
    ] {
        assert!(parse_condition_selector(json).is_err());
    }
}

#[test]
fn exact_node_and_source_boundaries_are_accepted_and_one_more_is_rejected() {
    fn balanced(leaves: usize) -> ConditionNode {
        if leaves == 1 {
            ConditionNode::Bool { value: true }
        } else {
            ConditionNode::And {
                left: Box::new(balanced(leaves / 2)),
                right: Box::new(balanced(leaves - leaves / 2)),
            }
        }
    }
    let fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    let tree = ConditionNode::Not {
        child: Box::new(balanced(64)),
    };
    assert!(fixture
        .project
        .preview_condition_draft(&compiled, &input("ready"), &Some(tree.clone()))
        .is_ok());
    let too_many = ConditionNode::Not {
        child: Box::new(tree),
    };
    assert!(fixture
        .project
        .preview_condition_draft(&compiled, &input("ready"), &Some(too_many))
        .is_err());
    let prefix = "title == \"";
    let source = format!(
        "{prefix}{}\"",
        "a".repeat(MAX_CONDITION_SOURCE_BYTES - prefix.len() - 1)
    );
    assert_eq!(source.len(), MAX_CONDITION_SOURCE_BYTES);
    assert_eq!(
        fixture
            .project
            .condition_draft_from_compile(&compiled, &input(&source))
            .unwrap()
            .support,
        ConditionSupport::Supported
    );
    assert_eq!(
        fixture
            .project
            .condition_draft_from_compile(&compiled, &input(&(source + " ")))
            .unwrap()
            .support,
        ConditionSupport::BudgetExceeded
    );
}

#[test]
fn missing_duplicate_or_wrong_owner_never_selects_similar_condition() {
    let fixture = Fixture::new("let ready = true\nevent start\n  scene inner\n    choice \"same\" if ready\n      -> END\nevent other\n  choice \"same\" if ready\n    -> END\n", "1.12");
    let mut selected = selector(ConditionSlot::ChoiceDisplay);
    selected.owner.choice_line = Some(4);
    let projection = fixture.project.condition_authoring(&selected).unwrap();
    assert_eq!(projection.location.unwrap().line, 4);
    selected.owner.choice_line = Some(7);
    assert!(fixture.project.condition_authoring(&selected).is_err());
    selected.owner.target = TargetRef::new("event", "other");
    assert!(fixture.project.condition_authoring(&selected).is_ok());
    selected.owner.target = TargetRef::new("event", "absent");
    assert!(fixture.project.condition_authoring(&selected).is_err());
}

#[test]
fn unparenthesized_chains_are_bounded_before_whole_project_analysis() {
    for expression in [
        vec!["true"; 1024].join(" and "),
        vec!["1"; 1024].join(" + "),
    ] {
        let source = format!("event start after {expression}\n  -> END\n");
        // Read-only open retains original bytes without compiling the hostile expression.
        let fixture = Fixture::new("event start after true\n  -> END\n", "1.12");
        let mut project = fixture.project.clone();
        project
            .set_text(&fixture.root.join("world.wl"), source)
            .unwrap();
        let projection = project
            .condition_authoring(&selector(ConditionSlot::EventAfter))
            .unwrap();
        assert_eq!(projection.support, ConditionSupport::BudgetExceeded);
        assert_eq!(projection.source, expression);
        assert!(!projection.writable);
        assert!(projection.tree.is_none());
    }
}

#[test]
fn qualified_event_identity_supports_all_three_slots() {
    let fixture = Fixture::new("let ready = true\nevent dream.entry after ready\n  choice \"go\" if ready enable ready disabled \"等候\"\n    -> END\n", "1.12");
    for slot in [
        ConditionSlot::EventAfter,
        ConditionSlot::ChoiceDisplay,
        ConditionSlot::ChoiceEnable,
    ] {
        let mut selected = selector(slot);
        selected.owner.target.id = "dream.entry".into();
        selected.owner.choice_line = (slot != ConditionSlot::EventAfter).then_some(3);
        let projection = fixture.project.condition_authoring(&selected).unwrap();
        assert_eq!(
            projection.support,
            ConditionSupport::Supported,
            "{projection:?}"
        );
        assert!(parse_condition_selector(&serde_json::to_string(&selected).unwrap()).is_ok());
    }
}

#[test]
fn query_never_loads_new_untracked_include_sources() {
    let fixture = Fixture::new(
        "include \"later.wl\"\nevent start after true\n  -> END\n",
        "1.12",
    );
    std::fs::write(fixture.root.join("later.wl"), "let added = true\n").unwrap();
    let baseline = fixture.project.content_baseline();
    let projection = fixture
        .project
        .condition_authoring(&selector(ConditionSlot::EventAfter))
        .unwrap();
    assert_eq!(projection.source, "true");
    assert!(!projection.writable);
    assert!(projection
        .read_only_reasons
        .iter()
        .any(|reason| reason.contains("新增源码")));
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(!fixture
        .project
        .documents
        .contains_key(&fixture.root.join("later.wl")));
}

#[test]
fn nonfinite_typed_replacements_are_rejected_without_changing_source() {
    let mut fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    let baseline = fixture.project.content_baseline();
    let generation = fixture
        .project
        .condition_authoring(&selector(ConditionSlot::EventAfter))
        .unwrap()
        .generation;
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let replacement = ConditionNode::Compare {
            variable: "score".into(),
            op: ConditionComparison::Eq,
            value: ConditionLiteral::Num(value),
            literal_left: false,
        };
        assert!(fixture
            .project
            .preview_condition_draft(&compiled, &input("ready"), &Some(replacement.clone()))
            .is_err());
        let request = ConditionAuthoringRequest {
            schema_version: 1,
            expected_baseline: baseline.clone(),
            expected_generation: generation,
            selector: selector(ConditionSlot::EventAfter),
            replacement,
        };
        assert!(fixture
            .project
            .preview_condition_authoring(&request)
            .is_err());
        assert!(fixture
            .project
            .apply_condition_authoring(&request, "untrusted")
            .is_err());
        assert_eq!(fixture.text(), SOURCE);
        assert_eq!(fixture.project.content_baseline(), baseline);
    }
}

#[test]
fn damaged_strings_and_illegal_escapes_keep_complete_invalid_source() {
    let fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    for source in [
        "title == \"未闭合🙂",
        "title == \"非法\\q🙂\"",
        "title == \"尾部\\",
    ] {
        let input = input(source);
        let projection = fixture
            .project
            .condition_draft_from_compile(&compiled, &input)
            .unwrap();
        assert_eq!(projection.source, source);
        assert_eq!(
            projection.support,
            ConditionSupport::Invalid,
            "{projection:?}"
        );
        assert!(projection.tree.is_none());
        assert!(projection
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "P003"
                && diagnostic.severity == worldline_core::Severity::Error));
        assert!(fixture
            .project
            .preview_condition_draft(
                &compiled,
                &input,
                &Some(ConditionNode::Bool { value: true })
            )
            .is_err());
        assert_eq!(fixture.text(), SOURCE);
    }
}

#[test]
fn duplicate_event_owners_reject_every_slot_instead_of_selecting_first() {
    let source = "event start after true\r\n  choice \"第一🙂\" if true enable true disabled \"说明一\"\r\n    -> END\r\nevent start after false\r\n  choice \"第二🙂\" if false enable false disabled \"说明二\"\r\n    -> END\r\n";
    let fixture = Fixture::new(source, "1.12");
    let compiled = fixture.compiled();
    assert!(compiled.has_errors());
    assert_eq!(
        compiled
            .program
            .events
            .iter()
            .filter(|event| event.name == "start")
            .count(),
        2
    );
    let baseline = fixture.project.content_baseline();
    for slot in [
        ConditionSlot::EventAfter,
        ConditionSlot::ChoiceDisplay,
        ConditionSlot::ChoiceEnable,
    ] {
        let mut selected = selector(slot);
        selected.owner.choice_line = (slot != ConditionSlot::EventAfter).then_some(2);
        assert!(fixture.project.condition_authoring(&selected).is_err());
        assert!(fixture
            .project
            .condition_authoring_from_compile(&compiled, &selected)
            .is_err());
        let input = ConditionDraftInput {
            schema_version: 1,
            owner: selected.owner,
            slot,
            generation: 1,
            source: "true".into(),
        };
        assert!(fixture
            .project
            .condition_draft_from_compile(&compiled, &input)
            .is_err());
    }
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(fixture.text(), source);
}

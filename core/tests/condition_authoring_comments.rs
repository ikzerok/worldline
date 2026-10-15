#[path = "condition_authoring/common.rs"]
mod common;

use common::*;
use std::fs;
use worldline_core::{
    authoring::EventDraft, condition_authoring::*, project::Project, Severity, TargetRef,
};

#[derive(Clone, Copy)]
struct SlotCase {
    slot: ConditionSlot,
    original: &'static str,
    changed: &'static str,
    comment_only: &'static str,
    invalid: &'static str,
}

const CASES: [SlotCase; 3] = [
    SlotCase {
        slot: ConditionSlot::EventAfter,
        original: "(ready /*事件条件内部🙂*/ and true)",
        changed: "(not ready /*事件条件已改🙂*/ or false)",
        comment_only: "(ready /*事件条件仅改注释🌙*/ and true)",
        invalid: "(missing_after /*事件无效条件🙂*/ and true)",
    },
    SlotCase {
        slot: ConditionSlot::ChoiceDisplay,
        original: "(ready /*显示条件内部🙂*/ or false)",
        changed: "(ready /*显示条件已改🙂*/ and false)",
        comment_only: "(ready /*显示条件仅改注释🌙*/ or false)",
        invalid: "(missing_display /*显示无效条件🙂*/ or false)",
    },
    SlotCase {
        slot: ConditionSlot::ChoiceEnable,
        original: "(score /*可选条件内部🙂*/ >= 3)",
        changed: "(score /*可选条件已改🙂*/ >= 5)",
        comment_only: "(score /*可选条件仅改注释🌙*/ >= 3)",
        invalid: "(missing_enable /*可选无效条件🙂*/ >= 3)",
    },
];

fn source() -> String {
    // 不插入前置物理行：event/choice 继续使用 common 的第 9/10 行身份。
    let source = SOURCE
        .replacen(
            "world realm as \"世界\"\r\n",
            "world realm as \"世界\" // 文件头🙂\r\n",
            1,
        )
        .replacen(
            "after ((ready))",
            &format!("after {}", CASES[0].original),
            1,
        )
        .replacen("if ((ready))", &format!("if {}", CASES[1].original), 1)
        .replacen(
            "enable score >= 3",
            &format!("enable {}", CASES[2].original),
            1,
        );
    format!("{source}// 文件尾🙂\r\n")
}

fn valid_fixture() -> Fixture {
    let fixture = Fixture::new(&source(), "1.12");
    let compiled = fixture.compiled();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    fixture
}

fn raw_field(draft: &EventDraft, slot: ConditionSlot) -> String {
    match slot {
        ConditionSlot::EventAfter => draft.after.clone(),
        ConditionSlot::ChoiceDisplay => draft.choices()[0].condition.clone(),
        ConditionSlot::ChoiceEnable => draft.choices()[0].enable_condition.clone(),
    }
}

fn form_input(project: &Project, slot: ConditionSlot) -> ConditionDraftInput {
    let (_, draft) = project.event_draft("start").unwrap();
    let choices = draft.choices();
    assert_eq!(choices.len(), 1);
    ConditionDraftInput {
        schema_version: 1,
        owner: ConditionOwner {
            target: TargetRef::new("event", "start"),
            choice_line: (slot != ConditionSlot::EventAfter).then_some(choices[0].line),
        },
        slot,
        generation: 17,
        source: raw_field(&draft, slot),
    }
}

fn assert_raw_advanced(project: &Project, slot: ConditionSlot, expected: &str) {
    let compiled = worldline_core::compile_sources_with_options(
        &project.entry,
        &project.sources(),
        project.compile_options(),
    );
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let direct = project.condition_authoring(&selector(slot)).unwrap();
    assert_eq!(direct.source, expected, "{slot:?}");
    assert_eq!(direct.support, ConditionSupport::Advanced, "{direct:?}");
    assert!(direct.tree.is_none());
    assert!(!direct.writable);
    let location = direct.location.as_ref().unwrap();
    assert_eq!(
        location.line,
        if slot == ConditionSlot::EventAfter {
            9
        } else {
            10
        }
    );
    let text = project
        .document(std::path::Path::new(&location.file))
        .unwrap();
    assert_eq!(&text[location.byte_range.clone()], expected);

    let input = form_input(project, slot);
    assert_eq!(input.source, direct.source, "{slot:?}");
    if slot != ConditionSlot::EventAfter {
        assert_eq!(input.owner.choice_line, Some(1));
    }
    let projected = project
        .condition_draft_from_compile(&compiled, &input)
        .unwrap();
    assert_eq!(projected.source, direct.source, "{slot:?}");
    assert_eq!(
        projected.support,
        ConditionSupport::Advanced,
        "{projected:?}"
    );
    assert!(projected.tree.is_none());
    assert!(!projected.writable);
    assert_eq!(projected.input_generation, input.generation);
}

fn edit_field(draft: &mut EventDraft, case: SlotCase, replacement: &str) {
    if case.slot == ConditionSlot::EventAfter {
        draft.after = replacement.into();
        return;
    }
    let old_body = draft.body.clone();
    let mut choice = draft.choices().remove(0);
    match case.slot {
        ConditionSlot::ChoiceDisplay => choice.condition = replacement.into(),
        ConditionSlot::ChoiceEnable => choice.enable_condition = replacement.into(),
        ConditionSlot::EventAfter => unreachable!(),
    }
    draft.write_choice(Some(choice.line), &choice).unwrap();
    assert_eq!(
        draft.body,
        old_body.replacen(case.original, replacement, 1),
        "只替换 {slot:?} 的表达式范围",
        slot = case.slot
    );
    assert_eq!(raw_field(draft, case.slot), replacement);
}

#[test]
fn commented_three_slots_keep_raw_drafts_and_reject_typed_writes() {
    let mut fixture = valid_fixture();
    let original = source();
    let baseline = fixture.project.content_baseline();
    let compiled = fixture.compiled();
    let path = fixture.root.join("world.wl");
    for case in CASES {
        assert_raw_advanced(&fixture.project, case.slot, case.original);
        let input = form_input(&fixture.project, case.slot);
        for replacement in [None, Some(ConditionNode::Bool { value: false })] {
            assert!(fixture
                .project
                .preview_condition_draft(&compiled, &input, &replacement)
                .is_err());

            // 即使拿到同 owner/代次的有效树预览，也不能用它覆盖含注释原稿。
            let mut plain_input = input.clone();
            plain_input.source = "ready".into();
            let preview = fixture
                .project
                .preview_condition_draft(&compiled, &plain_input, &replacement)
                .unwrap();
            assert!(fixture
                .project
                .apply_condition_draft(&compiled, &input, &preview)
                .is_err());
        }
        let request = request(
            &fixture.project,
            case.slot,
            ConditionNode::Bool { value: false },
        );
        assert!(fixture
            .project
            .preview_condition_authoring(&request)
            .is_err());
        assert!(fixture
            .project
            .apply_condition_authoring(&request, "untrusted")
            .is_err());
        assert_eq!(form_input(&fixture.project, case.slot), input);
        assert_eq!(fixture.text(), original);
        assert_eq!(fixture.project.content_baseline(), baseline);
        assert_eq!(fs::read(&path).unwrap().as_slice(), original.as_bytes());
    }
}

#[test]
fn commented_three_slots_form_noops_preserve_every_byte() {
    for case in CASES {
        let mut fixture = valid_fixture();
        let original = source();
        let baseline = fixture.project.content_baseline();
        let fingerprint = fixture.compiled().analysis.fingerprint;
        let (path, mut draft) = fixture.project.event_draft("start").unwrap();
        assert_eq!(raw_field(&draft, case.slot), case.original);
        fixture
            .project
            .write_event(&path, Some("start"), &draft)
            .unwrap();
        assert_eq!(fixture.text(), original);
        assert_eq!(fixture.project.content_baseline(), baseline);

        let body = draft.body.clone();
        let choice = draft.choices().remove(0);
        draft.write_choice(Some(choice.line), &choice).unwrap();
        assert_eq!(draft.body, body);
        fixture
            .project
            .write_event(&path, Some("start"), &draft)
            .unwrap();
        assert_eq!(fixture.text(), original);
        assert_eq!(fixture.project.content_baseline(), baseline);
        assert_eq!(fixture.compiled().analysis.fingerprint, fingerprint);
        fixture.project.save().unwrap();
        assert_eq!(fs::read(&path).unwrap().as_slice(), original.as_bytes());
        let reopened = Project::open(&fixture.root).unwrap();
        assert_eq!(reopened.document(&path).unwrap(), original);
        assert_raw_advanced(&reopened, case.slot, case.original);
    }
}

#[test]
fn commented_manual_edits_and_comment_only_edits_save_exactly_and_reopen() {
    for case in CASES {
        for (replacement, semantic_change) in [(case.changed, true), (case.comment_only, false)] {
            let mut fixture = valid_fixture();
            let original = source();
            let expected = original.replacen(case.original, replacement, 1);
            let before_fingerprint = fixture.compiled().analysis.fingerprint;
            let baseline = fixture.project.content_baseline();
            let manifest_path = fixture.root.join(".world/project.json");
            let manifest = fs::read(&manifest_path).unwrap();
            let (path, mut draft) = fixture.project.event_draft("start").unwrap();
            edit_field(&mut draft, case, replacement);
            assert_eq!(fixture.text(), original);
            fixture
                .project
                .write_event(&path, Some("start"), &draft)
                .unwrap();
            // 全串比较覆盖正文、其它条件、once/disabled、本地化、头尾注释及 CRLF。
            assert_eq!(fixture.text(), expected, "{:?}", case.slot);
            assert_ne!(fixture.project.content_baseline(), baseline);
            assert_eq!(fs::read(&path).unwrap().as_slice(), original.as_bytes());
            let after = fixture.compiled();
            assert!(!after.has_errors(), "{:?}", after.diagnostics);
            assert_eq!(
                before_fingerprint != after.analysis.fingerprint,
                semantic_change,
                "{:?}: {replacement}",
                case.slot
            );
            fixture.project.save().unwrap();
            assert_eq!(fs::read(&path).unwrap().as_slice(), expected.as_bytes());
            assert_eq!(fs::read(&manifest_path).unwrap(), manifest);

            let mut reopened = Project::open(&fixture.root).unwrap();
            assert_eq!(reopened.document(&path).unwrap(), expected);
            for other in CASES {
                let raw = if other.slot == case.slot {
                    replacement
                } else {
                    other.original
                };
                assert_raw_advanced(&reopened, other.slot, raw);
            }
            let reopened_baseline = reopened.content_baseline();
            let (_, mut draft) = reopened.event_draft("start").unwrap();
            let choice = draft.choices().remove(0);
            draft.write_choice(Some(choice.line), &choice).unwrap();
            reopened.write_event(&path, Some("start"), &draft).unwrap();
            assert_eq!(reopened.document(&path).unwrap(), expected);
            assert_eq!(reopened.content_baseline(), reopened_baseline);
            assert_eq!(fs::read(&path).unwrap().as_slice(), expected.as_bytes());
        }
    }
}

#[test]
fn commented_invalid_manual_edits_fail_final_compile_without_committing() {
    for case in CASES {
        let mut fixture = valid_fixture();
        let original = source();
        let baseline = fixture.project.content_baseline();
        let fingerprint = fixture.compiled().analysis.fingerprint;
        let (path, mut draft) = fixture.project.event_draft("start").unwrap();
        let mut candidate_sources = fixture.project.sources();
        candidate_sources.insert(
            path.clone(),
            original.replacen(case.original, case.invalid, 1),
        );
        let candidate = worldline_core::compile_sources_with_options(
            &fixture.project.entry,
            &candidate_sources,
            fixture.project.compile_options(),
        );
        assert!(candidate.has_errors());
        let diagnostic = candidate
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == Severity::Error)
            .unwrap();

        // 手动字段可保留尚未通过语义校验的输入，提交仍由完整候选编译守卫。
        edit_field(&mut draft, case, case.invalid);
        let error = fixture
            .project
            .write_event(&path, Some("start"), &draft)
            .unwrap_err();
        assert!(error.contains(diagnostic.code), "{error}");
        assert_eq!(raw_field(&draft, case.slot), case.invalid);
        assert_eq!(fixture.text(), original);
        assert_eq!(fixture.project.content_baseline(), baseline);
        assert_eq!(fixture.compiled().analysis.fingerprint, fingerprint);
        assert_eq!(fs::read(&path).unwrap().as_slice(), original.as_bytes());
        assert_raw_advanced(&fixture.project, case.slot, case.original);
    }
}

#[test]
fn commented_manual_edits_preserve_external_disk_change_guard() {
    for case in CASES {
        let mut fixture = valid_fixture();
        let original = source();
        let external = original.replacen("正文🙂。", "磁盘另改正文🌙。", 1);
        let baseline = fixture.project.content_baseline();
        let (path, mut draft) = fixture.project.event_draft("start").unwrap();
        edit_field(&mut draft, case, case.changed);
        fs::write(&path, &external).unwrap();
        assert!(fixture
            .project
            .write_event(&path, Some("start"), &draft)
            .is_err());
        assert_eq!(raw_field(&draft, case.slot), case.changed);
        assert_eq!(fixture.text(), original);
        assert_eq!(fixture.project.content_baseline(), baseline);
        assert_eq!(fs::read(&path).unwrap().as_slice(), external.as_bytes());
    }
}

#[test]
fn explicitly_removing_internal_comments_reopens_all_three_slots_as_supported() {
    for case in CASES {
        let mut fixture = valid_fixture();
        let original = source();
        let fingerprint = fixture.compiled().analysis.fingerprint;
        let (replacement, expected_tree) = match case.slot {
            ConditionSlot::EventAfter => (
                "(ready and true)",
                ConditionNode::And {
                    left: Box::new(ConditionNode::GlobalBool {
                        variable: "ready".into(),
                    }),
                    right: Box::new(ConditionNode::Bool { value: true }),
                },
            ),
            ConditionSlot::ChoiceDisplay => (
                "(ready or false)",
                ConditionNode::Or {
                    left: Box::new(ConditionNode::GlobalBool {
                        variable: "ready".into(),
                    }),
                    right: Box::new(ConditionNode::Bool { value: false }),
                },
            ),
            ConditionSlot::ChoiceEnable => (
                "(score >= 3)",
                ConditionNode::Compare {
                    variable: "score".into(),
                    op: ConditionComparison::Ge,
                    value: ConditionLiteral::Num(3.0),
                    literal_left: false,
                },
            ),
        };
        let expected = original.replacen(case.original, replacement, 1);
        let (path, mut draft) = fixture.project.event_draft("start").unwrap();
        edit_field(&mut draft, case, replacement);
        fixture
            .project
            .write_event(&path, Some("start"), &draft)
            .unwrap();
        assert_eq!(fixture.text(), expected);
        assert_eq!(fs::read(&path).unwrap().as_slice(), original.as_bytes());
        let compiled = fixture.compiled();
        assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
        assert_eq!(compiled.analysis.fingerprint, fingerprint);
        fixture.project.save().unwrap();
        assert_eq!(fs::read(&path).unwrap().as_slice(), expected.as_bytes());

        let reopened = Project::open(&fixture.root).unwrap();
        assert_eq!(reopened.document(&path).unwrap(), expected);
        let projected = reopened.condition_authoring(&selector(case.slot)).unwrap();
        assert_eq!(projected.source, replacement);
        assert_eq!(projected.support, ConditionSupport::Supported);
        assert_eq!(projected.tree, Some(expected_tree.clone()));
        assert!(projected.writable);
        let input = form_input(&reopened, case.slot);
        assert_eq!(input.source, replacement);
        let compiled = worldline_core::compile_sources_with_options(
            &reopened.entry,
            &reopened.sources(),
            reopened.compile_options(),
        );
        let projected = reopened
            .condition_draft_from_compile(&compiled, &input)
            .unwrap();
        assert_eq!(projected.source, replacement);
        assert_eq!(projected.support, ConditionSupport::Supported);
        assert_eq!(projected.tree, Some(expected_tree));
        for other in CASES {
            if other.slot != case.slot {
                assert_raw_advanced(&reopened, other.slot, other.original);
            }
        }
    }
}

#[test]
fn direct_body_condition_and_comment_edits_preserve_exact_choice_source() {
    for case in CASES {
        if case.slot == ConditionSlot::EventAfter {
            continue;
        }
        for replacement in [case.changed, case.comment_only] {
            let mut fixture = valid_fixture();
            let original = source();
            let expected = original.replacen(case.original, replacement, 1);
            let (path, mut draft) = fixture.project.event_draft("start").unwrap();
            let original_body = draft.body.clone();
            // 正文高级编辑入口直接提交完整 body，不经过 write_choice。
            draft.body = draft.body.replacen(case.original, replacement, 1);
            assert_ne!(draft.body, original_body);
            assert_eq!(raw_field(&draft, case.slot), replacement);
            fixture
                .project
                .write_event(&path, Some("start"), &draft)
                .unwrap();
            assert_eq!(fixture.text(), expected);
            assert_eq!(fs::read(&path).unwrap().as_slice(), original.as_bytes());
            let compiled = fixture.compiled();
            assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
            fixture.project.save().unwrap();
            assert_eq!(fs::read(&path).unwrap().as_slice(), expected.as_bytes());

            let reopened = Project::open(&fixture.root).unwrap();
            assert_eq!(reopened.document(&path).unwrap(), expected);
            for other in CASES {
                let raw = if other.slot == case.slot {
                    replacement
                } else {
                    other.original
                };
                assert_raw_advanced(&reopened, other.slot, raw);
            }
        }
    }
}

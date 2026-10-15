#[path = "condition_authoring/common.rs"]
mod common;
use common::*;
use worldline_core::{condition_authoring::*, project::Project};

#[test]
fn three_slots_noop_preserves_every_byte_and_fingerprint() {
    let mut fixture = Fixture::new(SOURCE, "1.12");
    let fingerprint = fixture.compiled().analysis.fingerprint;
    let baseline = fixture.project.content_baseline();
    for slot in [
        ConditionSlot::ChoiceDisplay,
        ConditionSlot::ChoiceEnable,
        ConditionSlot::EventAfter,
    ] {
        let projection = fixture
            .project
            .condition_authoring(&selector(slot))
            .unwrap();
        assert_eq!(
            projection.support,
            ConditionSupport::Supported,
            "{projection:?}"
        );
        let request = request(&fixture.project, slot, projection.tree.unwrap());
        let preview = fixture
            .project
            .preview_condition_authoring(&request)
            .unwrap();
        assert!(!preview.changed);
        assert!(preview.patch.is_none());
        fixture
            .project
            .apply_condition_authoring(&request, &preview.plan_digest)
            .unwrap();
        assert_eq!(fixture.text(), SOURCE);
    }
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(fixture.compiled().analysis.fingerprint, fingerprint);
}

#[test]
fn exact_slot_patch_save_reopen_and_snapshot_undo_preserve_other_bytes() {
    for (slot, replacement, old, new) in [
        (
            ConditionSlot::ChoiceDisplay,
            ConditionNode::Has {
                state_id: "mood".into(),
                tag_id: "alert".into(),
            },
            "if ((ready))",
            "if has(mood, alert)",
        ),
        (
            ConditionSlot::ChoiceEnable,
            ConditionNode::Bool { value: false },
            "enable score >= 3",
            "enable false",
        ),
        (
            ConditionSlot::EventAfter,
            ConditionNode::Not {
                child: Box::new(ConditionNode::GlobalBool {
                    variable: "ready".into(),
                }),
            },
            "after ((ready))",
            "after not (ready)",
        ),
    ] {
        let mut fixture = Fixture::new(SOURCE, "1.12");
        let snapshot = fixture.project.clone();
        let request = request(&fixture.project, slot, replacement.clone());
        let preview = fixture
            .project
            .preview_condition_authoring(&request)
            .unwrap();
        assert_eq!(fixture.text(), SOURCE);
        fixture
            .project
            .apply_condition_authoring(&request, &preview.plan_digest)
            .unwrap();
        assert_eq!(fixture.text(), SOURCE.replacen(old, new, 1));
        fixture.project.save().unwrap();
        let reopened = Project::open(&fixture.root).unwrap();
        assert_eq!(
            reopened.condition_authoring(&selector(slot)).unwrap().tree,
            Some(replacement)
        );
        assert!(fixture.project.restore(snapshot));
        assert_eq!(fixture.text(), SOURCE);
    }
}

#[test]
fn event_and_choice_form_roundtrip_only_changes_expression_bytes() {
    let mut fixture = Fixture::new(SOURCE, "1.12");
    let (path, mut draft) = fixture.project.event_draft("start").unwrap();
    fixture
        .project
        .write_event(&path, Some("start"), &draft)
        .unwrap();
    assert_eq!(fixture.text(), SOURCE);
    let mut choice = draft.choices().remove(0);
    choice.condition = "has(mood, alert)".into();
    draft.write_choice(Some(choice.line), &choice).unwrap();
    draft.after = "not ready".into();
    fixture
        .project
        .write_event(&path, Some("start"), &draft)
        .unwrap();
    let expected = SOURCE
        .replace("if ((ready))", "if has(mood, alert)")
        .replace("after ((ready))", "after not ready");
    assert_eq!(fixture.text(), expected);
    fixture.project.save().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    let reopened = Project::open(&fixture.root).unwrap();
    assert_eq!(reopened.event_draft("start").unwrap().1.after, "not ready");
}

#[test]
fn draft_preview_is_guarded_and_unchanged_keeps_parentheses_spaces() {
    let mut fixture = Fixture::new(SOURCE, "1.12");
    let compiled = fixture.compiled();
    let input = input(" ((ready)) ");
    let projection = fixture
        .project
        .condition_draft_from_compile(&compiled, &input)
        .unwrap();
    let same = fixture
        .project
        .preview_condition_draft(&compiled, &input, &projection.tree)
        .unwrap();
    assert_eq!(same.after_source, input.source);
    assert!(!same.changed);
    let replacement = Some(ConditionNode::Bool { value: false });
    let preview = fixture
        .project
        .preview_condition_draft(&compiled, &input, &replacement)
        .unwrap();
    assert_eq!(
        fixture
            .project
            .apply_condition_draft(&compiled, &input, &preview)
            .unwrap(),
        "false"
    );
    let mut stale = input.clone();
    stale.generation += 1;
    assert!(fixture
        .project
        .apply_condition_draft(&compiled, &stale, &preview)
        .is_err());
    let mut tampered = preview.clone();
    tampered.after_source = "true".into();
    assert!(fixture
        .project
        .apply_condition_draft(&compiled, &input, &tampered)
        .is_err());
    let source = fixture.text().replace("let score = 4", "let score = 5");
    fixture
        .project
        .set_text(&fixture.root.join("world.wl"), source)
        .unwrap();
    assert!(fixture
        .project
        .apply_condition_draft(&compiled, &input, &preview)
        .is_err());
}

#[test]
fn stale_baseline_generation_disk_and_unknown_capability_fail_atomically() {
    let mut fixture = Fixture::new(SOURCE, "1.12");
    let request = request(
        &fixture.project,
        ConditionSlot::EventAfter,
        ConditionNode::Bool { value: false },
    );
    let preview = fixture
        .project
        .preview_condition_authoring(&request)
        .unwrap();
    let mut stale = request.clone();
    stale.expected_generation += 1;
    assert!(fixture.project.preview_condition_authoring(&stale).is_err());
    assert!(fixture
        .project
        .apply_condition_authoring(&request, "tampered")
        .is_err());
    std::fs::write(
        fixture.root.join("world.wl"),
        SOURCE.replace("正文", "外改正文"),
    )
    .unwrap();
    assert!(fixture
        .project
        .apply_condition_authoring(&request, &preview.plan_digest)
        .is_err());
    assert_eq!(fixture.text(), SOURCE);
    fixture.project.refresh().unwrap();
    assert!(fixture
        .project
        .apply_condition_authoring(&request, &preview.plan_digest)
        .is_err());
    std::fs::write(
        fixture.root.join(".world/project.json"),
        r#"{"schema_version":1,"required_features":["unknown.future"]}"#,
    )
    .unwrap();
    fixture.project.refresh().unwrap();
    assert!(
        !fixture
            .project
            .condition_authoring(&selector(ConditionSlot::EventAfter))
            .unwrap()
            .writable
    );
}

#[test]
fn many_choice_slots_patch_once_preserves_all_other_bytes_and_noop() {
    fn source(display: &str, enable: &str, after: &str) -> String {
        let mut text = format!(
            "let ready = true\r\n/* 前置🙂注释 */\r\nevent start after {after} // 事件🙂\r\n"
        );
        for index in 0..48 {
            text.push_str(&format!("  choice once \"选择{index}🙂 if ready enable ready\" if {display} enable {enable} disabled \"仍需条件🙂\" #wl-localization:pick_{index} // 注释{index}\r\n    正文{index}：if ready enable ready。\r\n    -> END\r\n"));
        }
        text.push_str("\r\nevent other after true // 保留🙂\r\n  -> END\r\n// 文件尾🙂\r\n");
        text
    }
    let original = source("((ready))", "ready", "((ready))");
    let expected = source("not ready", "false", "not ready");
    let mut fixture = Fixture::new(&original, "1.12");
    let baseline = fixture.project.content_baseline();
    let (path, mut draft) = fixture.project.event_draft("start").unwrap();
    fixture
        .project
        .write_event(&path, Some("start"), &draft)
        .unwrap();
    assert_eq!(fixture.text(), original);
    assert_eq!(fixture.project.content_baseline(), baseline);
    let choices = draft.choices();
    assert_eq!(choices.len(), 48);
    for mut choice in choices {
        choice.condition = "not ready".into();
        choice.enable_condition = "false".into();
        draft.write_choice(Some(choice.line), &choice).unwrap();
    }
    draft.after = "not ready".into();
    fixture
        .project
        .write_event(&path, Some("start"), &draft)
        .unwrap();
    assert_eq!(fixture.text(), expected);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    fixture.project.save().unwrap();
    let mut reopened = Project::open(&fixture.root).unwrap();
    let (path, draft) = reopened.event_draft("start").unwrap();
    assert_eq!(draft.choices().len(), 48);
    let baseline = reopened.content_baseline();
    reopened.write_event(&path, Some("start"), &draft).unwrap();
    assert_eq!(reopened.document(&path).unwrap(), expected);
    assert_eq!(reopened.content_baseline(), baseline);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
}

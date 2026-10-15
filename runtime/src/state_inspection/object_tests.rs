use super::*;
use crate::{ReplayBudget, ReplayCancellation, Story, Value};
use serde_json::json;
use worldline_core::{compile_source_with_options, CompileOptions, TargetRef};

const SOURCE: &str = include_str!("../../tests/fixtures/object_state_inspection_v036/world.wl");

fn compile(source: &str) -> worldline_core::CompileResult {
    let result =
        compile_source_with_options("object-inspection.wl", source, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn query(kind: &str, id: &str) -> StateInspectionQuery {
    StateInspectionQuery {
        owner: Some(TargetRef::new(kind, id)),
        ..Default::default()
    }
}
fn item<'a>(page: &'a StateInspectionPage, id: &str) -> &'a StateInspectionItem {
    page.items.iter().find(|item| item.key.name == id).unwrap()
}
fn tags(values: &[&str]) -> Option<Value> {
    Some(Value::TagSet(
        values.iter().map(|value| (*value).into()).collect(),
    ))
}
fn runtime_state(story: &Story<'_>) -> serde_json::Value {
    json!({
        "save": story.save().unwrap(), "trace": story.replay_trace(),
        "rng": story.rng.get(), "visits": story.visits(), "turns": story.turns(),
        "choices": story.choice_presentations(), "history": story.state_history(),
        "actions": story.state_action_evidence(), "conditions": story.choice_evidence(),
        "stamp": story.inspection_stamp(), "view": story.state_view(),
        "variable_capture": story.action_capture.variables.captured,
    })
}

#[test]
fn object_inspection_uses_full_owner_identity_and_independent_same_name_states() {
    let compiled = compile(SOURCE);
    let story = Story::new_with_seed(&compiled.program, &compiled.analysis, 7).unwrap();
    for (kind, id, names) in [
        ("character", "shared", vec!["person_empty", "person_mood"]),
        ("character", "other", vec!["other_mood"]),
        ("world", "shared", vec!["world_mood"]),
    ] {
        let page = story.inspect_state(&query(kind, id)).unwrap();
        assert_eq!(
            page.owner.as_ref().unwrap().target,
            TargetRef::new(kind, id)
        );
        assert_eq!(page.owner.as_ref().unwrap().display, "同名");
        assert_eq!(page.total_items, names.len());
        assert_eq!(page.total_matches, names.len());
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.key.name.as_str())
                .collect::<Vec<_>>(),
            names
        );
        for row in &page.items {
            assert_eq!(row.key.group, InspectionGroup::State);
            assert_eq!(
                row.state_definition.as_ref().unwrap().owner,
                TargetRef::new(kind, id)
            );
            assert_eq!(row.state_definition.as_ref().unwrap().display, "状态");
            let source = row.source.as_ref().unwrap();
            let resolved =
                worldline_core::state_inspection_source::resolve_state_inspection_source(
                    &compiled, source,
                )
                .unwrap();
            assert!(compiled.sources[&resolved.path][resolved.range].starts_with("state "));
        }
    }
    let empty = story.inspect_state(&query("entity", "empty")).unwrap();
    assert_eq!(empty.total_items, 0);
    assert!(empty.owner.is_some() && empty.items.is_empty());
    assert_eq!(
        story
            .inspect_state(&query("character", "missing"))
            .unwrap_err()
            .code,
        "INSPECTION_OWNER_NOT_FOUND"
    );
    assert_eq!(
        story
            .inspect_state(&query("entity", "shared"))
            .unwrap_err()
            .code,
        "INSPECTION_OWNER_NOT_FOUND"
    );
}

#[test]
fn object_inspection_static_initial_is_not_first_and_dynamic_same_value_actions_remain_real() {
    let compiled = compile(SOURCE);
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, 7).unwrap();
    let q = query("character", "shared");
    let before = story.inspect_state(&q).unwrap();
    let mood = item(&before, "person_mood");
    assert_eq!(
        mood.state_definition.as_ref().unwrap().initial.value,
        tags(&["calm"])
    );
    assert_eq!(mood.first.status, InspectionCellStatus::Unrecorded);
    assert_eq!(mood.current.value, tags(&["alert"]));
    assert_eq!(before.first_observation, None);
    story.continue_story().unwrap();
    let first = story.inspect_state(&q).unwrap();
    assert_eq!(first.first_observation, Some(1));
    assert_eq!(first.previous_observation, None);
    assert_eq!(item(&first, "person_mood").first.value, tags(&["alert"]));
    assert_eq!(item(&first, "person_empty").current.value, tags(&[]));
    let unchanged = runtime_state(&story);
    for _ in 0..3 {
        assert_eq!(story.inspect_state(&q).unwrap(), first);
    }
    assert_eq!(runtime_state(&story), unchanged);
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let next = story.inspect_state(&q).unwrap();
    assert_eq!(next.previous_observation, Some(1));
    assert_eq!(next.current_observation, Some(2));
    assert_eq!(
        item(&next, "person_mood").previous_change,
        InspectionChange::Unchanged
    );
    assert_eq!(
        item(&next, "person_empty").previous_change,
        InspectionChange::Changed
    );
    assert_eq!(item(&next, "person_empty").current.value, tags(&["proved"]));
    assert_eq!(story.state_action_evidence().total_actions, 3);
    let same_write = &story.state_action_evidence().records[1];
    assert_eq!(same_write.before, same_write.after);
    let dynamic = &story.state_action_evidence().records[2];
    assert_eq!(dynamic.state, "person_empty");
    assert_eq!(dynamic.target, q.owner);
    let unchanged = runtime_state(&story);
    let changed = story
        .inspect_state(&StateInspectionQuery {
            changed_only: true,
            ..q
        })
        .unwrap();
    assert_eq!(changed.total_items, 2);
    assert_eq!(changed.items[0].key.name, "person_empty");
    assert_eq!(runtime_state(&story), unchanged);
}

#[test]
fn object_inspection_freezes_catalog_source_and_rejects_other_run_or_late_stamp() {
    let old = compile(SOURCE);
    let mut story = Story::new_with_seed(&old.program, &old.analysis, 9).unwrap();
    story.continue_story().unwrap();
    let old_page = story.inspect_state(&query("character", "shared")).unwrap();
    let mut q = query("character", "shared");
    q.expected_stamp = Some(old_page.stamp);
    let changed_source = SOURCE
        .replace(
            "state person_mood on character shared with calm",
            "state person_mood on character other with proved",
        )
        .replace(
            "character shared as \"同名\"",
            "character shared as \"新稿人物\"",
        );
    let current = compile(&changed_source);
    let newer = Story::new_with_seed(&current.program, &current.analysis, 9).unwrap();
    let new_page = newer.inspect_state(&query("character", "shared")).unwrap();
    assert_eq!(new_page.total_items, 1);
    assert_eq!(new_page.owner.unwrap().display, "新稿人物");
    assert_eq!(story.inspect_state(&q).unwrap(), old_page);
    assert_eq!(
        newer.inspect_state(&q).unwrap_err().code,
        "STALE_INSPECTION"
    );
    let same_program_run = Story::new_with_seed(&old.program, &old.analysis, 9).unwrap();
    assert_eq!(
        same_program_run.inspect_state(&q).unwrap_err().code,
        "STALE_INSPECTION"
    );
    story.choose(0).unwrap();
    assert_eq!(
        story.inspect_state(&q).unwrap_err().code,
        "STALE_INSPECTION"
    );
    q.owner = Some(TargetRef::new("character", "missing"));
    assert_eq!(
        story.inspect_state(&q).unwrap_err().code,
        "STALE_INSPECTION"
    );
    story.restart().unwrap();
    assert_eq!(
        story.inspect_state(&q).unwrap_err().code,
        "STALE_INSPECTION"
    );
}

#[test]
fn object_inspection_partial_cancel_budget_and_failed_state_preserve_real_previous() {
    let c = compile("world w\ntag a\ntag b\nstate s on world w with a\nlet divisor = 0\nevent start\n  choice \"继续\"\n    become s with b\n    {1 / divisor}\n");
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 7).unwrap();
    let q = query("world", "w");
    let token = ReplayCancellation::new();
    token.cancel();
    story
        .continue_story_bounded(ReplayBudget::new(100, 1000), &token)
        .unwrap();
    let cancelled = story.inspect_state(&q).unwrap();
    assert_eq!(cancelled.status, InspectionStatus::Cancelled);
    assert_eq!(cancelled.first_observation, None);
    assert_eq!(
        cancelled.items[0].first.status,
        InspectionCellStatus::Unrecorded
    );
    story
        .continue_story_bounded(ReplayBudget::new(100, 0), &ReplayCancellation::new())
        .unwrap();
    assert_eq!(
        story.inspect_state(&q).unwrap().status,
        InspectionStatus::TimeBudgetExceeded
    );
    story.continue_story().unwrap();
    story.choose(0).unwrap();
    story
        .continue_story_bounded(ReplayBudget::new(1, 1000), &ReplayCancellation::new())
        .unwrap();
    let partial = story.inspect_state(&q).unwrap();
    assert_eq!(partial.status, InspectionStatus::StepBudgetExceeded);
    assert_eq!(partial.current_observation, None);
    assert_eq!(partial.previous_observation, Some(1));
    assert_eq!(partial.items[0].current.value, tags(&["b"]));
    assert_eq!(partial.items[0].previous.value, tags(&["a"]));
    assert!(story.continue_story().is_err());
    let failed = story.inspect_state(&q).unwrap();
    assert_eq!(failed.status, InspectionStatus::Failed);
    assert_eq!(failed.current_observation, None);
    assert_eq!(failed.previous_observation, Some(1));
    let unchanged = runtime_state(&story);
    assert_eq!(story.inspect_state(&q).unwrap(), failed);
    assert_eq!(runtime_state(&story), unchanged);
}

#[test]
fn object_inspection_strict_owner_validation_and_legacy_wire_shape() {
    for owner in [
        json!({}),
        json!({"id":"w"}),
        json!({"kind":"world"}),
        json!({"kind":"world","id":1}),
        json!({"kind":"world","id":"w","extra":true}),
        json!([]),
    ] {
        assert!(serde_json::from_value::<StateInspectionQuery>(json!({"owner":owner})).is_err());
    }
    for q in [
        query("World", "w"),
        query("world", ""),
        query("world", &"a".repeat(16 * 1024)),
        StateInspectionQuery {
            group: Some(InspectionGroup::Global),
            ..query("world", "w")
        },
        StateInspectionQuery {
            group: Some(InspectionGroup::Local),
            ..query("world", "w")
        },
    ] {
        assert_eq!(q.validate().unwrap_err().code, "INVALID_INSPECTION_QUERY");
    }
    let c = compile(SOURCE);
    let story = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    let q: StateInspectionQuery = serde_json::from_value(json!({"owner":null})).unwrap();
    let legacy = story.inspect_state(&q).unwrap();
    let json = serde_json::to_value(&legacy).unwrap();
    assert!(json.get("owner").is_none());
    assert!(json["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row.get("state_definition").is_none()));
    assert_eq!(
        serde_json::from_value::<StateInspectionPage>(json).unwrap(),
        legacy
    );
    let page = story.inspect_state(&query("character", "shared")).unwrap();
    assert_eq!(
        serde_json::from_value::<StateInspectionPage>(serde_json::to_value(&page).unwrap())
            .unwrap(),
        page
    );
}

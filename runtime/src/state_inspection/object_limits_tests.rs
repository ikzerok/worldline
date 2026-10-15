use super::*;
use crate::{Story, Value};
use worldline_core::{compile_source_with_options, CompileOptions, TargetRef};

fn compile(source: &str) -> worldline_core::CompileResult {
    let result = compile_source_with_options("object-limits.wl", source, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
}
fn query() -> StateInspectionQuery {
    StateInspectionQuery {
        owner: Some(TargetRef::new("world", "w")),
        ..Default::default()
    }
}

#[test]
fn object_inspection_pages_all_owned_states_before_text_changes_and_offsets() {
    let mut source =
        "world w\ncharacter w\nstate outside on character w with []\nlet w_score = 0\n".to_string();
    for index in 0..107 {
        source.push_str(&format!(
            "state state_{index:03} on world w with [] as \"重复显示名\"\n"
        ));
    }
    source.push_str("event start\n  choice \"结束\"\n    -> END\n");
    let c = compile(&source);
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    story.continue_story().unwrap();
    let mut q = StateInspectionQuery {
        limit: 100,
        text: "重复".into(),
        ..query()
    };
    let first = story.inspect_state(&q).unwrap();
    assert_eq!(first.total_items, 107);
    assert_eq!(first.total_matches, 107);
    assert_eq!(first.incomparable_items, 107);
    assert_eq!(first.items.len(), 100);
    assert_eq!(first.next_offset, Some(100));
    assert_eq!(first.items[0].key.name, "state_000");
    q.expected_stamp = Some(first.stamp);
    q.offset = 100;
    let last = story.inspect_state(&q).unwrap();
    assert_eq!(last.stamp, first.stamp);
    assert_eq!(last.items.len(), 7);
    assert_eq!(last.items[0].key.name, "state_100");
    assert_eq!(last.next_offset, None);
    q.offset = usize::MAX;
    let out_of_range = story.inspect_state(&q).unwrap();
    assert_eq!(out_of_range.total_items, 107);
    assert_eq!(out_of_range.total_matches, 107);
    assert_eq!(out_of_range.offset, usize::MAX);
    assert!(out_of_range.items.is_empty());
    assert_eq!(out_of_range.next_offset, None);
    q.offset = 0;
    q.changed_only = true;
    let no_previous = story.inspect_state(&q).unwrap();
    assert_eq!(no_previous.total_items, 107);
    assert_eq!(no_previous.total_matches, 0);
    assert_eq!(no_previous.incomparable_items, 107);
    q.compare_to = InspectionBaseline::First;
    let unchanged = story.inspect_state(&q).unwrap();
    assert_eq!(unchanged.total_matches, 0);
    assert_eq!(unchanged.incomparable_items, 0);
    q.changed_only = false;
    q.text = "state_106".into();
    assert_eq!(story.inspect_state(&q).unwrap().total_matches, 1);
    q.text = "no_match".into();
    let filtered = story.inspect_state(&q).unwrap();
    assert_eq!(filtered.total_items, 107);
    assert_eq!(filtered.total_matches, 0);
}

#[test]
fn object_inspection_static_initial_and_real_history_use_existing_omission_budgets() {
    let tag = "a".repeat(MAX_INSPECTION_VALUE_BYTES + 1);
    let c = compile(&format!("world w\ntag {tag}\nstate s on world w with {tag}\nevent start\n  choice \"结束\"\n    -> END\n"));
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    story.continue_story().unwrap();
    let page = story.inspect_state(&query()).unwrap();
    let item = &page.items[0];
    let initial = &item.state_definition.as_ref().unwrap().initial;
    assert_eq!(initial.status, InspectionCellStatus::Omitted);
    assert!(initial.truncated && initial.value.is_none());
    assert_eq!(item.first.status, InspectionCellStatus::Omitted);
    assert_eq!(item.current.status, InspectionCellStatus::Omitted);
    assert_eq!(item.first_change, InspectionChange::NotComparable);
    assert!(page.history_omitted);
    let mut source = "world w\nstate s on world w with []\n".to_string();
    for index in 0..MAX_INSPECTION_ROWS {
        source.push_str(&format!("let global_{index:05} = 0\n"));
    }
    source.push_str("event start\n  choice \"结束\"\n    -> END\n");
    let c = compile(&source);
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    story.continue_story().unwrap();
    let page = story.inspect_state(&query()).unwrap();
    assert_eq!(page.total_items, 1);
    assert_eq!(page.items[0].first.status, InspectionCellStatus::Omitted);
    assert_eq!(page.items[0].current.value, Some(Value::TagSet(vec![])));
    assert_eq!(
        page.items[0]
            .state_definition
            .as_ref()
            .unwrap()
            .initial
            .value,
        Some(Value::TagSet(vec![]))
    );
    assert!(page.history_omitted);
}

#[test]
fn object_inspection_definition_metadata_counts_toward_total_page_output_budget() {
    let c = compile(&format!(
        "world w\nstate s on world w with [] as \"{}\"\nevent start\n  -> END\n",
        "x".repeat(1024 * 1024)
    ));
    let story = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    let save = story.save().unwrap();
    let trace = story.replay_trace();
    assert_eq!(
        story.inspect_state(&query()).unwrap_err().code,
        "INSPECTION_OUTPUT_LIMIT"
    );
    let q = StateInspectionQuery {
        text: "missing".into(),
        ..query()
    };
    assert_eq!(story.inspect_state(&q).unwrap().total_matches, 0);
    assert!(story
        .inspect_state(&StateInspectionQuery::default())
        .is_ok());
    assert_eq!(story.save().unwrap(), save);
    assert_eq!(story.replay_trace(), trace);
}

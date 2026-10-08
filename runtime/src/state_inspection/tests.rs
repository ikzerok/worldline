use super::*;
use crate::{ReplayBudget, ReplayCancellation, Story, Value};
use worldline_core::{compile_source_with_options, CompileOptions};
fn compile(text: &str) -> worldline_core::CompileResult {
    let c = compile_source_with_options("inspection.wl", text, CompileOptions::v1_13());
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    c
}
fn page(story: &Story<'_>) -> StateInspectionPage {
    story.inspect_state(&Default::default()).unwrap()
}
fn row<'a>(page: &'a StateInspectionPage, name: &str) -> &'a StateInspectionItem {
    page.items.iter().find(|row| row.key.name == name).unwrap()
}
#[test]
fn inspection_real_observations_never_use_construction_as_first() {
    let c = compile(
        "let n = 0\nevent start\n  set n = 1\n  choice \"继续\"\n    set n = 2\n    -> END\n",
    );
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    let p = page(&s);
    assert_eq!(p.first_observation, None);
    assert_eq!(row(&p, "n").first.status, InspectionCellStatus::Unrecorded);
    s.continue_story().unwrap();
    let p = page(&s);
    assert_eq!(p.current_observation, Some(1));
    assert_eq!(p.previous_observation, None);
    assert_eq!(row(&p, "n").first.value, Some(Value::Num(1.0)));
    let stamp = p.stamp;
    s.continue_story().unwrap();
    assert_eq!(page(&s).stamp, stamp);
    s.choose(0).unwrap();
    let p = page(&s);
    assert_eq!(p.current_observation, None);
    assert_eq!(p.previous_observation, Some(1));
    s.continue_story().unwrap();
    let p = page(&s);
    assert_eq!(p.current_observation, Some(2));
    assert_eq!(p.previous_observation, Some(1));
    assert_eq!(row(&p, "n").previous_change, InspectionChange::Changed);
}
#[test]
fn inspection_partial_budget_cancel_and_error_keep_last_real_observation() {
    let c = compile("let n = 0\nevent start\n  choice \"继续\"\n    set n = 2\n    {1 / n}\n    set n = 0\n    {1 / n}\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 7).unwrap();
    let token = ReplayCancellation::new();
    token.cancel();
    s.continue_story_bounded(ReplayBudget::new(100, 1000), &token)
        .unwrap();
    assert_eq!(page(&s).status, InspectionStatus::Cancelled);
    assert_eq!(page(&s).first_observation, None);
    s.continue_story().unwrap();
    s.choose(0).unwrap();
    s.continue_story_bounded(ReplayBudget::new(1, 1000), &ReplayCancellation::new())
        .unwrap();
    let p = page(&s);
    assert_eq!(p.status, InspectionStatus::StepBudgetExceeded);
    assert_eq!(p.previous_observation, Some(1));
    assert_eq!(p.current_observation, None);
    assert!(s.continue_story().is_err());
    let p = page(&s);
    assert_eq!(p.status, InspectionStatus::Failed);
    assert_eq!(p.previous_observation, Some(1));
}
#[test]
fn inspection_typed_values_initialization_and_empty_state_are_distinct() {
    let c=compile("world place\ntag calm\nstate mood on world place with []\nlet zero = 0\nlet no = false\nlet empty = \"\"\nlet tagvalue = tag(calm)\nlet tagsvalue = tags()\nlet statevalue = state(mood)\nevent start\n  choice \"init\"\n    let later = 0\n    -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    s.continue_story().unwrap();
    let p = page(&s);
    assert_eq!(
        row(&p, "later").current.status,
        InspectionCellStatus::Uninitialized
    );
    assert_eq!(row(&p, "zero").current.value, Some(Value::Num(0.0)));
    assert_eq!(row(&p, "no").current.value, Some(Value::Bool(false)));
    assert_eq!(
        row(&p, "empty").current.value,
        Some(Value::Str(String::new()))
    );
    assert_eq!(row(&p, "empty").current.display, "\"\"");
    assert_eq!(row(&p, "mood").current.value, Some(Value::TagSet(vec![])));
    assert_eq!(
        row(&p, "tagvalue").current.value,
        Some(Value::Tag("calm".into()))
    );
    assert_eq!(
        row(&p, "statevalue").current.value,
        Some(Value::StateRef("mood".into()))
    );
    s.choose(0).unwrap();
    s.continue_story().unwrap();
    assert_eq!(
        row(&page(&s), "later").previous_change,
        InspectionChange::Changed
    );
}
#[test]
fn inspection_local_invocations_never_alias_globals_or_each_other() {
    let c=compile("let n = 99\nfragment f(n: num)\n  choice \"继续\"\n    return\n  local later: num = 1\nevent start\n  call f(1)\n  call f(2)\n  -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    s.continue_story().unwrap();
    let p = page(&s);
    let local = p
        .items
        .iter()
        .find(|r| r.key.group == InspectionGroup::Local && r.key.name == "n")
        .unwrap();
    let first_call = local.key.call_id;
    assert_eq!(local.current.value, Some(Value::Num(1.0)));
    assert_eq!(
        row(&p, "later").current.status,
        InspectionCellStatus::Uninitialized
    );
    s.choose(0).unwrap();
    s.continue_story().unwrap();
    let p = page(&s);
    let local = p
        .items
        .iter()
        .find(|r| r.key.group == InspectionGroup::Local && r.key.name == "n")
        .unwrap();
    assert_ne!(local.key.call_id, first_call);
    assert_eq!(local.previous.status, InspectionCellStatus::NotInScope);
    assert_eq!(local.previous_change, InspectionChange::NotComparable);
    s.choose(0).unwrap();
    s.continue_story().unwrap();
    assert!(page(&s)
        .items
        .iter()
        .all(|r| r.key.group != InspectionGroup::Local));
}
#[test]
fn inspection_is_read_only_including_rng_trace_and_same_value_writes() {
    let c=compile("let n = 2\nevent start\n  choice \"随机 {rnd(1, 9)}\"\n    set n = 2\n    choice \"结束\"\n      -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 19).unwrap();
    s.continue_story().unwrap();
    s.choose(0).unwrap();
    s.continue_story().unwrap();
    let save = s.save().unwrap();
    let trace = s.replay_trace();
    let rng = s.rng.get();
    let stamp = s.inspection_stamp();
    for _ in 0..5 {
        let p = page(&s);
        assert_eq!(row(&p, "n").previous_change, InspectionChange::Unchanged);
    }
    assert_eq!(s.save().unwrap(), save);
    assert_eq!(s.replay_trace(), trace);
    assert_eq!(s.rng.get(), rng);
    assert_eq!(s.inspection_stamp(), stamp);
}
#[test]
fn inspection_filters_before_paging_and_rejects_stale_stamp() {
    let c=compile("let aa = 0\nlet bb = false\nlet cc = \"匹配值\"\nevent start\n  choice \"end\"\n    set aa = 1\n    -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    s.continue_story().unwrap();
    let mut q = StateInspectionQuery {
        limit: 1,
        ..Default::default()
    };
    let p = s.inspect_state(&q).unwrap();
    assert_eq!(p.total_matches, 3);
    assert_eq!(p.next_offset, Some(1));
    q.offset = 1;
    q.expected_stamp = Some(p.stamp);
    assert_eq!(s.inspect_state(&q).unwrap().items[0].key.name, "bb");
    q.text = "匹配".into();
    q.offset = 0;
    assert_eq!(s.inspect_state(&q).unwrap().total_matches, 1);
    s.choose(0).unwrap();
    assert_eq!(s.inspect_state(&q).unwrap_err().code, "STALE_INSPECTION");
    s.continue_story().unwrap();
    let q = StateInspectionQuery {
        changed_only: true,
        ..Default::default()
    };
    let p = s.inspect_state(&q).unwrap();
    assert_eq!(p.total_matches, 1);
    assert_eq!(p.items[0].key.name, "aa");
}
#[test]
fn inspection_long_values_are_omitted_not_falsely_equal() {
    let c = compile(&format!(
        "let long = \"{}\"\nevent start\n  choice \"继续\"\n    -> END\n",
        "甲".repeat(2000)
    ));
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    s.continue_story().unwrap();
    let p = page(&s);
    let r = row(&p, "long");
    assert_eq!(r.current.status, InspectionCellStatus::Omitted);
    assert!(r.current.truncated);
    assert!(r.current.display.len() < 2048);
    assert!(p.history_omitted);
    assert_eq!(r.first_change, InspectionChange::NotComparable);
}
#[test]
fn inspection_checkpoint_restart_and_new_trace_clear_binding() {
    let c = compile("let n = 0\nevent start\n  choice \"end\"\n    -> END\n");
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    s.continue_story().unwrap();
    let stamp = s.inspection_stamp();
    let loaded = Story::from_checkpoint(&c.program, &c.analysis, &s.checkpoint().unwrap()).unwrap();
    assert_ne!(loaded.inspection_stamp().run_id, stamp.run_id);
    assert_eq!(page(&loaded).previous_observation, None);
    s.start_trace_from_here().unwrap();
    assert_ne!(
        s.inspection_stamp().trace_generation,
        stamp.trace_generation
    );
    assert_eq!(page(&s).previous_observation, None);
    s.restart().unwrap();
    assert_ne!(s.inspection_stamp().run_id, stamp.run_id);
    assert_eq!(page(&s).first_observation, None);
}

#[test]
fn inspection_history_budget_missing_rows_are_not_uninitialized() {
    let mut source = String::new();
    for i in 0..MAX_INSPECTION_ROWS + 2 {
        source.push_str(&format!("let value_{i:05} = {i}\n"));
    }
    source.push_str("event start\n  choice \"结束\"\n    -> END\n");
    let c = compile(&source);
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    s.continue_story().unwrap();
    let query = StateInspectionQuery {
        text: format!("value_{:05}", MAX_INSPECTION_ROWS + 1),
        ..Default::default()
    };
    let p = s.inspect_state(&query).unwrap();
    assert_eq!(p.total_matches, 1);
    assert_eq!(p.items[0].current.status, InspectionCellStatus::Present);
    assert_eq!(p.items[0].first.status, InspectionCellStatus::Omitted);
    assert_eq!(p.items[0].first_change, InspectionChange::NotComparable);
    assert!(p.history_omitted);
    assert!(s.inspection.latest.as_ref().unwrap().values.len() <= MAX_INSPECTION_ROWS);
}

#[test]
fn inspection_query_finds_full_current_value_beyond_display_and_keeps_offset() {
    let c = compile(&format!(
        "let value = \"{}FinalNeedle\"\nevent start\n  choice \"结束\"\n    -> END\n",
        "甲".repeat(2000)
    ));
    let mut s = Story::new_with_seed(&c.program, &c.analysis, 1).unwrap();
    s.continue_story().unwrap();
    let q = StateInspectionQuery {
        text: "finalneedle".into(),
        ..Default::default()
    };
    let p = s.inspect_state(&q).unwrap();
    assert_eq!(p.total_matches, 1);
    assert!(!p.items[0].current.display.contains("FinalNeedle"));
    assert_eq!(p.items[0].current.status, InspectionCellStatus::Omitted);
    let q = StateInspectionQuery {
        offset: usize::MAX,
        ..q
    };
    let p = s.inspect_state(&q).unwrap();
    assert!(p.items.is_empty());
    assert_eq!(p.offset, usize::MAX);
    assert_eq!(p.next_offset, None);
    assert_eq!(p.total_matches, 1);
}

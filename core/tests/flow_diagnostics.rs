//! 静态流程反馈与调用抽取等价；不依赖运行预算或展示图的可能边。
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};

fn compile(source: &str) -> CompileResult {
    let compiled = compile_source_with_options("flow.wl", source, CompileOptions::v1_13());
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled
}

fn count(compiled: &CompileResult, code: &str) -> usize {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == code)
        .count()
}

#[test]
fn unconditional_cycles_include_all_members_and_source_locations() {
    for length in [1, 2, 3, 64] {
        let mut source = String::new();
        for index in 0..length {
            source.push_str(&format!("event e{index}\n  -> e{}\n", (index + 1) % length));
        }
        let compiled = compile(&source);
        let cycles: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.code == "A206")
            .collect();
        assert_eq!(cycles.len(), 1, "length={length}");
        assert_eq!(cycles[0].file, "flow.wl");
        assert_eq!(cycles[0].span.line, 2);
        assert_eq!(cycles[0].related.len(), length - 1);
        for index in 0..length {
            assert!(cycles[0].message.contains(&format!("`e{index}`")));
        }
    }
}

#[test]
fn output_and_effects_are_not_choice_pauses() {
    let compiled = compile(
        "character witness\nevent start\n  effect on enter\n    meet witness\n  正文\n  -> next\nevent next\n  -> start\n",
    );
    assert_eq!(count(&compiled, "A206"), 1);
}

#[test]
fn conditional_finite_loops_and_choice_pauses_are_not_definite_cycles() {
    for source in [
        "let n = 0\nevent start\n  set n = n + 1\n  if n < 3\n    -> next\n  -> END\nevent next\n  -> start\n",
        "event start\n  choice \"再来\"\n    -> next\n  choice \"离开\"\n    -> END\nevent next\n  -> start\n",
        "event start\n  choice \"继续\"\n    -> start\n",
        "event start\n  choice once \"继续\"\n    -> start\n  -> END\n",
        "let gate = true\nevent start after gate\n  set gate = false\n  -> start\n",
    ] {
        let compiled = compile(source);
        assert_eq!(count(&compiled, "A206"), 0, "{source}");
    }
}

#[test]
fn earlier_exit_or_pause_prevents_old_top_level_self_loop_false_positive() {
    for source in [
        "event start\n  -> END\n  -> start\n",
        "event start\n  choice \"继续\"\n    正文\n  -> start\n",
        "let n = 0\nevent start\n  set n = n + 1\n  if n > 2\n    -> END\n  -> start\n",
    ] {
        assert_eq!(count(&compile(source), "A206"), 0, "{source}");
    }
}

#[test]
fn all_conditional_branches_inside_a_closed_component_are_detected() {
    let compiled = compile(
        "let gate = true\nevent start\n  if gate\n    -> left\n  else\n    -> right\nevent left\n  -> start\nevent right\n  -> start\n",
    );
    assert_eq!(count(&compiled, "A206"), 1);
}

#[test]
fn unknown_exit_path_prevents_a_definite_cycle_warning() {
    let compiled = compile(
        "let gate = true\nevent start\n  if gate\n    -> next\n  else\n    -> end\nevent next\n  -> start\nevent end\n  -> END\n",
    );
    assert_eq!(count(&compiled, "A206"), 0);
}

#[test]
fn dated_entry_and_reachable_target_are_checked_but_history_is_not() {
    let entry = compile("period night\nevent start during night\n  缺少结尾\n");
    assert_eq!(count(&entry, "A202"), 0);
    let execution =
        worldline_core::analysis::execution_diagnostics(&entry.program, &entry.analysis);
    assert_eq!(execution.len(), 1);
    assert_eq!(execution[0].code, "A202");
    assert_eq!(execution[0].severity, worldline_core::Severity::Hint);
    assert!(execution[0].message.contains("作为历史资料无需补跃迁"));
    let compiled = compile(
        "period night\nevent start during night\n  -> played\nevent played during night\n  缺少结尾\nevent archive during night follows played\n  历史资料\n",
    );
    let tails: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == "A202")
        .collect();
    assert_eq!(tails.len(), 1);
    assert!(tails[0].message.contains("`played`"));
    assert_eq!(tails[0].span.line, 4);
    assert_eq!(tails[0].severity, worldline_core::Severity::Hint);
    assert!(
        worldline_core::analysis::execution_diagnostics(&compiled.program, &compiled.analysis)
            .is_empty()
    );
}

#[test]
fn unreachable_suffix_does_not_activate_dated_history() {
    let compiled = compile(
        "period night\nevent start\n  -> END\n  -> archive\nevent archive during night\n  历史资料\n",
    );
    assert_eq!(count(&compiled, "A202"), 0);
}

#[test]
fn fragments_that_divert_or_end_do_not_create_tail_warnings() {
    for exit in ["-> reunion", "-> END"] {
        let compiled = compile(&format!(
            "fragment reveal()\n  正文\n  {exit}\nevent start\n  call reveal()\nevent reunion\n  -> END\n"
        ));
        assert_eq!(count(&compiled, "A202"), 0, "{exit}");
    }
}

#[test]
fn fragment_explicit_and_implicit_returns_preserve_real_caller_tail_warning() {
    for body in ["return", "正文"] {
        let compiled = compile(&format!(
            "fragment reveal()\n  {body}\nevent start\n  call reveal()\n"
        ));
        assert_eq!(count(&compiled, "A202"), 1, "{body}");
    }
}

#[test]
fn mixed_fragment_return_and_transfer_remains_distinct() {
    let source = "let gate = true\nfragment reveal()\n  if gate\n    return\n  else\n    -> reunion\nevent start\n  call reveal()\nevent reunion\n  -> END\n";
    assert_eq!(count(&compile(source), "A202"), 1);
    let complete = source.replace("  call reveal()\n", "  call reveal()\n  -> END\n");
    assert_eq!(count(&compile(&complete), "A202"), 0);
}

#[test]
fn nested_calls_propagate_transfer_but_return_only_one_call_level() {
    let compiled = compile(
        "fragment inner()\n  return\nfragment outer()\n  call inner()\n  -> reunion\nevent start\n  call outer()\nevent reunion\n  -> END\n",
    );
    assert_eq!(count(&compiled, "A202"), 0);
    let returns = compile(
        "fragment inner()\n  return\nfragment outer()\n  call inner()\n  return\nevent start\n  call outer()\n",
    );
    assert_eq!(count(&returns, "A202"), 1);
}

#[test]
fn conditional_fragment_return_skips_unreachable_transfer() {
    let compiled = compile(
        "fragment reveal()\n  if true\n    return\n  -> reunion\nevent start\n  call reveal()\nevent reunion\n  -> END\n",
    );
    assert_eq!(count(&compiled, "A202"), 1);
}

#[test]
fn fragment_transfer_participates_in_reachability_and_cycle_proof() {
    let compiled = compile(
        "period night\nfragment reveal()\n  -> archive\nevent start\n  call reveal()\nevent archive during night\n  缺少结尾\n",
    );
    assert_eq!(count(&compiled, "A202"), 1);
    let cycle = compile(
        "fragment reveal()\n  -> next\nevent start\n  call reveal()\nevent next\n  -> start\n",
    );
    assert_eq!(count(&cycle, "A206"), 1);
    let diagnostic = cycle.diagnostics.iter().find(|d| d.code == "A206").unwrap();
    assert_eq!(diagnostic.span.line, 2);
}

#[test]
fn fragment_choice_pause_prevents_closed_cycle_claim() {
    let compiled = compile(
        "fragment reveal()\n  choice \"继续\"\n    return\nevent start\n  call reveal()\n  -> start\n",
    );
    assert_eq!(count(&compiled, "A206"), 0);
    assert_eq!(count(&compiled, "A202"), 0);
}

#[test]
fn once_and_hidden_or_disabled_groups_can_fall_through() {
    for clause in [
        "once \"一次\"",
        "\"隐藏\" if false",
        "\"禁用\" enable false disabled \"锁定\"",
    ] {
        let compiled = compile(&format!("event start\n  choice {clause}\n    -> END\n"));
        assert_eq!(count(&compiled, "A202"), 1, "{clause}");
    }
    let compiled = compile("event start\n  choice \"可用\"\n    -> END\n");
    assert_eq!(count(&compiled, "A202"), 0);
}

#[test]
fn scene_entry_includes_parent_continuations() {
    let compiled = compile(
        "event start\n  -> later.inner\nevent later\n  scene inner\n    正文\n  -> start\n",
    );
    assert_eq!(count(&compiled, "A206"), 1);
    assert_eq!(count(&compiled, "A202"), 0);
    let exit =
        compile("event start\n  -> later.inner\nevent later\n  scene inner\n    正文\n  -> END\n");
    assert_eq!(count(&exit, "A206"), 0);
    assert_eq!(count(&exit, "A202"), 0);
}

#[test]
fn nested_scene_continuations_and_dated_entry_tail_are_preserved() {
    let compiled = compile(
        "event start\n  -> later.outer.inner\nevent later\n  scene outer\n    scene inner\n      正文\n    返回外层\n  -> END\n",
    );
    assert_eq!(count(&compiled, "A202"), 0);
    let tail = compile(
        "period night\nevent start\n  -> later.inner\nevent later during night\n  -> END\n  scene inner\n    实际跳入后缺尾\n",
    );
    assert_eq!(count(&tail, "A202"), 1);
}

#[test]
fn current_event_relative_scene_resolution_matches_execution() {
    let compiled =
        compile("event start\n  scene next\n    -> start\n  -> END\nevent next\n  -> END\n");
    assert_eq!(count(&compiled, "A206"), 1);
    let relative =
        compile("event start\n  -> next\n  scene next\n    -> start\nevent next\n  -> END\n");
    assert_eq!(count(&relative, "A206"), 1);
}

#[test]
fn diagnostics_do_not_modify_ast_or_runtime_fingerprint() {
    let mut compiled = compile("event start\n  -> next\nevent next\n  -> start\n");
    let fingerprint = compiled.analysis.fingerprint;
    let (again, diagnostics) = worldline_core::analysis::analyze(&compiled.program, Vec::new());
    assert_eq!(again.fingerprint, fingerprint);
    assert_eq!(diagnostics.iter().filter(|d| d.code == "A206").count(), 1);
    compiled.program.events[0].period = Some("historic".into());
    assert_eq!(
        worldline_core::analysis::fingerprint_program(&compiled.program),
        fingerprint
    );
}

#[test]
fn cycle_feedback_is_available_without_a_language_upgrade() {
    for language in worldline_core::LanguageVersion::SUPPORTED {
        let compiled = compile_source_with_options(
            "flow.wl",
            "event start\n  -> next\nevent next\n  -> start\n",
            CompileOptions::new(language),
        );
        assert!(!compiled.has_errors());
        assert_eq!(compiled.program.language_version, language);
        assert_eq!(count(&compiled, "A206"), 1);
    }
}

#[test]
fn permanently_hidden_or_disabled_choices_do_not_invent_a_pause() {
    for clause in [
        "\"隐藏\" if false",
        "\"禁用\" enable false disabled \"锁定\"",
    ] {
        let compiled = compile(&format!(
            "event start\n  choice {clause}\n    -> END\n  -> start\n"
        ));
        assert_eq!(count(&compiled, "A206"), 1, "{clause}");
    }
}

#[test]
fn recursive_or_missing_fragment_cannot_prove_a_closed_cycle() {
    for source in [
        "fragment recurse()\n  call recurse()\nevent start\n  call recurse()\n  -> start\n",
        "event start\n  call missing()\n  -> start\n",
    ] {
        let compiled = compile_source_with_options("flow.wl", source, CompileOptions::v1_13());
        assert!(compiled.has_errors());
        assert_eq!(count(&compiled, "A206"), 0);
    }
}

#[test]
fn separate_closed_components_get_one_diagnostic_each() {
    let compiled =
        compile("event start\n  -> next\nevent next\n  -> start\nevent isolated\n  -> isolated\n");
    assert_eq!(count(&compiled, "A206"), 2);
}

#[test]
fn pure_historical_records_are_silent_until_explicit_execution() {
    let compiled = compile(
        "period night\nevent a during night\n  港口记录\nevent b during night\n  灯塔记录\nevent c during night follows a\n  次日记录\n",
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let fingerprint = compiled.analysis.fingerprint;
    let additional =
        worldline_core::analysis::execution_diagnostics(&compiled.program, &compiled.analysis);
    assert_eq!(additional.len(), 1);
    assert!(additional[0].message.contains("`a`"));
    assert_eq!(additional[0].severity, worldline_core::Severity::Hint);
    assert_eq!(compiled.analysis.fingerprint, fingerprint);
    assert!(compiled.diagnostics.is_empty());
}

#[test]
fn execution_hints_do_not_repeat_existing_undated_warnings_or_authored_dated_hints() {
    for source in [
        "event start\n  正文\n",
        "period night\nfragment explain()\n  return\nevent start during night\n  call explain()\n",
    ] {
        let compiled = compile(source);
        assert_eq!(count(&compiled, "A202"), 1);
        assert!(worldline_core::analysis::execution_diagnostics(
            &compiled.program,
            &compiled.analysis
        )
        .is_empty());
    }
}

#[test]
fn potential_runtime_errors_are_not_claimed_as_mandatory_cycles() {
    for source in [
        "event start\n  let n = 1 / 0\n  -> start\n",
        "event start\n  {later}\n  -> start\nevent declarations\n  let later = 1\n  -> END\n",
        "event start\n  set later = 1\n  -> start\nevent declarations\n  let later = 0\n  -> END\n",
        "let lower = 3\nlet upper = 1\nevent start\n  {rnd(lower, upper)}\n  -> start\n",
        "event start\n  if 1 / 0 > 2\n    -> next\n  else\n    -> start\nevent next\n  -> start\n",
        "fragment work(x: num)\n  return\nevent start\n  call work(1 / 0)\n  -> start\n",
    ] {
        assert_eq!(count(&compile(source), "A206"), 0, "{source}");
    }
}

#[test]
fn safe_initialized_assignments_and_fragment_parameters_still_allow_cycle_proof() {
    for source in [
        "event start\n  let n = 0\n  set n = n + 1\n  -> start\n",
        "fragment explain(n: num)\n  {n}\n  return\nevent start\n  call explain(1)\n  -> start\n",
    ] {
        assert_eq!(count(&compile(source), "A206"), 1, "{source}");
    }
}

#[test]
fn fragment_local_shadow_does_not_inherit_global_initialization_proof() {
    let compiled = compile(
        "let n = 1\nfragment explain()\n  {n}\n  local n: num = 2\n  return\nevent start\n  call explain()\n  -> start\n",
    );
    assert_eq!(count(&compiled, "A206"), 0);
}

#[test]
fn fragment_reuse_preserves_qualified_scene_identity() {
    let compiled = compile(
        "fragment onward()\n  -> start.next\nevent start\n  call onward()\n  scene next\n    -> start\nevent other\n  call onward()\nevent next\n  -> END\n",
    );
    assert_eq!(count(&compiled, "A206"), 1);
    assert_eq!(count(&compiled, "A202"), 0);
    let cycle = compiled
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "A206")
        .unwrap();
    assert!(cycle.message.contains("`start.next`"));
    assert!(!cycle.message.contains("`other`"));
}

#[test]
fn fragment_cycle_location_keeps_its_definition_file() {
    use std::{collections::BTreeMap, path::PathBuf};
    let entry = PathBuf::from("flow-summary-entry.wl");
    let fragments = PathBuf::from("flow-summary-fragments.wl");
    let sources = BTreeMap::from([
        (entry.clone(), "include \"flow-summary-fragments.wl\"\nevent start\n  call onward()\nevent next\n  -> start\n".into()),
        (fragments.clone(), "fragment onward()\n  -> next\n".into()),
    ]);
    let compiled =
        worldline_core::compile_sources_with_options(&entry, &sources, CompileOptions::v1_13());
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let cycle = compiled
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "A206")
        .unwrap();
    assert!(cycle.file.ends_with(fragments.to_str().unwrap()));
    assert_eq!(cycle.span.line, 2);
    assert!(cycle.related[0].0.ends_with(entry.to_str().unwrap()));
}

#[test]
fn fragment_depth_limit_is_a_possible_failure_not_a_proven_cycle() {
    for depth in [2, 128, 129] {
        let mut source = String::new();
        for index in 0..depth {
            source.push_str(&format!("fragment f{index}()\n"));
            if index + 1 == depth {
                source.push_str("  return\n");
            } else {
                source.push_str(&format!("  call f{}()\n", index + 1));
            }
        }
        source.push_str("event start\n  call f0()\n  -> start\n");
        let compiled = compile(&source);
        assert_eq!(
            count(&compiled, "A206"),
            usize::from(depth <= 128),
            "depth={depth}"
        );
    }
}

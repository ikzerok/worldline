//! 静态使用处来自 AST；不改变旧目录/运行证据。
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use worldline_core::{
    compile_source_with_options, compile_sources_with_options, CompileOptions, CompileResult,
    ExecutableContextRole as Role, RelationQueryDirection, TargetRef, WorldContextKind as Kind,
    WorldContextLimit as Limit, WorldContextOptions, WorldContextProvenance, WorldContextResult,
};
const SOURCE: &str = r#"let score = 3
let shadow = 9
rule ready() -> bool = score > 0
rule nested() -> bool = ready() and ready()
rule parameter(score: num) -> num = score
fragment leaf(value: num)
  set score = score + value
  return
fragment receipt(shadow: num)
  local score: num = shadow
  local scratch: num = score
  call leaf(score)
  return
event start after nested()
  中文 {score} 与 {score}
  choice "读取{score}" if ready() enable nested() disabled "稍后"
    call leaf(score)
    -> END
"#;
fn compile(source: &str) -> CompileResult {
    compile_source_with_options("world.wl", source, CompileOptions::v1_13())
}
fn query(result: &CompileResult, kind: &str, id: &str) -> WorldContextResult {
    result
        .query_world_context(
            &TargetRef::new(kind, id),
            WorldContextOptions {
                include_executable: true,
                ..Default::default()
            },
        )
        .unwrap()
}
fn context(record: &worldline_core::WorldContextRecord) -> Role {
    match record.provenance {
        WorldContextProvenance::Executable { context, .. } => context,
        _ => panic!("非执行记录"),
    }
}
#[test]
fn opt_in_preserves_prior_defaults_and_every_repeated_use_site() {
    let compiled = compile(SOURCE);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let catalog_before = serde_json::to_value(&compiled.analysis.catalog).unwrap();
    let old = compiled
        .query_world_context(&TargetRef::new("rule", "ready"), Default::default())
        .unwrap();
    assert_eq!(old.returned, 0);
    let result = query(&compiled, "rule", "ready");
    assert!(result.complete, "{:?}", result.reasons);
    assert_eq!(
        result
            .records
            .iter()
            .filter(|row| row.kind == Kind::RuleCall)
            .count(),
        3
    );
    assert_eq!(
        result
            .records
            .iter()
            .filter(|row| row.kind == Kind::GlobalRead)
            .count(),
        1
    );
    assert_eq!(
        result
            .records
            .iter()
            .map(|row| &row.id)
            .collect::<BTreeSet<_>>()
            .len(),
        result.returned
    );
    let repeated: Vec<_> = result
        .records
        .iter()
        .filter(|row| row.from_ref == TargetRef::new("rule", "nested"))
        .collect();
    assert_eq!(repeated.len(), 2);
    assert_ne!(repeated[0].source.column, repeated[1].source.column);
    assert_eq!(context(repeated[0]), Role::RuleBody);
    assert_eq!(
        catalog_before,
        serde_json::to_value(&compiled.analysis.catalog).unwrap()
    );
    let explicit = compiled
        .query_world_context(
            &TargetRef::new("rule", "ready"),
            WorldContextOptions {
                kinds: vec![Kind::RuleCall],
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(explicit.returned, 3);
    assert!(explicit
        .records
        .iter()
        .all(|row| row.kind == Kind::RuleCall));
}
#[test]
fn parameters_locals_and_identity_arguments_are_never_global_reads_or_writes() {
    let compiled = compile(SOURCE);
    for owner in [
        TargetRef::new("rule", "parameter"),
        TargetRef::new("fragment", "receipt"),
    ] {
        let result = compiled
            .query_world_context(
                &owner,
                WorldContextOptions {
                    include_executable: true,
                    direction: RelationQueryDirection::Outgoing,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(
            result
                .records
                .iter()
                .all(|row| !matches!(row.kind, Kind::GlobalRead | Kind::GlobalWrite)),
            "{:?}",
            result.records
        );
    }
    let invalid_shadow_write = compile(&SOURCE.replace(
        "  local scratch: num = score",
        "  local scratch: num = score\n  set score = score + 1",
    ));
    assert!(invalid_shadow_write.has_errors());
    let shadow = invalid_shadow_write
        .query_world_context(
            &TargetRef::new("fragment", "receipt"),
            WorldContextOptions {
                include_executable: true,
                direction: RelationQueryDirection::Outgoing,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(shadow
        .records
        .iter()
        .all(|row| !matches!(row.kind, Kind::GlobalRead | Kind::GlobalWrite)));
    assert!(!shadow.complete);
    let identities = compile("let place = 1\nlet state_id = 2\nlet tag_id = 3\ntag tag_id\ncharacter c\nstate state_id on character c with tag_id\nevent place\n  choice \"是\" if has(state_id, tag_id) and visits(place) == 0\n    -> END\n");
    assert!(!identities.has_errors(), "{:?}", identities.diagnostics);
    for id in ["place", "state_id", "tag_id"] {
        let result = query(&identities, "variable", id);
        assert!(result
            .records
            .iter()
            .all(|row| row.kind != Kind::GlobalRead));
    }
}
#[test]
fn typed_roles_distinguish_requirements_conditions_rhs_and_actual_statement_owner() {
    let compiled = compile(SOURCE);
    let result = query(&compiled, "event", "start");
    for role in [
        Role::EventRequirement,
        Role::TextInterpolation,
        Role::ChoiceLabel,
        Role::ChoiceCondition,
        Role::ChoiceEnable,
        Role::FragmentArgument,
    ] {
        assert!(
            result.records.iter().any(|row| context(row) == role),
            "{role:?}"
        );
    }
    let score = query(&compiled, "variable", "score");
    assert!(score
        .records
        .iter()
        .any(|row| row.kind == Kind::GlobalWrite && context(row) == Role::GlobalInitializer));
    assert!(score
        .records
        .iter()
        .any(|row| row.kind == Kind::GlobalWrite && context(row) == Role::AssignmentTarget));
    assert!(score
        .records
        .iter()
        .any(|row| row.kind == Kind::GlobalRead && context(row) == Role::AssignmentValue));
    let leaf = query(&compiled, "fragment", "leaf");
    assert_eq!(
        leaf.records
            .iter()
            .filter(|row| row.kind == Kind::FragmentCall)
            .count(),
        2
    );
}
#[test]
fn two_hops_are_bounded_static_occurrences_and_all_query_limits_are_truthful() {
    let compiled = compile(SOURCE);
    let target = TargetRef::new("event", "start");
    let two = compiled
        .query_world_context(
            &target,
            WorldContextOptions {
                include_executable: true,
                depth: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(two.complete);
    assert!(two
        .records
        .iter()
        .any(|row| row.from_ref == TargetRef::new("rule", "nested")));
    assert_eq!(
        two.records
            .iter()
            .map(|row| &row.id)
            .collect::<BTreeSet<_>>()
            .len(),
        two.returned
    );
    for (options, reason) in [
        (
            WorldContextOptions {
                max_records: 1,
                ..Default::default()
            },
            Limit::RecordLimit,
        ),
        (
            WorldContextOptions {
                max_candidates: 1,
                ..Default::default()
            },
            Limit::CandidateBudget,
        ),
        (
            WorldContextOptions {
                max_nodes: 1,
                ..Default::default()
            },
            Limit::NodeLimit,
        ),
    ] {
        let result = compiled
            .query_world_context(
                &target,
                WorldContextOptions {
                    include_executable: true,
                    ..options
                },
            )
            .unwrap();
        assert!(!result.complete && result.truncated);
        assert!(result.reasons.contains(&reason));
    }
    assert!(compiled
        .query_world_context_cancellable(
            &target,
            WorldContextOptions {
                include_executable: true,
                ..Default::default()
            },
            || true
        )
        .is_err());
}
#[test]
fn physical_include_file_unicode_columns_and_scene_identity_are_preserved() {
    let root = std::env::temp_dir().join(format!("executable-context-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let root = worldline_core::project::Project::new(&root).root;
    let entry = root.join("world.wl");
    let sources = BTreeMap::from([
        (
            entry.clone(),
            "let score = 1\nevent start\ninclude \"body.wl\"\n".into(),
        ),
        (
            root.join("body.wl"),
            "  scene inner\n    中文 {score} 和 {score}\n    set score = score + 1\n    -> END\n"
                .into(),
        ),
    ]);
    let compiled = compile_sources_with_options(&entry, &sources, CompileOptions::v1_13());
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let result = query(&compiled, "scene", "start.inner");
    assert!(result.complete, "{:?}", result.reasons);
    assert_eq!(result.returned, 4);
    assert!(result
        .records
        .iter()
        .all(|row| PathBuf::from(&row.source.file) == root.join("body.wl")));
    let text: Vec<_> = result
        .records
        .iter()
        .filter(|row| row.source.line == 2)
        .collect();
    assert_eq!(
        text.iter()
            .map(|row| row.source.column.unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([9, 19])
    );
}
#[test]
fn missing_or_invalid_sources_never_claim_complete_and_stale_snapshot_is_rejected() {
    let mut compiled = compile(SOURCE);
    let original = query(&compiled, "rule", "ready");
    compiled.sources.clear();
    let missing = query(&compiled, "rule", "ready");
    assert!(!missing.complete && !missing.truncated);
    assert_eq!(missing.total, None);
    assert!(missing.records.is_empty());
    assert!(missing.reasons.contains(&Limit::SourceUnavailable));
    assert!(compiled
        .query_world_context(
            &original.target,
            WorldContextOptions {
                include_executable: true,
                expected_snapshot: Some(original.snapshot),
                ..Default::default()
            }
        )
        .is_err());
    let invalid = compile(&SOURCE.replace("score > 0", "missing > 0"));
    let partial = query(&invalid, "rule", "ready");
    assert!(!partial.complete);
    assert!(partial.reasons.contains(&Limit::InvalidSource));
}

#[test]
fn effects_branch_conditions_and_local_initializers_keep_their_typed_roles() {
    let source = "let score = 1\nrule ready() -> bool = score > 0\ncharacter c\nfragment check()\n  local gate: bool = ready()\n  return\nevent start\n  effect on enter if ready()\n    meet c\n  if ready()\n    set score = score + 1\n  else if ready()\n    call check()\n  -> END\n";
    let compiled = compile(source);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let result = query(&compiled, "rule", "ready");
    assert!(result.complete, "{:?}", result.reasons);
    for (role, count) in [
        (Role::EffectCondition, 1),
        (Role::BranchCondition, 2),
        (Role::LocalInitializer, 1),
    ] {
        assert_eq!(
            result
                .records
                .iter()
                .filter(|row| row.kind == Kind::RuleCall && context(row) == role)
                .count(),
            count
        );
    }
    assert!(result
        .records
        .iter()
        .any(|row| row.source.line == 12 && context(row) == Role::BranchCondition));
}

#[test]
fn ambiguous_included_statement_ownership_is_unavailable_instead_of_a_guessed_file() {
    let root = std::env::temp_dir().join(format!("executable-ambiguous-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let root = worldline_core::project::Project::new(&root).root;
    let entry = root.join("world.wl");
    let sources = BTreeMap::from([
        (
            entry.clone(),
            "let score = 1\nevent start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n".into(),
        ),
        (root.join("a.wl"), "  {score}\n".into()),
        (root.join("b.wl"), "  {score}\n".into()),
    ]);
    let compiled = compile_sources_with_options(&entry, &sources, CompileOptions::v1_13());
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let result = query(&compiled, "event", "start");
    assert!(!result.complete);
    assert!(!result.truncated);
    assert_eq!(result.total, None);
    assert!(result.records.is_empty());
    assert!(result.reasons.contains(&Limit::SourceUnavailable));
}

#[test]
fn explicit_global_declaration_stays_a_write_even_when_a_local_shadows_reads() {
    let compiled = compile("fragment make()\n  local score: num = 5\n  let score = 1\n  {score}\n  return\nevent start\n  call make()\n  -> END\n");
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let result = query(&compiled, "variable", "score");
    assert!(result.complete, "{:?}", result.reasons);
    assert_eq!(result.returned, 1);
    assert_eq!(result.records[0].kind, Kind::GlobalWrite);
    assert_eq!(context(&result.records[0]), Role::GlobalInitializer);
    assert_eq!(
        result.records[0].from_ref,
        TargetRef::new("fragment", "make")
    );
}

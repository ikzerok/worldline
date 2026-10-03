use std::collections::BTreeSet;
use worldline_core::{
    compile_source_with_options, CompileOptions, CompileResult, RelationQueryDirection, TargetRef,
    WorldContextError, WorldContextIdentity, WorldContextKind as Kind, WorldContextLimit as Limit,
    WorldContextOptions, WorldContextPrecision, WorldContextProvenance,
};
const SOURCE: &str = r#"character linqi as "林栖"
  property mentor = ref("character", "lingzhou")
  relation lingzhou as "伙伴"
character lingzhou as "绫舟"
entity north_lighthouse kind place as "北灯塔"
relation_type guards as "看守"
  direction directed
relation_def guard_1 type guards from character lingzhou to entity north_lighthouse
relation_def guard_2 type guards from character lingzhou to entity north_lighthouse
period night
event witness during night with lingzhou
  [[character:lingzhou|绫舟]] [[character:lingzhou|绫舟]] 绫舟港和绫舟
  -> END
event lights during night follows witness
  失灯
  -> END
"#;
fn compile(source: &str) -> CompileResult {
    compile_source_with_options(
        "world.wl",
        source,
        CompileOptions::v1_13()
            .with_object_refs(true)
            .with_character_refs(true),
    )
}
fn target() -> TargetRef {
    TargetRef::new("character", "lingzhou")
}
#[test]
fn typed_context_preserves_roles_parallel_edges_and_occurrences() {
    let result = compile(SOURCE);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let query = result
        .query_world_context(&target(), WorldContextOptions::default())
        .unwrap();
    assert!(query.complete);
    assert_eq!(query.total, Some(7));
    assert_eq!(query.returned, 7);
    let count = |kind| {
        query
            .records
            .iter()
            .filter(|record| record.kind == kind)
            .count()
    };
    assert_eq!(count(Kind::FormalRelation), 2);
    assert_eq!(count(Kind::LegacyCharacterRelation), 1);
    assert_eq!(count(Kind::PropertyReference), 1);
    assert_eq!(count(Kind::EventParticipation), 1);
    assert_eq!(count(Kind::ExplicitBodyLink), 2);
    assert_eq!(count(Kind::TextMention), 0);
    let ids: BTreeSet<_> = query.records.iter().map(|record| &record.id).collect();
    assert_eq!(ids.len(), query.records.len());
    let mentor = query
        .records
        .iter()
        .find(|record| record.kind == Kind::PropertyReference)
        .unwrap();
    assert_eq!(mentor.from_ref, TargetRef::new("character", "linqi"));
    assert_eq!(mentor.to_ref, target());
    assert_eq!(mentor.role, "mentor");
    assert!(
        matches!(&mentor.provenance, WorldContextProvenance::PropertyReference { property } if property == "mentor")
    );
    assert_eq!(mentor.source.line, 2);
    assert_eq!(mentor.source.precision, WorldContextPrecision::Column);
    for record in query
        .records
        .iter()
        .filter(|record| record.kind == Kind::ExplicitBodyLink)
    {
        assert_eq!(record.source.precision, WorldContextPrecision::Line);
        assert_eq!(record.source.column, None);
        assert_eq!(record.source.line, 12);
    }
    for record in query
        .records
        .iter()
        .filter(|record| record.kind == Kind::FormalRelation)
    {
        assert_eq!(record.identity, WorldContextIdentity::PersistentRelation);
        assert_eq!(record.from_ref, target());
    }
    // 旧接口只包含独立关系，不因统一上下文改变语义。
    assert_eq!(
        result
            .analysis
            .catalog
            .query_relations(&target(), Default::default())
            .edges
            .len(),
        2
    );
}
#[test]
fn mentions_remain_optional_and_do_not_replace_explicit_links() {
    let result = compile(SOURCE);
    let query = result
        .query_world_context(
            &target(),
            WorldContextOptions {
                include_text_mentions: true,
                ..Default::default()
            },
        )
        .unwrap();
    let mentions: Vec<_> = query
        .records
        .iter()
        .filter(|record| record.kind == Kind::TextMention)
        .collect();
    assert!(mentions.iter().any(|record| matches!(&record.provenance, WorldContextProvenance::TextMention { preview } if preview.contains("绫舟港"))));
    assert_eq!(
        query
            .records
            .iter()
            .filter(|record| record.kind == Kind::ExplicitBodyLink)
            .count(),
        2
    );
    assert!(mentions
        .iter()
        .all(|record| record.identity == WorldContextIdentity::SnapshotOccurrence));
}
#[test]
fn direction_never_creates_a_reverse_relation_identity() {
    let result = compile(SOURCE);
    let outgoing = result
        .query_world_context(
            &target(),
            WorldContextOptions {
                direction: RelationQueryDirection::Outgoing,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(outgoing.returned, 2);
    let reverse = result
        .query_world_context(
            &TargetRef::new("entity", "north_lighthouse"),
            WorldContextOptions {
                direction: RelationQueryDirection::Incoming,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        reverse.records.iter().map(|r| &r.id).collect::<Vec<_>>(),
        outgoing.records.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    assert!(reverse.records.iter().all(|r| r.from_ref == target()));
}
#[test]
fn unchanged_compiles_serialize_identically_and_stale_source_is_rejected() {
    let first = compile(SOURCE)
        .query_world_context(&target(), Default::default())
        .unwrap();
    let expected = serde_json::to_string(&first).unwrap();
    for _ in 0..100 {
        let current = compile(SOURCE)
            .query_world_context(&target(), Default::default())
            .unwrap();
        assert_eq!(serde_json::to_string(&current).unwrap(), expected);
    }
    let changed = compile(&format!("{SOURCE}// changed\n"));
    assert_eq!(
        changed
            .query_world_context(
                &target(),
                WorldContextOptions {
                    expected_snapshot: Some(first.snapshot),
                    ..Default::default()
                }
            )
            .unwrap_err(),
        WorldContextError::StaleSnapshot
    );
}
#[test]
fn unknown_invalid_source_missing_endpoint_and_limits_are_distinct() {
    let result = compile(SOURCE);
    assert!(matches!(
        result.query_world_context(&TargetRef::new("character", "deleted"), Default::default()),
        Err(WorldContextError::UnknownTarget(_))
    ));
    let limited = result
        .query_world_context(
            &target(),
            WorldContextOptions {
                max_records: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(limited.total, Some(7));
    assert_eq!(limited.returned, 2);
    assert!(limited.truncated && !limited.complete);
    assert!(limited.reasons.contains(&Limit::RecordLimit));
    let budget = result
        .query_world_context(
            &target(),
            WorldContextOptions {
                max_candidates: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(budget.total, None);
    assert_eq!(budget.returned, 2);
    assert!(budget.reasons.contains(&Limit::CandidateBudget));
    let nodes = result
        .query_world_context(
            &target(),
            WorldContextOptions {
                max_nodes: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(nodes.returned, 0);
    assert_eq!(nodes.total, Some(7));
    assert!(nodes.reasons.contains(&Limit::NodeLimit));
    let broken = compile(&SOURCE.replace("character lingzhou as \"绫舟\"", ""));
    let incomplete = broken
        .query_world_context(&TargetRef::new("character", "linqi"), Default::default())
        .unwrap();
    assert!(!incomplete.complete);
    assert!(!incomplete.truncated);
    assert!(incomplete.reasons.contains(&Limit::InvalidSource));
    assert!(incomplete
        .nodes
        .iter()
        .any(|node| node.target == target() && !node.exists));
    assert!(!incomplete.diagnostics.is_empty());
    assert_eq!(
        result
            .query_world_context_cancellable(&target(), Default::default(), || true)
            .unwrap_err(),
        WorldContextError::Cancelled
    );
}
#[test]
fn cancellation_during_scan_does_not_return_a_partial_complete_result() {
    let result = compile(SOURCE);
    let mut calls = 0;
    let result = result.query_world_context_cancellable(&target(), Default::default(), || {
        calls += 1;
        calls > 5
    });
    assert_eq!(result.unwrap_err(), WorldContextError::Cancelled);
}
#[test]
fn exact_lookup_and_two_hops_ignore_unrelated_catalog_size() {
    let mut source = String::from("relation_type next\n");
    for index in 0..5000 {
        source.push_str(&format!("entity e{index} kind place\n"));
    }
    for index in 0..4999 {
        source.push_str(&format!(
            "relation_def r{index} type next from entity e{index} to entity e{}\n",
            index + 1
        ));
    }
    source.push_str("event start\n  -> END\n");
    let result = compile(&source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.analysis.catalog.objects.len() > 10_000);
    let selected = TargetRef::new("entity", "e2500");
    assert_eq!(
        result.lookup_world_object(&selected).unwrap().target,
        selected
    );
    let query = result
        .query_world_context(
            &selected,
            WorldContextOptions {
                depth: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(query.complete);
    assert_eq!(query.returned, 4);
    assert_eq!(query.nodes.len(), 5);
}
#[test]
fn duplicate_legacy_occurrences_are_preserved_even_when_diagnosed() {
    let source = SOURCE.replace(
        "  relation lingzhou as \"伙伴\"",
        "  relation lingzhou as \"伙伴\"\n  relation lingzhou as \"伙伴\"",
    );
    let result = compile(&source);
    let query = result
        .query_world_context(
            &target(),
            WorldContextOptions {
                kinds: vec![Kind::LegacyCharacterRelation],
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(query.returned, 2);
    assert_ne!(query.records[0].id, query.records[1].id);
    assert!(!query.complete);
}

#[test]
fn formal_relation_keeps_scope_qualifiers_in_typed_provenance() {
    let source = SOURCE.replace(
        "relation_def guard_1 type guards from character lingzhou to entity north_lighthouse\n",
        "relation_def guard_1 type guards from character lingzhou to entity north_lighthouse\n  scope period night\n",
    );
    let result = compile(&source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let context = result
        .query_world_context(&target(), Default::default())
        .unwrap();
    let relation = context
        .records
        .iter()
        .find(|record| record.id == "relation:guard_1")
        .unwrap();
    assert!(matches!(&relation.provenance,
        WorldContextProvenance::FormalRelation { scope_refs, .. }
        if scope_refs == &vec![TargetRef::new("period", "night")]));
    let json = serde_json::to_value(relation).unwrap();
    assert_eq!(
        json["provenance"]["scope_refs"],
        serde_json::json!([{"kind":"period","id":"night"}])
    );
    let unscoped = context
        .records
        .iter()
        .find(|record| record.id == "relation:guard_2")
        .unwrap();
    assert!(
        matches!(&unscoped.provenance, WorldContextProvenance::FormalRelation { scope_refs, .. } if scope_refs.is_empty())
    );
}

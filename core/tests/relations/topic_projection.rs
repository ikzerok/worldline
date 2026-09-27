use worldline_core::{compile_source_with_options, RelationQueryDirection, TargetRef};

#[test]
fn topic_projection_uses_only_explicit_role_mapping_and_keeps_relation_identity() {
    let source = r#"
character lin as "林舟"
  relation mei as "旧人物关系"
character mei as "梅"
entity mei kind place as "梅"
period now as "现在"
period past as "过去"
entity version_one kind version as "版本一"
entity version_two kind version as "版本二"
relation_type biological_parent as "亲生关系"
relation_type adoptive_parent as "收养关系"
relation_def birth_a type biological_parent from character lin to character mei
  scope period now
  scope entity version_one
relation_def birth_b type biological_parent from character lin to character mei
  scope period now
  scope entity version_two
relation_def adoption type adoptive_parent from character lin to character mei
  scope period past
  scope entity version_one
relation_def cycle type biological_parent from character mei to character lin
  scope period now
  scope entity version_one
"#;
    let result =
        compile_source_with_options("world.wl", source, worldline_core::CompileOptions::v1_10());
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let fingerprint = result.analysis.fingerprint;
    let target = TargetRef::new("character", "lin");
    let options = worldline_core::TopicProjectionOptions {
        role_mapping: std::collections::BTreeMap::from([
            ("biological_parent".into(), "生亲".into()),
            ("adoptive_parent".into(), "养亲".into()),
        ]),
        depth: 2,
        scope_refs: vec![
            TargetRef::new("period", "now"),
            TargetRef::new("entity", "version_one"),
        ],
        ..Default::default()
    };

    let mapped = result
        .analysis
        .query_topic_projection(&target, options.clone())
        .unwrap();
    assert_eq!(
        mapped
            .relations
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["birth_a", "cycle"]
    );
    assert!(mapped
        .relations
        .edges
        .iter()
        .all(|edge| edge.role == "生亲"));
    assert!(mapped.relations.cycle_hint);
    let birth = mapped
        .relations
        .edges
        .iter()
        .find(|edge| edge.id == "birth_a")
        .unwrap();
    assert_eq!(
        birth.scope_refs,
        [
            TargetRef::new("period", "now"),
            TargetRef::new("entity", "version_one")
        ]
    );
    assert!(mapped
        .relations
        .edges
        .iter()
        .all(|edge| { edge.file == "world.wl" && edge.from_ref.kind == "character" }));
    assert!(!mapped.relations.truncated);
    let outgoing = result
        .analysis
        .query_topic_projection(
            &target,
            worldline_core::TopicProjectionOptions {
                scope_refs: Vec::new(),
                direction: RelationQueryDirection::Outgoing,
                depth: 1,
                ..options.clone()
            },
        )
        .unwrap();
    assert!(!outgoing.relations.cycle_hint);
    let all_scopes = result
        .analysis
        .query_topic_projection(
            &target,
            worldline_core::TopicProjectionOptions {
                scope_refs: Vec::new(),
                ..options.clone()
            },
        )
        .unwrap();
    assert_eq!(
        all_scopes
            .relations
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["adoption", "birth_a", "birth_b", "cycle"]
    );
    assert_eq!(
        all_scopes
            .relations
            .edges
            .iter()
            .find(|edge| edge.id == "adoption")
            .unwrap()
            .role,
        "养亲"
    );
    let other_version = result
        .analysis
        .query_topic_projection(
            &target,
            worldline_core::TopicProjectionOptions {
                scope_refs: vec![
                    TargetRef::new("period", "now"),
                    TargetRef::new("entity", "version_two"),
                ],
                ..options.clone()
            },
        )
        .unwrap();
    assert_eq!(
        other_version
            .relations
            .edges
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["birth_b"]
    );
    let same_id_other_kind = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("entity", "mei"),
            worldline_core::TopicProjectionOptions {
                role_mapping: options.role_mapping.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(same_id_other_kind.relations.edges.is_empty());

    let bounded = result
        .analysis
        .query_topic_projection(
            &target,
            worldline_core::TopicProjectionOptions {
                max_edges: 1,
                ..options
            },
        )
        .unwrap();
    assert_eq!(bounded.relations.edges.len(), 1);
    assert!(bounded.relations.truncated);
    assert!(bounded.relations.continuation.is_some());

    let unmapped = result
        .analysis
        .query_topic_projection(&target, worldline_core::TopicProjectionOptions::default())
        .unwrap();
    assert!(unmapped.relations.edges.is_empty());
    assert_eq!(unmapped.relations.nodes.len(), 1);

    let unknown = result.analysis.query_topic_projection(
        &target,
        worldline_core::TopicProjectionOptions {
            role_mapping: std::collections::BTreeMap::from([(
                "unrecognized_type".into(),
                "关系".into(),
            )]),
            ..Default::default()
        },
    );
    assert!(matches!(
        unknown,
        Err(worldline_core::TopicProjectionError::UnknownRelationType(_))
    ));
    let empty_role = result.analysis.query_topic_projection(
        &target,
        worldline_core::TopicProjectionOptions {
            role_mapping: std::collections::BTreeMap::from([(
                "biological_parent".into(),
                "   ".into(),
            )]),
            ..Default::default()
        },
    );
    assert!(matches!(
        empty_role,
        Err(worldline_core::TopicProjectionError::EmptyRoleLabel(_))
    ));
    assert_eq!(result.analysis.fingerprint, fingerprint);
}
#[test]
fn topic_projection_keeps_explicit_history_and_partial_time_without_inference() {
    let source = r#"
character lin as "林舟"
entity harbor kind place as "雾港"
period era as "旧纪元"
entity version_one kind version as "版本一"
period disputed as "争议年代"
anchor_def arc as "转折"
anchor_link arc character lin
anchor_link arc event alpha
event alpha with lin during era
  甲记录。
  -> END
event beta with lin during era
  乙记录。
  -> END
event later with lin during era follows alpha
  后续记录。
  -> END
event undated with lin
  文献没有日期。
  -> END
event mention
  [[entity:harbor|港口提及]]
  -> END
relation_type happens_at as "发生地点"
relation_def alpha_at type happens_at from event alpha to entity harbor
  scope period era
  scope entity version_one
relation_def beta_at type happens_at from event beta to entity harbor
relation_def undated_at type happens_at from event undated to entity harbor
relation_def reverse_at type happens_at from entity harbor to event later
relation_type date_claim as "日期主张"
relation_def claim_a type date_claim from event alpha to period era
  source_note "史料甲"
relation_def claim_b type date_claim from event alpha to period disputed
  source_note "史料乙"
"#;
    let result =
        compile_source_with_options("world.wl", source, worldline_core::CompileOptions::v1_10());
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);

    let character_history = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("character", "lin"),
            worldline_core::TopicProjectionOptions::default(),
        )
        .unwrap();
    assert_eq!(
        character_history
            .history
            .items
            .iter()
            .map(|item| item.event.id.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta", "later", "undated"]
    );
    assert!(character_history.history.items.iter().all(|item| matches!(
        &item.source,
        worldline_core::TopicProjectionHistorySource::With
    )));
    assert_eq!(
        character_history
            .history
            .temporal_edges
            .iter()
            .map(|edge| (edge.before.as_str(), edge.after.as_str()))
            .collect::<Vec<_>>(),
        [("alpha", "later")]
    );
    assert_eq!(
        character_history.history.parallel_groups,
        vec![vec![
            TargetRef::new("event", "alpha"),
            TargetRef::new("event", "beta")
        ]]
    );
    let undated = character_history
        .history
        .events
        .iter()
        .find(|event| event.target.id == "undated")
        .unwrap();
    assert_eq!(
        undated.time_status,
        worldline_core::TopicProjectionTimeStatus::Unknown
    );
    assert!(undated.period.is_none() && undated.rank.is_none());
    assert_eq!(
        character_history.history.target_anchors,
        [TargetRef::new("anchor", "arc")]
    );
    assert_eq!(
        character_history
            .history
            .events
            .iter()
            .find(|event| event.target.id == "alpha")
            .unwrap()
            .anchors,
        [TargetRef::new("anchor", "arc")]
    );

    let place_history = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("entity", "harbor"),
            worldline_core::TopicProjectionOptions {
                role_mapping: std::collections::BTreeMap::from([
                    ("happens_at".into(), "记录地点".into()),
                    ("date_claim".into(), "日期主张".into()),
                ]),
                depth: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        place_history
            .history
            .items
            .iter()
            .map(|item| match &item.source {
                worldline_core::TopicProjectionHistorySource::Relation { id, .. } => id.as_str(),
                worldline_core::TopicProjectionHistorySource::With => "",
            })
            .collect::<Vec<_>>(),
        ["alpha_at", "beta_at", "undated_at"]
    );
    let alpha_at = place_history
        .history
        .items
        .iter()
        .find(|item| item.event.id == "alpha")
        .unwrap();
    assert!(matches!(
        &alpha_at.source,
        worldline_core::TopicProjectionHistorySource::Relation { scope_refs, .. }
            if scope_refs == &vec![
                TargetRef::new("period", "era"),
                TargetRef::new("entity", "version_one")
            ]
    ));
    let claims: Vec<_> = place_history
        .relations
        .edges
        .iter()
        .filter(|edge| edge.relation_type == "date_claim")
        .collect();
    assert_eq!(
        claims
            .iter()
            .map(|edge| edge.id.as_str())
            .collect::<Vec<_>>(),
        ["claim_a", "claim_b"]
    );
    assert_eq!(
        claims
            .iter()
            .map(|edge| edge.to_ref.id.as_str())
            .collect::<Vec<_>>(),
        ["era", "disputed"]
    );
    assert!(claims.iter().all(|edge| edge.role == "日期主张"));
    assert_eq!(
        claims
            .iter()
            .map(|edge| edge.source_note.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["史料甲", "史料乙"]
    );
    assert!(!place_history
        .history
        .items
        .iter()
        .any(|item| item.event.id == "later" || item.event.id == "mention"));
    let first_page = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("entity", "harbor"),
            worldline_core::TopicProjectionOptions {
                role_mapping: std::collections::BTreeMap::from([
                    ("happens_at".into(), "记录地点".into()),
                    ("date_claim".into(), "日期主张".into()),
                ]),
                depth: 2,
                max_edges: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(first_page.history.truncated);
    assert_eq!(first_page.history.next_offset, Some(1));
    assert_eq!(first_page.history.items[0].event.id, "alpha");
    let second_page = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("entity", "harbor"),
            worldline_core::TopicProjectionOptions {
                role_mapping: std::collections::BTreeMap::from([
                    ("happens_at".into(), "记录地点".into()),
                    ("date_claim".into(), "日期主张".into()),
                ]),
                depth: 2,
                history_offset: first_page.history.next_offset.unwrap(),
                max_edges: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(second_page.history.items[0].event.id, "beta");
    let zero_node_budget = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("character", "lin"),
            worldline_core::TopicProjectionOptions {
                max_nodes: 0,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(zero_node_budget.history.items.len(), 1);
    assert!(zero_node_budget.history.truncated);
    assert_eq!(zero_node_budget.history.next_offset, Some(1));
    let zero_history_budget = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("entity", "harbor"),
            worldline_core::TopicProjectionOptions {
                role_mapping: std::collections::BTreeMap::from([
                    ("happens_at".into(), "记录地点".into()),
                    ("date_claim".into(), "日期主张".into()),
                ]),
                max_nodes: 0,
                max_edges: 0,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(zero_history_budget.truncated);
    assert!(zero_history_budget.relations.truncated);
    assert!(zero_history_budget.relations.continuation.is_none());
    assert!(zero_history_budget.history.items.is_empty());
    assert!(zero_history_budget.history.truncated);
    assert!(zero_history_budget.history.next_offset.is_none());
}
#[test]
fn topic_projection_hints_undirected_topology_cycles() {
    let result = compile_source_with_options(
        "cycle.wl",
        r#"
entity a kind place as "甲"
entity b kind place as "乙"
entity c kind place as "丙"
relation_type near as "相邻"
  direction undirected
  from entity
  to entity
relation_def a_b type near from entity a to entity b
relation_def b_c type near from entity b to entity c
relation_def c_a type near from entity c to entity a
"#,
        worldline_core::CompileOptions::v1_10(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);

    let projection = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("entity", "a"),
            worldline_core::TopicProjectionOptions {
                role_mapping: std::collections::BTreeMap::from([("near".into(), "相邻".into())]),
                depth: 2,
                ..Default::default()
            },
        )
        .unwrap();

    assert_eq!(projection.relations.edges.len(), 3);
    assert!(projection.relations.cycle_hint);
}

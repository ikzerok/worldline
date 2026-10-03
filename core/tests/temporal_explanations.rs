use worldline_core::timeline::{
    TemporalComparisonReason as Reason, TemporalEdge, TemporalRelation as Relation, Timeline,
    TimelineStatus,
};
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};

const SOURCE: &str = "period night\nperiod dawn within night\nperiod morning within night\nperiod elsewhere\nevent witness during dawn\n  目击\nevent council_order during dawn\n  命令\nevent lights_out during morning follows witness, council_order\n  熄灯\nevent lighthouse during morning follows lights_out\n  灯塔\nevent cave during morning follows lights_out\n  洞穴\nevent outsider during elsewhere\n  独立根\nevent undated\n  没有时段\nevent spare during night\n  旁支\nevent spare_later during night follows spare\n  旁支后续\n";

fn compile(source: &str) -> CompileResult {
    compile_source_with_options("temporal.wl", source, CompileOptions::v1_13())
}

fn pairs(edges: &[TemporalEdge]) -> Vec<(&str, &str)> {
    edges
        .iter()
        .map(|e| (e.before.as_str(), e.after.as_str()))
        .collect()
}

fn assert_real_edges(timeline: &Timeline, source: &str, evidence: &[TemporalEdge]) {
    for edge in evidence {
        assert!(timeline.edges.contains(edge));
        assert_eq!(edge.file, "temporal.wl");
        let header = source.lines().nth(edge.line as usize - 1).unwrap();
        assert!(header.starts_with(&format!("event {} ", edge.after)));
        let predecessors = header.split(" follows ").nth(1).unwrap();
        assert!(predecessors.split(',').any(|p| p.trim() == edge.before));
    }
    for pair in evidence.windows(2) {
        assert_eq!(pair[0].after, pair[1].before);
    }
}

#[test]
fn comparisons_distinguish_identity_order_unknown_and_root_boundaries() {
    let compiled = compile(SOURCE);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let timeline = &compiled.analysis.timeline;
    for (left, right, relation, reason) in [
        ("witness", "witness", Relation::SameEvent, None),
        ("undated", "undated", Relation::SameEvent, None),
        ("witness", "lights_out", Relation::Before, None),
        ("council_order", "lights_out", Relation::Before, None),
        ("lighthouse", "witness", Relation::After, None),
        ("lighthouse", "cave", Relation::UnorderedSameRoot, None),
        (
            "witness",
            "council_order",
            Relation::UnorderedSameRoot,
            None,
        ),
        ("witness", "spare_later", Relation::UnorderedSameRoot, None),
        ("witness", "outsider", Relation::DifferentRoots, None),
        (
            "witness",
            "undated",
            Relation::Unknown,
            Some(Reason::MissingTime),
        ),
        (
            "absent",
            "absent",
            Relation::Unknown,
            Some(Reason::UnknownEvent),
        ),
        (
            "witness",
            "absent",
            Relation::Unknown,
            Some(Reason::UnknownEvent),
        ),
    ] {
        let result = timeline.compare(left, right);
        assert_eq!(
            (result.relation, result.reason),
            (relation, reason),
            "{left}/{right}"
        );
        assert_eq!(result.status, TimelineStatus::Complete);
        assert_eq!((result.left.as_str(), result.right.as_str()), (left, right));
        if !matches!(relation, Relation::Before | Relation::After) {
            assert!(result.evidence.is_empty());
        } else {
            assert_real_edges(timeline, SOURCE, &result.evidence);
        }
    }
    assert_eq!(timeline.unplaced_events, ["undated"]);
    let before = timeline.compare("witness", "lighthouse");
    assert_eq!(
        pairs(&before.evidence),
        [("witness", "lights_out"), ("lights_out", "lighthouse")]
    );
    assert_eq!(
        timeline.compare("lighthouse", "witness").evidence,
        before.evidence
    );
    let json = serde_json::to_value(before).unwrap();
    assert_eq!(json["relation"], "before");
    assert_eq!(json["reason"], serde_json::Value::Null);
}

#[test]
fn equal_and_unequal_layout_ranks_never_invent_order_or_simultaneity() {
    let compiled = compile(SOURCE);
    let timeline = &compiled.analysis.timeline;
    let rank = |name| {
        timeline
            .events
            .iter()
            .find(|e| e.event == name)
            .unwrap()
            .root_rank
    };
    assert_eq!(rank("lighthouse"), rank("cave"));
    assert_ne!(rank("witness"), rank("spare_later"));
    for (a, b) in [("lighthouse", "cave"), ("witness", "spare_later")] {
        assert_eq!(timeline.compare(a, b).relation, Relation::UnorderedSameRoot);
        assert_eq!(timeline.compare(b, a).relation, Relation::UnorderedSameRoot);
    }
}

#[test]
fn shortest_path_then_lexical_edges_is_stable_over_a_hundred_calls() {
    let source = "period root\nevent start during root\n  起点\nevent c during root follows start\n  路径丙\nevent b during root follows start\n  路径乙\nevent finish during root follows c, b\n  汇合\n";
    let compiled = compile(source);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let timeline = &compiled.analysis.timeline;
    let expected = timeline.compare("start", "finish");
    assert_eq!(pairs(&expected.evidence), [("start", "b"), ("b", "finish")]);
    assert_real_edges(timeline, source, &expected.evidence);
    for _ in 0..100 {
        assert_eq!(timeline.compare("start", "finish"), expected);
    }
    let direct = compile(&source.replace("follows c, b", "follows c, b, start"));
    assert_eq!(
        pairs(&direct.analysis.timeline.compare("start", "finish").evidence),
        [("start", "finish")]
    );
    let mut reversed = timeline.clone();
    reversed.edges.reverse();
    reversed.events.reverse();
    assert_eq!(reversed.compare("start", "finish"), expected);
}

#[test]
fn cycle_members_are_distinct_from_two_blocked_downstream_events() {
    let source = SOURCE.replace(
        "event witness during dawn",
        "event witness during dawn follows lights_out",
    );
    let compiled = compile(&source);
    assert!(compiled.has_errors());
    let timeline = &compiled.analysis.timeline;
    assert_eq!(timeline.status, TimelineStatus::Partial);
    assert!(timeline.events.iter().all(|e| e.root_rank.is_none()));
    assert_eq!(timeline.cycles.len(), 1);
    let cycle = &timeline.cycles[0];
    assert_eq!(cycle.id, "lights_out");
    assert_eq!(cycle.members, ["lights_out", "witness"]);
    assert_eq!(
        pairs(&cycle.witness),
        [("lights_out", "witness"), ("witness", "lights_out")]
    );
    assert_real_edges(timeline, &source, &cycle.witness);
    assert_eq!(
        timeline
            .blocked
            .iter()
            .map(|b| b.event.as_str())
            .collect::<Vec<_>>(),
        ["cave", "lighthouse"]
    );
    assert!(timeline
        .blocked
        .iter()
        .all(|b| b.cycle_ids == ["lights_out"]));
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == "A213")
        .collect();
    assert_eq!(errors.len(), 4);
    assert_eq!(
        errors
            .iter()
            .filter(|d| d.message.contains("属于时间约束环"))
            .count(),
        2
    );
    assert_eq!(
        errors
            .iter()
            .filter(|d| d.message.contains("受时间约束环"))
            .count(),
        2
    );
    for diagnostic in errors {
        assert!(diagnostic
            .note
            .as_ref()
            .unwrap()
            .contains("lights_out → witness → lights_out"));
        assert!(cycle.witness.iter().all(|e| diagnostic
            .related
            .iter()
            .any(|(file, span)| file == &e.file && span.line == e.line)));
    }
    let result = timeline.compare("council_order", "outsider");
    assert_eq!(
        (result.relation, result.reason),
        (Relation::Invalid, Some(Reason::PartialTimeline))
    );
    assert!(result.evidence.is_empty());
    let fixed = compile(SOURCE);
    assert!(fixed.analysis.timeline.cycles.is_empty());
    assert!(fixed.analysis.timeline.blocked.is_empty());
    assert!(!fixed.diagnostics.iter().any(|d| d.code == "A213"));
    assert_eq!(
        fixed
            .analysis
            .timeline
            .compare("witness", "lighthouse")
            .relation,
        Relation::Before
    );
    assert_eq!(compiled.analysis.fingerprint, fixed.analysis.fingerprint);
}

#[test]
fn independent_cycles_and_self_loops_report_every_downstream_cause() {
    let source = "period root\nevent a during root follows b\n  甲\nevent b during root follows a\n  乙\nevent x during root follows y\n  丙\nevent y during root follows x\n  丁\nevent z during root follows z, z\n  自环\nevent tail during root follows b, y, z\n  三环下游\nevent untouched during root\n  独立\n";
    let compiled = compile(source);
    let timeline = &compiled.analysis.timeline;
    assert_eq!(
        timeline
            .cycles
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "x", "z"]
    );
    assert_eq!(timeline.cycles[0].members, ["a", "b"]);
    assert_eq!(timeline.cycles[1].members, ["x", "y"]);
    assert_eq!(timeline.cycles[2].members, ["z"]);
    assert_eq!(pairs(&timeline.cycles[2].witness), [("z", "z")]);
    assert_eq!(timeline.blocked.len(), 1);
    assert_eq!(timeline.blocked[0].event, "tail");
    assert_eq!(timeline.blocked[0].cycle_ids, ["a", "x", "z"]);
    for cycle in &timeline.cycles {
        assert_real_edges(timeline, source, &cycle.witness);
    }
    assert_eq!(
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.code == "A213")
            .count(),
        6
    );
    let expected = serde_json::to_string(&timeline).unwrap();
    for _ in 0..100 {
        assert_eq!(
            serde_json::to_string(&compile(source).analysis.timeline).unwrap(),
            expected
        );
    }
}

#[test]
fn downstream_cycles_remain_true_components_and_propagate_causes() {
    let source = "period root\nevent a during root follows b\n  甲\nevent b during root follows a\n  乙\nevent x during root follows y, b\n  丙\nevent y during root follows x\n  丁\nevent tail during root follows y\n  下游\n";
    let compiled = compile(source);
    let timeline = &compiled.analysis.timeline;
    assert_eq!(timeline.cycles.len(), 2);
    assert_eq!(timeline.cycles[0].members, ["a", "b"]);
    assert_eq!(timeline.cycles[1].members, ["x", "y"]);
    assert_eq!(timeline.blocked.len(), 1);
    assert_eq!(timeline.blocked[0].event, "tail");
    assert_eq!(timeline.blocked[0].cycle_ids, ["a", "x"]);
}

#[test]
fn one_component_can_contain_more_members_than_its_minimal_cycle_witness() {
    let source = "period root\nevent a during root follows b, c\n  甲\nevent b during root follows a\n  乙\nevent c during root follows d, a\n  丙\nevent d during root follows c\n  丁\n";
    let compiled = compile(source);
    let timeline = &compiled.analysis.timeline;
    assert_eq!(timeline.cycles.len(), 1);
    assert_eq!(timeline.cycles[0].members, ["a", "b", "c", "d"]);
    assert_eq!(pairs(&timeline.cycles[0].witness), [("a", "b"), ("b", "a")]);
    assert!(timeline.blocked.is_empty());
}

#[test]
fn old_language_retains_direct_scope_and_numeric_at_is_not_a_date() {
    let source = "period root\nperiod early within root\nperiod late within root\nevent a during early\n  甲\nevent b during late at 20261099\n  乙\n";
    for options in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
        CompileOptions::v1_12(),
    ] {
        let old = compile_source_with_options("temporal.wl", source, options);
        assert!(!old.has_errors(), "{:?}", old.diagnostics);
        let comparison = old.analysis.timeline.compare("a", "b");
        assert_eq!(
            (comparison.relation, comparison.reason),
            (Relation::Unknown, Some(Reason::DifferentOrderScopes))
        );
    }
    let new = compile(source);
    assert!(!new.has_errors(), "{:?}", new.diagnostics);
    assert_eq!(
        new.analysis.timeline.compare("a", "b").relation,
        Relation::UnorderedSameRoot
    );
    assert!(new.analysis.timeline.edges.is_empty());
}

#[test]
fn invalid_snapshots_preserve_partial_protection_even_without_a_cycle() {
    for source in [
        "period\n",
        "event start\n  -> missing\n",
        "period a\nperiod b\nevent x during a\n  甲\nevent y during b follows x\n  乙\n",
        "period root within missing\nevent x during root\n  甲\n",
        "period root\nevent x during root follows absent\n  甲\n",
    ] {
        let compiled = compile(source);
        assert!(compiled.has_errors(), "{source}");
        let timeline = &compiled.analysis.timeline;
        assert!(timeline.cycles.is_empty());
        assert!(timeline.blocked.is_empty());
        let comparison = timeline.compare("x", "x");
        assert_eq!(comparison.status, TimelineStatus::Partial);
        assert_eq!(
            (comparison.relation, comparison.reason),
            (Relation::Invalid, Some(Reason::PartialTimeline))
        );
        assert!(comparison.evidence.is_empty());
    }
}

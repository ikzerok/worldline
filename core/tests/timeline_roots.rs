use worldline_core::timeline::{TemporalOrderScope, TimelineStatus};
use worldline_core::{
    compile_source_with_options, CompileOptions, CompileResult, TargetRef, TopicProjectionOptions,
};

const SOURCE: &str = "character witness\nperiod year\nperiod summer within year\nperiod autumn within year\nperiod early within autumn\nevent opening during summer with witness\n  开幕\nevent closing during early follows opening with witness\n  闭幕\nevent independent during autumn with witness\n  日期未知\nevent undated with witness\n  无时段\n";
fn compile(source: &str) -> CompileResult {
    compile_source_with_options("seasons.wl", source, CompileOptions::v1_13())
}
fn event<'a>(result: &'a CompileResult, id: &str) -> &'a worldline_core::timeline::TemporalEvent {
    result
        .analysis
        .timeline
        .events
        .iter()
        .find(|e| e.event == id)
        .unwrap()
}

#[test]
fn new_version_only_shares_declared_root_and_keeps_direct_membership() {
    for options in [
        CompileOptions::default(),
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
        CompileOptions::v1_12(),
    ] {
        let old = compile_source_with_options("seasons.wl", SOURCE, options);
        assert!(old.diagnostics.iter().any(|d| d.code == "A213"));
        assert_eq!(
            old.analysis.timeline.order_scope,
            TemporalOrderScope::DirectPeriod
        );
        assert_eq!(old.analysis.timeline.status, TimelineStatus::Partial);
    }
    let result = compile(SOURCE);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let timeline = &result.analysis.timeline;
    assert_eq!(timeline.status, TimelineStatus::Complete);
    assert_eq!(timeline.order_scope, TemporalOrderScope::RootPeriod);
    assert_eq!(timeline.edges.len(), 1);
    let opening = event(&result, "opening");
    let closing = event(&result, "closing");
    assert_eq!(opening.period, "summer");
    assert_eq!(closing.period, "early");
    assert_eq!(closing.root.as_deref(), Some("year"));
    assert_eq!(closing.order_scope.as_deref(), Some("year"));
    assert_eq!((opening.rank, closing.rank), (0, 0));
    assert_eq!((opening.root_rank, closing.root_rank), (Some(0), Some(1)));
    assert_eq!(event(&result, "independent").root_rank, Some(0));
    assert!(timeline.events.iter().all(|e| e.event != "undated"));
    let mermaid = timeline.to_mermaid(&result.analysis.graph);
    assert_eq!(mermaid.matches("subgraph").count(), 4);
    assert!(mermaid.contains("范围 year · 层 1") && mermaid.contains("独立根不可比"));
    assert_eq!(timeline.edges[0].file, "seasons.wl");
    assert_eq!(timeline.edges[0].line, 8);
}

#[test]
fn direct_rank_ignores_cross_period_paths_while_root_rank_keeps_them() {
    let source = "period root\nperiod a within root\nperiod b within root\nevent one during a\n  一\nevent two during b follows one\n  二\nevent three during a follows two\n  三\nevent four during a follows three\n  四\n";
    let result = compile(source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(event(&result, "three").rank, 0);
    assert_eq!(event(&result, "three").root_rank, Some(2));
    assert_eq!(event(&result, "four").rank, 1);
    assert_eq!(event(&result, "four").root_rank, Some(3));
}

#[test]
fn root_itself_and_any_depth_descendants_can_share_explicit_edges() {
    let result = compile(&SOURCE.replace("during summer", "during year"));
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(event(&result, "closing").root_rank, Some(1));
    let no_edges = compile(&SOURCE.replace(" follows opening", ""));
    assert!(no_edges.analysis.timeline.edges.is_empty());
    assert!(no_edges
        .analysis
        .timeline
        .events
        .iter()
        .all(|e| e.root_rank == Some(0)));
}

#[test]
fn unrelated_roots_missing_members_and_ambiguous_parentage_are_partial() {
    for source in [
        SOURCE.replace("period autumn within year", "period autumn"),
        SOURCE.replace("during summer", "during missing"),
        SOURCE.replace(" follows opening", " follows absent"),
        SOURCE.replace("during summer ", ""),
        SOURCE.replace("period summer within year", "period summer within absent"),
        SOURCE.replace("period year", "period year within early"),
        SOURCE.replace("period year", "period year\nperiod year"),
    ] {
        let result = compile(&source);
        assert!(result.has_errors(), "{source}");
        assert_eq!(result.analysis.timeline.status, TimelineStatus::Partial);
        assert!(result.analysis.timeline.events.iter().all(|e| {
            e.status == TimelineStatus::Partial
                && e.root_rank.is_none()
                && result.analysis.timeline.event_rank(e).is_none()
        }));
    }
    let separate = compile(
        &SOURCE
            .replace("period autumn within year", "period autumn")
            .replace(" follows opening", ""),
    );
    assert!(!separate.has_errors(), "{:?}", separate.diagnostics);
    assert_eq!(event(&separate, "opening").root.as_deref(), Some("year"));
    assert_eq!(event(&separate, "closing").root.as_deref(), Some("autumn"));
}

#[test]
fn cross_period_cycles_self_edges_and_dependents_report_sources() {
    for source in [
        SOURCE.replace(
            "event opening during summer",
            "event opening during summer follows closing",
        ),
        SOURCE.replace("follows opening", "follows closing"),
    ] {
        let result = compile(&source);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == "A213")
            .collect();
        assert!(!errors.is_empty(), "{:?}", result.diagnostics);
        assert!(errors
            .iter()
            .all(|d| d.file == "seasons.wl" && !d.related.is_empty()));
        assert_eq!(result.analysis.timeline.status, TimelineStatus::Partial);
    }
}

#[test]
fn duplicate_constraints_are_deduplicated_and_no_dates_or_transitive_edges_are_inferred() {
    let result = compile(&SOURCE.replace("follows opening", "follows opening, opening"));
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.timeline.edges.len(), 1);
    let value = serde_json::to_value(&result.analysis.timeline).unwrap();
    assert!(value["events"][0].get("date").is_none());
    assert_eq!(value["status"], "complete");
    assert_eq!(value["order_scope"], "root_period");
}

#[test]
fn parse_failures_and_non_temporal_errors_cannot_look_like_complete_empty_graphs() {
    for source in ["period\n", "event start\n  -> missing\n"] {
        let result = compile(source);
        assert!(result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(result.analysis.timeline.status, TimelineStatus::Partial);
        assert!(result
            .analysis
            .timeline
            .to_mermaid(&result.analysis.graph)
            .contains("不完整"));
    }
    let valid = compile("event start\n  -> END\n");
    assert!(!valid.has_errors());
    assert_eq!(valid.analysis.timeline.status, TimelineStatus::Complete);
}

#[test]
fn history_projection_uses_root_scope_and_cannot_group_comparable_cross_period_events() {
    let result = compile(SOURCE);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let history = result
        .analysis
        .query_topic_projection(
            &TargetRef::new("character", "witness"),
            TopicProjectionOptions::default(),
        )
        .unwrap()
        .history;
    let closing = history
        .events
        .iter()
        .find(|e| e.target.id == "closing")
        .unwrap();
    assert_eq!(closing.period, Some(TargetRef::new("period", "early")));
    assert_eq!(closing.rank, Some(0));
    assert_eq!(closing.root_rank, Some(1));
    assert_eq!(closing.order_scope, Some(TargetRef::new("period", "year")));
    assert_eq!(
        history.parallel_groups,
        vec![vec![
            TargetRef::new("event", "independent"),
            TargetRef::new("event", "opening")
        ]]
    );
    let invalid = compile(&SOURCE.replace("follows opening", "follows missing"));
    let history = invalid
        .analysis
        .query_topic_projection(
            &TargetRef::new("character", "witness"),
            TopicProjectionOptions::default(),
        )
        .unwrap()
        .history;
    assert_eq!(history.timeline_status, TimelineStatus::Partial);
    assert!(history.parallel_groups.is_empty());
    assert!(history
        .events
        .iter()
        .all(|e| e.rank.is_none() && e.root_rank.is_none()));
}

#[test]
fn parent_edit_revalidates_constraints_and_rejected_edit_preserves_source() {
    let root = std::env::temp_dir().join(format!("wl-root-order-memory-{}", std::process::id()));
    let mut project = worldline_core::project::Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|p, _| p == &entry);
    project
        .set_text(&entry, SOURCE.replace(" follows opening", ""))
        .unwrap();
    project
        .create_authoring_document(
            &root.join(".world/project.json"),
            br#"{"schema_version":1,"language_version":"1.13","required_features":[]}"#.to_vec(),
        )
        .unwrap();
    let fingerprint = project.compile().analysis.fingerprint;
    project
        .edit(|p| p.order_events("opening", "closing"))
        .unwrap();
    let result = project.compile();
    assert_eq!(result.analysis.fingerprint, fingerprint);
    assert_eq!(event(&result, "closing").root_rank, Some(1));
    let sources = project.sources();
    assert!(project
        .edit(|p| p.write_period_with_parent("autumn", "秋", None))
        .is_err());
    assert_eq!(project.sources(), sources);
    assert!(project
        .edit(|p| p.order_events("closing", "opening"))
        .is_err());
    assert_eq!(project.sources(), sources);
}

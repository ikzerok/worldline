use worldline_core::compile_source;

#[test]
fn nested_periods_compile_and_invalid_parent_graphs_fail() {
    let source = "period year as \"全年\"\nperiod month as \"九月\" within year\nperiod day within month\nevent start during day\n  -> END\n";
    let result = compile_source("world.wl", source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    for declarations in [
        "period a within missing",
        "period a within a",
        "period a within b\nperiod b within a",
    ] {
        let result = compile_source(
            "world.wl",
            &format!("{declarations}\nevent start\n  -> END\n"),
        );
        assert!(result.diagnostics.iter().any(|d| d.code == "A219"));
    }
}

#[test]
fn period_edit_roundtrip_keeps_parent_and_runtime_fingerprint() {
    let mut project =
        worldline_core::project::Project::new(&std::env::temp_dir().join("wl-period-memory"));
    project.documents.retain(|p, _| p == &project.entry);
    project.set_text(&project.entry.clone(), "period year as \"全年\"\nperiod month as \"九月\" // 保留说明\nevent start during month\n  -> END\n".into()).unwrap();
    let before = project.compile().analysis.fingerprint;
    project
        .edit(|p| p.write_period_with_parent("month", "九月", Some("year")))
        .unwrap();
    project
        .edit(|p| p.write_period("month", "新的月份名"))
        .unwrap();
    let result = project.compile();
    assert_eq!(result.analysis.fingerprint, before);
    let order = result.analysis.timeline.period_order();
    assert_eq!(
        order
            .iter()
            .map(|(p, d)| (p.id.as_str(), *d))
            .collect::<Vec<_>>(),
        vec![("year", 0), ("month", 1)]
    );
    assert_eq!(order[1].0.parent.as_deref(), Some("year"));
    assert!(project
        .document(&project.entry)
        .unwrap()
        .contains("保留说明"));
    let sources = project.sources();
    assert!(project
        .edit(|p| p.write_period_with_parent("year", "全年", Some("month")))
        .is_err());
    assert_eq!(project.sources(), sources);
    assert_eq!(
        result
            .analysis
            .timeline
            .to_mermaid(&result.analysis.graph)
            .matches("subgraph")
            .count(),
        2
    );
}

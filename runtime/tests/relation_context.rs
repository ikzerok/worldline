use worldline_core::{compile_source, EdgeKind};

#[test]
fn graph_keeps_nested_choice_and_exclusive_branch_conditions() {
    let result = compile_source(
        "conditions.wl",
        r#"let score = 2
event start
  choice "探索" if score > 0
    if score > 3
      -> high
    else if score > 1
      -> middle
    else
      -> low
event high
  -> END
event middle after score > 0
  -> END
event low
  -> END
"#,
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let graph = &result.analysis.graph;
    let edge = graph
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Divert && graph.nodes[e.to as usize].name == "middle")
        .unwrap();
    assert_eq!(edge.contexts.len(), 1);
    assert_eq!(edge.contexts[0].choices, ["探索"]);
    assert_eq!(
        edge.contexts[0].conditions,
        ["(score > 0)", "not ((score > 3))", "(score > 1)"]
    );
    assert_eq!(edge.target_requirement.as_deref(), Some("(score > 0)"));
    let edge = graph
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Divert && graph.nodes[e.to as usize].name == "low")
        .unwrap();
    assert_eq!(
        edge.contexts[0].conditions,
        ["(score > 0)", "not ((score > 3))", "not ((score > 1))"]
    );
}

#[test]
fn same_event_scene_jump_does_not_reapply_admission_but_cross_event_does() {
    let result = compile_source("admission.wl", "event start after true\n  -> inner\n  scene inner\n    -> other\nevent other after false\n  scene scene\n    -> END\n");
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let graph = &result.analysis.graph;
    let local = graph
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Divert && graph.nodes[e.to as usize].name == "start.inner")
        .unwrap();
    assert_eq!(local.target_requirement, None);
    let cross = graph
        .edges
        .iter()
        .find(|e| e.kind == EdgeKind::Divert && graph.nodes[e.to as usize].name == "other")
        .unwrap();
    assert_eq!(cross.target_requirement.as_deref(), Some("false"));
}

//! 共享片段的展示图优化不改变原始边；oracle 由优化前工具在同一源码上生成。
use serde_json::Value;
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult};

fn compile(source: &str) -> CompileResult {
    compile_source_with_options("oracle.wl", source, CompileOptions::v1_13())
}

fn dag(depth: usize, leaf: &str) -> String {
    let mut source = format!("fragment f0()\n{leaf}");
    for level in 1..=depth {
        source.push_str(&format!(
            "fragment f{level}()\n  call f{}()\n  call f{}()\n  return\n",
            level - 1,
            level - 1
        ));
    }
    source.push_str(&format!("event start\n  call f{depth}()\n  -> END\n"));
    source
}

#[test]
fn all_transfer_edges_match_pre_optimization_oracle_in_order() {
    let source = include_str!("fixtures/fragment_projection/oracle.wl");
    let result = compile(source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/fragment_projection/oracle.json")).unwrap();
    assert_eq!(
        serde_json::to_value(&result.analysis.graph).unwrap(),
        expected
    );
}

#[test]
fn deeply_shared_return_only_dag_has_no_projected_edges_or_budget_error() {
    let result = compile(&dag(32, "  return\n"));
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.analysis.graph.edges.is_empty());
    assert!(!result.diagnostics.iter().any(|d| d.code == "A231"));
}

#[test]
fn end_and_conditional_return_helpers_are_also_transfer_free() {
    for leaf in [
        "  -> END\n",
        "  if true\n    return\n  else\n    return\n",
        "  choice \"在这里继续\"\n    return\n  return\n",
    ] {
        let result = compile(&dag(24, leaf));
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(result.analysis.graph.edges.is_empty());
    }
}

#[test]
fn unreachable_transfer_is_not_pruned_from_the_conservative_display_graph() {
    let source = "fragment helper()\n  return\n  -> target\nevent start\n  call helper()\n  -> END\nevent target\n  -> END\n";
    let result = compile(source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.graph.edges.len(), 1);
    assert_eq!(result.analysis.graph.edges[0].line, 3);
}

#[test]
fn repeated_transfer_paths_are_not_semantically_deduplicated() {
    let mut source = dag(4, "  -> target\n");
    source.push_str("event target\n  -> END\n");
    let result = compile(&source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.analysis.graph.edges.len(), 16);
    assert!(result
        .analysis
        .graph
        .edges
        .iter()
        .all(|edge| edge.line == 2));
}

#[test]
fn recursive_and_unknown_calls_keep_their_original_diagnostics() {
    for source in [
        "fragment first()\n  call second()\n  return\nfragment second()\n  call first()\n  return\nevent start\n  call first()\n  -> END\n",
        "fragment first()\n  call first()\n  -> target\nevent start\n  call first()\nevent target\n  -> END\n",
    ] {
        let result = compile(source);
        assert!(result.has_errors());
        assert!(result.diagnostics.iter().any(|d| d.code == "A230"));
        assert!(!result.diagnostics.iter().any(|d| d.code == "A231"));
    }
    let result = compile("event start\n  call missing()\n  -> END\n");
    assert!(result.diagnostics.iter().any(|d| d.code == "A103"));
}

#[test]
fn transfer_heavy_dag_stops_with_one_explicit_error_at_the_real_divert() {
    let mut source = dag(16, "  -> target\n");
    source.push_str("event target\n  -> END\n");
    let result = compile(&source);
    assert!(result.has_errors());
    let limits: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "A231")
        .collect();
    assert_eq!(limits.len(), 1);
    assert_eq!(limits[0].file, "oracle.wl");
    assert_eq!(limits[0].span.line, 2);
    assert!(limits[0].message.contains("不完整"));
    assert_eq!(result.analysis.graph.edges.len(), 8_192);
}

#[test]
fn expansion_budget_is_independent_of_edge_budget() {
    let mut source = "fragment f0()\n  -> target\n".to_string();
    for depth in 1..100 {
        source.push_str(&format!(
            "fragment f{depth}()\n  call f{}()\n  return\n",
            depth - 1
        ));
    }
    source.push_str("event start\n");
    for _ in 0..400 {
        source.push_str("  call f99()\n");
    }
    source.push_str("event target\n  -> END\n");
    let result = compile(&source);
    assert!(result.has_errors());
    let limits: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "A231")
        .collect();
    assert_eq!(limits.len(), 1);
    assert!(limits[0].note.as_ref().unwrap().contains("32768"));
    assert!(result.analysis.graph.edges.len() < 8_192);
}

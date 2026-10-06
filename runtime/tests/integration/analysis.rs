use super::*;

#[test]
fn memory_include_deduplication_cycles_and_entry_priority() {
    let dir = ProjectDir::new();
    let root = &dir.0;
    let entry = root.join("main.wl");
    let mut sources = std::collections::BTreeMap::from([
        (
            entry.clone(),
            "include \"chapter.wl\"\nevent start\n  -> sub\n".into(),
        ),
        (
            root.join("chapter.wl"),
            "include \"characters.wl\"\nevent sub with a\n  -> END\n".into(),
        ),
        (root.join("characters.wl"), "character a\n".into()),
    ]);
    let result = worldline_core::compile_sources(&entry, &sources);
    assert!(!result.has_errors());
    assert_eq!(result.program.entry, "start");
    sources
        .get_mut(&entry)
        .unwrap()
        .push_str("\ninclude \"characters.wl\"\n");
    assert_eq!(
        worldline_core::compile_sources(&entry, &sources)
            .analysis
            .stats
            .characters,
        1
    );
    sources
        .get_mut(&root.join("characters.wl"))
        .unwrap()
        .push_str("include \"main.wl\"\n");
    assert!(worldline_core::compile_sources(&entry, &sources)
        .diagnostics
        .iter()
        .any(|d| d.code == "A105"));
}

#[test]
fn layout_order_does_not_change_save_fingerprint() {
    let a = compile_source("story.wl", "event start at 10\n  -> END\n");
    let b = compile_source("story.wl", "event start at 50\n  -> END\n");
    assert_eq!(a.analysis.fingerprint, b.analysis.fingerprint);
    let a = compile_source(
        "story.wl",
        "world a\n  property era = \"古代\"\nevent start\n  -> END\n",
    );
    let b = compile_source(
        "story.wl",
        "world a\n  property era = \"现代\"\nevent start\n  -> END\n",
    );
    assert_ne!(a.analysis.fingerprint, b.analysis.fingerprint);
}

// ---------------------------------------------------------------------------

#[test]
fn generated_sources_compile_clean() {
    let dir = ProjectDir::new();
    for (name, source) in [
        ("empty.wl", "event start\n  -> END\n"),
        (
            "choices.wl",
            "let n = 0\nevent start\n  choice once \"增量\" if n == 0\n    set n = n + 1\n  if n == 1\n    数值:{n}\n  else\n    空值\n  -> END\n",
        ),
    ] {
        let path = dir.0.join(name);
        std::fs::write(&path, source).unwrap();
        let result = compile_path(&path).expect("读取测试输入失败");
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{name} 存在错误:{errors:#?}");
    }
}

#[test]
fn included_source_keeps_main_file_entry() {
    let dir = ProjectDir::new();
    let main = dir.0.join("main.wl");
    std::fs::write(&main, "include \"child.wl\"\nevent start\n  -> child\n").unwrap();
    std::fs::write(dir.0.join("child.wl"), "event child\n  -> END\n").unwrap();
    let result = compile_path(&main).unwrap();
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert_eq!(
        result.program.entry, "start",
        "入口必须是主文件的第一个事件"
    );
    assert!(
        result.program.events.iter().any(|e| e.name == "child"),
        "include 的事件应被合并"
    );
}

// -- 关系图 -------------------------------------------------------------------

#[test]
fn graph_edges_and_mermaid() {
    let result = compile_source(
        "g.wl",
        "event start\n  choice \"去A\"\n    -> a\n  -> b\n\nevent a\n  -> END\n\nevent b\n  -> END\n",
    );
    let g = &result.analysis.graph;
    assert_eq!(g.nodes.len(), 3);
    // Divert 两条:选择体内的 -> a,以及组后汇聚路径的 -> b;Choice 一条(带标签)
    let diverts = g
        .edges
        .iter()
        .filter(|e| e.kind == worldline_core::EdgeKind::Divert)
        .count();
    let choices = g
        .edges
        .iter()
        .filter(|e| e.kind == worldline_core::EdgeKind::Choice)
        .count();
    assert_eq!(diverts, 2, "{:?}", g.edges);
    assert_eq!(choices, 1);
    let mermaid = g.to_mermaid();
    assert!(mermaid.contains("flowchart TD"));
    assert!(mermaid.contains(".->"), "{mermaid}");
}

#[test]
fn graph_scene_enter_edge() {
    let result = compile_source("g.wl", "event start\n  scene inner\n    x\n    -> END\n");
    let enters = result
        .analysis
        .graph
        .edges
        .iter()
        .filter(|e| e.kind == worldline_core::EdgeKind::Enter)
        .count();
    assert_eq!(enters, 1);
}

// -- include -----------------------------------------------------------------

#[test]
fn include_cycle_detected() {
    let dir = std::env::temp_dir().join("wl_test_cycle");
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.wl");
    let b = dir.join("b.wl");
    std::fs::write(&a, "include \"b.wl\"\nevent start\n  -> END\n").unwrap();
    std::fs::write(&b, "include \"a.wl\"\n").unwrap();
    let result = compile_path(&a).unwrap();
    assert!(
        result.diagnostics.iter().any(|d| d.code == "A105"),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn include_nested_relative_paths() {
    let dir = std::env::temp_dir().join("wl_test_inc");
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let main = dir.join("main.wl");
    std::fs::write(&main, "include \"sub/one.wl\"\nevent start\n  -> chapter\n").unwrap();
    std::fs::write(sub.join("one.wl"), "include \"two.wl\"\n").unwrap();
    std::fs::write(
        sub.join("two.wl"),
        "event chapter\n  嵌套成功。\n  -> END\n",
    )
    .unwrap();
    let result = compile_path(&main).unwrap();
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert!(result.program.events.iter().any(|e| e.name == "chapter"));
}

// -- v1.5:故事线 / 准入 / 效果 / 锚点 / 漂流 ----------------------------------

#[test]
fn storyline_forest_seq_and_drift_graph() {
    let result = compile_source(
        "t.wl",
        "storyline a as \"甲线\"\n  event start\n    -> x\n  event x\n    -> END\n\nstoryline b as \"乙线\"\n  event b.entry\n    ->> x\n",
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    let g = &result.analysis.graph;
    assert_eq!(
        g.storyline_order,
        vec![
            ("a".to_string(), "甲线".to_string()),
            ("b".to_string(), "乙线".to_string())
        ]
    );
    let start = *g.ids.get("start").unwrap();
    let x = *g.ids.get("x").unwrap();
    assert_eq!(g.nodes[start as usize].seq, 1);
    assert_eq!(g.nodes[x as usize].seq, 2);
    assert_eq!(g.nodes[x as usize].storyline, "a");
    assert_eq!(
        g.edges.iter().filter(|e| e.kind == EdgeKind::Drift).count(),
        1
    );
    assert!(g.to_mermaid().contains("==>"), "{:?}", g.to_mermaid());
    let tl = g.to_timeline_mermaid();
    assert!(tl.contains("subgraph"), "{tl}");
    assert!(tl.contains("漂流"), "{tl}");
}

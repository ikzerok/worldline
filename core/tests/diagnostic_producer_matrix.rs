//! 共享生产路径与 related 的来源完整性。
use std::collections::BTreeMap;
use worldline_core::diagnostic::DiagnosticSourceRole as Role;
use worldline_core::project::Project;
use worldline_core::{
    compile_source_with_options, compile_sources_with_options, CompileOptions, Diagnostic, Span,
};

fn slice(source: &str, span: Span) -> String {
    source
        .split('\n')
        .nth(span.line as usize - 1)
        .unwrap()
        .trim_end_matches('\r')
        .chars()
        .skip(span.column as usize - 1)
        .take(span.length as usize)
        .collect()
}
fn diagnostics(source: &str) -> Vec<Diagnostic> {
    compile_source_with_options(
        "source.wl",
        source,
        CompileOptions::v1_13().with_object_refs(true),
    )
    .diagnostics
}

#[test]
fn after_effect_rules_nested_arguments_and_else_if_use_their_own_expressions() {
    let source = "rule ready(x: num) -> bool = 1 + true\nfragment action(x: num)\n  local copy: num = \"local\"\n  return\nevent start after 12\n  effect on enter if 13\n    grant key\n  if true\n    正文\n  else if 14\n    正文\n  call action(\"argument\")\n  -> END\n";
    let ds = diagnostics(source);
    let actual: Vec<_> = ds
        .iter()
        .filter(|d| d.code == "A103")
        .map(|d| slice(source, d.span))
        .collect();
    assert_eq!(
        actual,
        ["1 + true", "\"local\"", "12", "13", "14", "\"argument\""]
    );
    assert!(ds
        .iter()
        .filter(|d| d.code == "A103")
        .all(|d| d.source_role() == Some(Role::Expression)));
}

#[test]
fn schema_value_and_missing_property_have_real_primary_and_related_headers() {
    let source = "schema city for entity entity_type place closed\n        field count_id count number required\n        field title_id title text required\nbind entity harbor to city\nentity harbor kind place\n        property count = \"错\"\n        property extra = 1\nevent start\n  -> END\n";
    let ds = diagnostics(source);
    let wrong = ds.iter().find(|d| d.code == "SCH005").unwrap();
    assert_eq!(slice(source, wrong.span), "property count = \"错\"");
    assert_eq!(wrong.source_role(), Some(Role::Statement));
    assert_eq!(
        slice(source, wrong.related[0].1),
        "field count_id count number required"
    );
    assert_eq!(wrong.related_source_role(0), Some(Role::Declaration));
    let missing = ds.iter().find(|d| d.code == "SCH004").unwrap();
    assert_eq!(slice(source, missing.span), "entity harbor kind place");
    assert_eq!(missing.source_role(), Some(Role::Declaration));
    assert_eq!(
        slice(source, missing.related[0].1),
        "field title_id title text required"
    );
    let extra = ds.iter().find(|d| d.code == "SCH008").unwrap();
    assert_eq!(slice(source, extra.span), "property extra = 1");
}

#[test]
fn duplicate_same_basename_and_include_keep_correct_file_and_target_roles() {
    let root = std::env::temp_dir().join(format!("wl-diagnostic-sources-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    // 使用 core 的源码身份，展开 Windows 8.3 别名并去除设备路径前缀。
    let root = Project::new(&root).root;
    let main = root.join("world.wl");
    let first = root.join("a").join("actors.wl");
    let second = root.join("b").join("actors.wl");
    let sources = BTreeMap::from([
        (
            main.clone(),
            "include \"a/actors.wl\"\ninclude \"b/actors.wl\"\nevent start\n  -> END\n".into(),
        ),
        (first.clone(), "let tide = 1\n".into()),
        (second.clone(), "let tide = 2\n".into()),
    ]);
    let result = compile_sources_with_options(&main, &sources, CompileOptions::v1_13());
    let duplicate = result
        .diagnostics
        .iter()
        .find(|d| d.code == "A104")
        .unwrap();
    assert_eq!(duplicate.file, second.to_string_lossy());
    assert_eq!(duplicate.related[0].0, first.to_string_lossy());
    assert_eq!(slice(&sources[&second], duplicate.span), "tide");
    assert_eq!(slice(&sources[&first], duplicate.related[0].1), "tide");
    assert_eq!(duplicate.source_role(), Some(Role::Target));
    assert_eq!(duplicate.related_source_role(0), Some(Role::Target));
    let unread = result
        .diagnostics
        .iter()
        .find(|d| d.code == "A107")
        .unwrap();
    assert_eq!(unread.source_role(), Some(Role::Target));
    assert_eq!(slice(&sources[&first], unread.span), "tide");
}

#[test]
fn missing_block_does_not_steal_the_next_declarations_source() {
    let source = "event empty\nevent following\n  -> END\n";
    let ds = diagnostics(source);
    let missing = ds.iter().find(|d| d.code == "P005").unwrap();
    assert_eq!(slice(source, missing.span), "event empty");
    assert_eq!(missing.source_role(), Some(Role::Declaration));
    let source = "event eof";
    let ds = diagnostics(source);
    let missing = ds.iter().find(|d| d.code == "P005").unwrap();
    assert_eq!(slice(source, missing.span), "event eof");
}

#[test]
fn retained_compile_diagnostics_all_publish_an_audited_role() {
    let source = "world one\nworld two\ncharacter hero\n  property count = 1\n  property count = 2\nentity place kind place\n  property contact = ref(\"entity\", \"missing\")\nperiod first within missing\nrule recursion() -> bool = recursion()\nevent start with nobody follows vanished\n  meet nobody\n  effect on enter\n    to missing\n  choice once \"same\" if true\n    -> missing\n  choice \"same\" if false\n    -> END\n";
    let ds = diagnostics(source);
    assert!(!ds.is_empty());
    for diagnostic in &ds {
        assert!(diagnostic.source_role().is_some(), "{diagnostic:?}");
        if matches!(
            diagnostic.source_role(),
            Some(Role::Target | Role::Expression | Role::Statement | Role::Declaration)
        ) {
            assert!(!slice(source, diagnostic.span).is_empty(), "{diagnostic:?}");
        }
        for index in 0..diagnostic.related.len() {
            assert!(
                diagnostic.related_source_role(index).is_some(),
                "{diagnostic:?}"
            );
        }
    }
}

#[test]
fn explicit_execution_hint_reuses_the_same_declaration_source() {
    let source = "period history\nevent record during history\n  历史正文\n";
    let result = compile_source_with_options("history.wl", source, CompileOptions::v1_13());
    assert!(!result.diagnostics.iter().any(|d| d.code == "A202"));
    let hints = worldline_core::analysis::execution_diagnostics(&result.program, &result.analysis);
    let hint = hints.iter().find(|d| d.code == "A202").unwrap();
    assert_eq!(hint.source_role(), Some(Role::Declaration));
    assert_eq!(slice(source, hint.span), "event record during history");
}

#[test]
fn failed_include_points_to_the_supplied_path_not_the_missing_document() {
    let root =
        std::env::temp_dir().join(format!("wl-missing-include-source-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let root = Project::new(&root).root;
    let main = root.join("world.wl");
    let source = "include    \"missing.wl\"\nevent start\n  -> END\n";
    let result = compile_sources_with_options(
        &main,
        &BTreeMap::from([(main.clone(), source.into())]),
        CompileOptions::v1_13(),
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|d| d.code == "A105")
        .unwrap();
    assert_eq!(diagnostic.file, main.to_string_lossy());
    assert_eq!(diagnostic.source_role(), Some(Role::Target));
    assert_eq!(slice(source, diagnostic.span), "\"missing.wl\"");
}

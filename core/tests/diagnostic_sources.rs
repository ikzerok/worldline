//! 原文 golden 来源；不能以合法 bounds 或诊断 message 推导预期范围。
use worldline_core::diagnostic::DiagnosticSourceRole as Role;
use worldline_core::{compile_source_with_options, CompileOptions, Diagnostic, LanguageVersion};

fn selected(source: &str, diagnostic: &Diagnostic) -> String {
    source
        .split('\n')
        .nth(diagnostic.span.line as usize - 1)
        .unwrap()
        .trim_end_matches('\r')
        .chars()
        .skip(diagnostic.span.column as usize - 1)
        .take(diagnostic.span.length as usize)
        .collect()
}

fn expect(source: &str, code: &str, expected: &str, role: Role) {
    let result = compile_source_with_options(
        "story.wl",
        source,
        CompileOptions::v1_13()
            .with_object_refs(true)
            .with_character_refs(true),
    );
    let matches: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == code)
        .collect();
    assert_eq!(matches.len(), 1, "{source}\n{:#?}", result.diagnostics);
    assert_eq!(
        selected(source, matches[0]),
        expected,
        "{source}\n{:#?}",
        matches[0]
    );
    assert_eq!(matches[0].source_role(), Some(role));
}

#[test]
fn fresh_fault_families_point_to_real_syntax() {
    for indent in ["", "  ", "        "] {
        // Top-level diverts also produce P002; A101 belongs to an actual event body.
        if !indent.is_empty() {
            expect(
                &format!("event arrival\n{indent}-> missing\n"),
                "A101",
                "missing",
                Role::Target,
            );
            expect(
                &format!("event arrival\n{indent}-> z\n"),
                "A101",
                "z",
                Role::Target,
            );
            expect(
                &format!("event arrival\n{indent}set missing = true\n{indent}-> END\n"),
                "A102",
                "missing",
                Role::Target,
            );
        }
        expect(
            &format!("{indent}let count = 1 + \"潮汐\"\nevent arrival\n  {{count}}\n  -> END\n"),
            "A103",
            "1 + \"潮汐\"",
            Role::Expression,
        );
    }
    expect(
        "event arrival\n  青禾说：🔔 {missing}\n  -> END\n",
        "A102",
        "missing",
        Role::Target,
    );
    expect(
        "event arrival\n  if 1\n    继续\n  -> END\n",
        "A103",
        "1",
        Role::Expression,
    );
    expect(
        "event arrival\n  choice \"敲响🔔旧钟\" if 1\n    -> END\n",
        "A103",
        "1",
        Role::Expression,
    );
    expect(
        "event arrival\n  choice \"敲响🔔旧钟\" if missing\n    -> END\n",
        "A102",
        "missing",
        Role::Target,
    );
    expect(
        "event arrival with missing\n  -> END\n",
        "A208",
        "event arrival with missing",
        Role::Declaration,
    );
    expect(
        "event arrival\n  {visits(missing)}\n  -> END\n",
        "A101",
        "visits(missing)",
        Role::Expression,
    );
    expect(
        "event arrival\n  call missing()\n  -> END\n",
        "A103",
        "call missing()",
        Role::Statement,
    );
    expect(
        "event arrival\r\n        -> missing\r\n",
        "A101",
        "missing",
        Role::Target,
    );
    expect(
        "event arrival\n  \\choice 开头只是正文 {missing}\n  -> END\n",
        "A102",
        "missing",
        Role::Target,
    );
    expect(
        "let missing = 1\nevent arrival\n  choice \"missing 不是条件\" if unknown\n    -> END\n",
        "A102",
        "unknown",
        Role::Target,
    );
    expect("entity old_city kind place\n  property guardian = ref(\"character\", \"missing\")\nevent arrival\n  -> END\n",
        "A214", "property guardian = ref(\"character\", \"missing\")", Role::Statement);
    expect(
        "event arrival\n  青禾说：🔔 [[entity:missing|旧城]]\n  -> END\n",
        "A218",
        "[[entity:missing|旧城]]",
        Role::Target,
    );
}

#[test]
fn expressions_have_distinct_structural_owners_even_when_identical() {
    let source = "event arrival\n  {1 + true}，{1 + true}，{rnd(1, \"错\")}\n  choice \"{2 + false}\" if 3 enable 4 disabled \"原因\"\n    -> END\n";
    let result = compile_source_with_options("story.wl", source, CompileOptions::v1_13());
    let diagnostics: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "A103")
        .collect();
    let selections: Vec<_> = diagnostics.iter().map(|d| selected(source, d)).collect();
    assert_eq!(
        selections,
        ["1 + true", "1 + true", "\"错\"", "2 + false", "3", "4"]
    );
    assert!(diagnostics
        .windows(2)
        .all(|pair| (pair[0].span.line, pair[0].span.column)
            < (pair[1].span.line, pair[1].span.column)));
    assert!(diagnostics
        .iter()
        .all(|d| d.source_role() == Some(Role::Expression)));
}

#[test]
fn quoted_mapping_preserves_both_endpoints_and_scalar_columns() {
    expect("character narrator\nevent arrival\n  say narrator \"前\\\"缀🔔 {1 + \\\"潮\\\\汐\\\"} 后\"\n  -> END\n",
        "A103", "1 + \\\"潮\\\\汐\\\"", Role::Expression);
    expect(
        "event arrival\n  choice \"前\\\"缀 {missing}\" if true\n    -> END\n",
        "A102",
        "missing",
        Role::Target,
    );
}

#[test]
fn supported_versions_share_physical_and_fragment_coordinate_contracts() {
    for version in [
        LanguageVersion::V1_9,
        LanguageVersion::V1_10,
        LanguageVersion::V1_11,
        LanguageVersion::V1_12,
        LanguageVersion::V1_13,
    ] {
        let source = "event start\r\n        e\u{301}👩‍👩‍👧‍👦 {missing}\r\n        -> END\r\n";
        let result = compile_source_with_options("story.wl", source, CompileOptions::new(version));
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|d| d.code == "A102")
            .unwrap();
        assert_eq!(selected(source, diagnostic), "missing");
    }
    let expression =
        worldline_core::expression::parse_expr_src("missing", "fragment.wl", 1, 0, &mut Vec::new());
    assert!(matches!(expression, worldline_core::ast::Expr::Var { loc, .. } if loc.column == 1));
}

#[test]
fn diagnostic_wire_shape_remains_eight_fields() {
    let result = compile_source_with_options(
        "story.wl",
        "event start\n  -> missing\n",
        CompileOptions::v1_13(),
    );
    for diagnostic in &result.diagnostics {
        let object = serde_json::to_value(diagnostic).unwrap();
        let keys: Vec<_> = object
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "code",
                "file",
                "message",
                "note",
                "related",
                "severity",
                "span",
                "suggestion"
            ]
        );
    }
}

#[test]
fn parenthesized_names_keep_target_boundaries_and_composites_keep_expression_boundaries() {
    for expression in ["(missing)", "((missing))"] {
        expect(
            &format!("event start\n  {{{expression}}}\n  -> END\n"),
            "A102",
            "missing",
            Role::Target,
        );
    }
    expect(
        "event start\n  {((1 + true))}\n  -> END\n",
        "A103",
        "((1 + true))",
        Role::Expression,
    );
}

#[test]
fn missing_expression_uses_real_owner_context_without_changing_recovery_facts() {
    let source = "event start\n  if\n    正文\n  -> END\n";
    let result = compile_source_with_options("story.wl", source, CompileOptions::v1_13());
    let syntax = result
        .diagnostics
        .iter()
        .find(|d| d.code == "P006")
        .unwrap();
    assert_eq!(syntax.source_role(), Some(Role::Statement));
    assert_eq!(selected(source, syntax), "if");
    let codes: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.span.line == 2)
        .map(|d| d.code)
        .collect();
    assert_eq!(codes, ["A103", "P004", "P006"]);
    let mut standalone = Vec::new();
    worldline_core::expression::parse_expr_src("", "fragment.wl", 1, 0, &mut standalone);
    assert_eq!(standalone.len(), 1);
    assert_eq!(standalone[0].source_role(), None);
}

#[test]
fn type_mismatch_on_parenthesized_known_variable_is_the_complete_expression() {
    expect(
        "let n = 1\nevent start\n  if (n)\n    -> END\n",
        "A103",
        "(n)",
        Role::Expression,
    );
}

#[test]
fn drift_target_range_excludes_arrow_spacing_crlf_and_comment() {
    for newline in ["\n", "\r\n"] {
        for suffix in ["", "    ", " // finish不是另一个目标"] {
            let source = format!("event start{newline}  ->> finish{suffix}{newline}event finish{newline}  -> END{newline}");
            expect(&source, "A209", "finish", Role::Target);
        }
    }
}

#[test]
fn missing_assignment_and_divert_targets_keep_recovery_facts_as_context() {
    expect(
        "event start\n  set\n  -> END\n",
        "A102",
        "set",
        Role::Statement,
    );
    expect("event start\n  ->\n", "A101", "->", Role::Statement);
}

#[test]
fn escaped_choice_links_are_mapped_once_for_both_diagnostics_and_wiki() {
    let source = r#"character hero as "旅人"
event start
  choice "前\"缀🔔 [[character:hero|那位\"人]] 末尾"
    -> END
"#;
    let result = compile_source_with_options("story.wl", source, CompileOptions::v1_13());
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let index = worldline_core::wiki::KeywordIndex::new(&result);
    let hits: Vec<_> = index
        .occurrences(&worldline_core::TargetRef::new("character", "hero"))
        .iter()
        .filter(|hit| hit.line == 3)
        .collect();
    assert_eq!(hits.len(), 1);
    let link = result
        .analysis
        .catalog
        .text_links
        .iter()
        .find(|link| link.line == 3)
        .unwrap();
    assert_eq!(hits[0].column, link.column);
    assert_eq!(
        hits[0].preview.chars().nth(hits[0].column as usize - 1),
        Some('[')
    );
    let invalid = source.replace("character:hero", "character:missing");
    expect(
        &invalid,
        "A218",
        r#"[[character:missing|那位\"人]]"#,
        Role::Target,
    );
}

use worldline_core::{authoring::EventDraft, compile_source_with_options, CompileOptions};

#[test]
fn new_syntax_is_explicit_112_and_enable_must_be_boolean() {
    let source = "event start\n  choice \"门\" enable false disabled \"没钥匙\"\n    -> END\n";
    for options in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
    ] {
        assert!(compile_source_with_options("x.wl", source, options).has_errors());
    }
    assert!(!compile_source_with_options("x.wl", source, CompileOptions::v1_12()).has_errors());
    for tail in [
        "enable false",
        "disabled \"原因\"",
        "enable 3 disabled \"原因\"",
        "enable true disabled \"\"",
        "enable true disabled \"说明\" if false",
    ] {
        let source = format!("event start\n  choice \"门\" {tail}\n    -> END\n");
        assert!(
            compile_source_with_options("x.wl", &source, CompileOptions::v1_12()).has_errors(),
            "{tail}"
        );
    }
}

#[test]
fn quoted_keywords_do_not_split_expressions_and_form_preserves_both_conditions() {
    let source = "let gate = true\nevent start\n  choice once \"门\" if \"enable disabled\" == \"enable disabled\" enable gate disabled \"条件不满足 {secret} //普通文字\" //保留\n    -> END\n";
    let c = compile_source_with_options("x.wl", source, CompileOptions::v1_12());
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    let mut event = EventDraft {
        body: source
            .lines()
            .skip(2)
            .map(|line| line.strip_prefix("  ").unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n"),
        ..Default::default()
    };
    let choice = event.choices().remove(0);
    assert_eq!(choice.enable_condition, "gate");
    assert_eq!(choice.disabled_reason, "条件不满足 {secret} //普通文字");
    assert!(choice.condition.contains("enable disabled"));
    event.write_choice(Some(choice.line), &choice).unwrap();
    assert_eq!(event.choices()[0], choice);
    assert!(event.body.contains("//保留"));
}

#[test]
fn enable_expression_references_join_catalog_and_deletion_impact() {
    let source = "event target\n  -> END\nevent start\n  choice \"门\" enable seen(target) disabled \"尚未到过\"\n    -> END\n";
    let c = compile_source_with_options("x.wl", source, CompileOptions::v1_12());
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    let refs = worldline_core::deletion_content_references::content_deletion_references(
        &c,
        &worldline_core::TargetRef::new("event", "target"),
    );
    assert!(refs.iter().any(|r| r.source.id == "start"));
}

#[test]
fn clause_words_remain_valid_variable_names_inside_choice_conditions() {
    for tail in [
        "if enable and true enable disabled disabled \"说明\"",
        "if true and enable enable true and disabled disabled \"说明\"",
    ] {
        let source = format!("let enable = true\nlet disabled = false\nevent start\n  choice \"门\" {tail}\n    -> END\n");
        let c = compile_source_with_options("x.wl", &source, CompileOptions::v1_12());
        assert!(!c.has_errors(), "{tail}: {:?}", c.diagnostics);
    }
}

#[test]
fn reading_projection_retains_enable_and_plain_author_explanation() {
    let source = r#"  choice "问[[character:hero|英雄]]" if true enable false disabled "{secret} [[event:hidden|仅文字]]""#;
    let lines = worldline_core::navigation::reading_lines_with_options(
        source,
        "x.wl",
        CompileOptions::v1_12(),
    );
    let text = lines[0].iter().map(|p| p.text.as_str()).collect::<String>();
    assert!(text.contains("if true enable false disabled \"{secret} [[event:hidden|仅文字]]\""));
    let targets: Vec<_> = lines[0].iter().filter_map(|p| p.target.as_ref()).collect();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].id, "hero");
}

#[test]
fn disabled_literal_span_retains_unicode_escapes_and_excludes_quotes_comments() {
    let raw = r#"  choice "门🚪" if true enable false disabled "条件 {secret} [[event:hidden|文字]] \"引号\" \\斜杠" // disabled "注释""#;
    let mut diagnostics = Vec::new();
    let lines = worldline_core::lexer::lex_source_with_options(
        "x.wl",
        raw,
        &mut diagnostics,
        CompileOptions::v1_12(),
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let line = &lines[0];
    let worldline_core::lexer::LineKind::Choice {
        disabled_span: Some(span),
        ..
    } = &line.kind
    else {
        panic!("missing disabled span")
    };
    let actual: String = raw
        .chars()
        .skip((line.indent + span.column - 1) as usize)
        .take(span.length as usize)
        .collect();
    assert_eq!(
        actual,
        r#"条件 {secret} [[event:hidden|文字]] \"引号\" \\斜杠"#
    );
}

#[test]
fn permission_migration_rewrites_enable_but_never_the_same_text_in_explanation() {
    let root = std::env::temp_dir().join(format!(
        "choice-permission-migration-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.12","required_features":[]}"#,
    )
    .unwrap();
    let source="event start\n  grant key\n  choice \"门\" if perm(key) enable perm(key) disabled \"perm(key) {perm(key)}\"\n    -> END\n";
    std::fs::write(root.join("world.wl"), source).unwrap();
    let mut project = worldline_core::project::Project::open(&root).unwrap();
    // open 已按既有契约在缓冲中迁移；磁盘原文不变。
    let opened = project.document(&root.join("world.wl")).unwrap();
    assert!(opened.contains(" enable has("));
    assert!(opened.contains("disabled \"perm(key) {perm(key)}\""));
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        source
    );
    project
        .set_text(&root.join("world.wl"), source.into())
        .unwrap();
    let before = project.compile();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    assert_eq!(project.migrate_permissions().unwrap(), 1);
    let text = project.document(&root.join("world.wl")).unwrap();
    assert!(text.contains("disabled \"perm(key) {perm(key)}\""));
    assert!(text.contains(" if has("));
    assert!(text.contains(" enable has("));
    let after = project.compile();
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    assert_eq!(project.migrate_permissions().unwrap(), 0);
    std::fs::remove_dir_all(root).unwrap();
}

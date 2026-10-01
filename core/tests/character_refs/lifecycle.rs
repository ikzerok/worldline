use super::*;
use worldline_core::ast::PropertyValue;
use worldline_core::source_edit::SourceEditRequest;

#[test]
fn cross_file_rename_preview_cancel_apply_undo_and_save_keep_strong_identity() {
    let mut f = fixture("rename");
    let target = TargetRef::new("character", "lin");
    let before = f.project.clone();
    let baseline = f.project.content_baseline();
    let result = f.project.compile();
    let fingerprint = result.analysis.fingerprint;
    let refs = result.analysis.catalog.references_to(&target);
    assert_eq!(
        refs.iter()
            .filter(|reference| reference.kind == "对象属性引用")
            .count(),
        3
    );
    assert!(refs
        .iter()
        .any(|reference| reference.file.ends_with("world.wl")));
    let impact = f.project.deletion_impact(&target);
    assert!(impact.complete, "{:?}", impact.diagnostics);
    assert!(!impact.can_delete());
    assert!(impact
        .content_references
        .iter()
        .any(|reference| reference.kind == "对象属性引用"));
    let cancelled = f.project.plan_rename_target(&target, "navigator").unwrap();
    assert_eq!(cancelled.changes.len(), 2);
    assert_eq!(f.project.content_baseline(), baseline);
    drop(cancelled);
    assert_eq!(f.project.content_baseline(), baseline);
    let plan = f.project.plan_rename_target(&target, "navigator").unwrap();
    f.project.apply_rename_plan(&plan).unwrap();
    let renamed = f.project.compile();
    assert!(!renamed.has_errors(), "{:?}", renamed.diagnostics);
    assert_ne!(fingerprint, renamed.analysis.fingerprint);
    assert!(renamed.analysis.catalog.references_to(&target).is_empty());
    let source = f.project.document(&f.root.join("world.wl")).unwrap();
    for expected in [
        "property captain = ref(\"character\", \"navigator\") // ref(\"character\", \"lin\") 注释不变",
        "property plain = \"lin\"", "to character navigator", "with navigator",
        "[[character:navigator|lin]] 与普通 lin 文字", "say navigator", "alias character navigator as \"lin\"",
    ] { assert!(source.contains(expected), "missing {expected}: {source}"); }
    assert!(f
        .project
        .document(&f.root.join("people.wl"))
        .unwrap()
        .contains("self = ref(\"character\", \"navigator\")"));
    let after = f.project.clone();
    assert!(f.project.restore(before));
    assert_eq!(f.project.content_baseline(), baseline);
    assert!(f.project.restore(after));
    f.project.save().unwrap();
    let mut reopened = Project::open(&f.root).unwrap();
    assert!(!reopened.compile().has_errors());
    assert_eq!(
        reopened.compile().analysis.catalog.entities["boat"].properties["captain"],
        PropertyValue::Ref(TargetRef::new("character", "navigator"))
    );
}

#[test]
fn stale_and_disk_conflicting_rename_plans_never_apply_partially() {
    let mut f = fixture("stale");
    let target = TargetRef::new("character", "lin");
    let plan = f.project.plan_rename_target(&target, "navigator").unwrap();
    let path = f.root.join("world.wl");
    f.project
        .set_text(&path, format!("{FACTS}// author edit\n"))
        .unwrap();
    let stale_baseline = f.project.content_baseline();
    assert!(f.project.apply_rename_plan(&plan).is_err());
    assert_eq!(f.project.content_baseline(), stale_baseline);
    let plan = f.project.plan_rename_target(&target, "navigator").unwrap();
    fs::write(
        f.root.join("people.wl"),
        format!("{PEOPLE}// outside edit\n"),
    )
    .unwrap();
    assert!(f.project.apply_rename_plan(&plan).is_err());
    assert_eq!(f.project.content_baseline(), stale_baseline);
    assert!(f
        .project
        .document(&f.root.join("people.wl"))
        .unwrap()
        .contains("character lin"));
}

#[test]
fn source_property_preview_cancel_apply_undo_preserves_runtime_fingerprint() {
    let mut f = fixture("source");
    let before = f.project.clone();
    let fingerprint = f.project.compile().analysis.fingerprint;
    let request = SourceEditRequest {
        schema_version: 1,
        path: PathBuf::from("world.wl"),
        expected_baseline: f.project.content_baseline(),
        source: FACTS.replace(
            "captain = ref(\"character\", \"lin\")",
            "captain = ref(\"character\", \"mei\")",
        ),
    };
    let preview = f.project.preview_source_edit(&request).unwrap();
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
    assert_eq!(f.project.content_baseline(), request.expected_baseline);
    f.project
        .apply_source_edit(&request, &preview.plan_digest)
        .unwrap();
    assert_eq!(f.project.compile().analysis.fingerprint, fingerprint);
    assert!(f.project.restore(before));
    assert_eq!(f.project.content_baseline(), request.expected_baseline);
}

#[test]
fn character_form_rename_preserves_self_ref_without_promoting_plain_string() {
    let mut f = fixture("form");
    let draft = worldline_core::authoring::CharacterDraft {
        id: "navigator".into(),
        display: "林舟".into(),
        properties: vec![
            (
                "self".into(),
                PropertyValue::Ref(TargetRef::new("character", "lin")),
            ),
            ("plain".into(), PropertyValue::Str("lin".into())),
        ],
        ..Default::default()
    };
    f.project
        .write_character(&f.root.join("people.wl"), Some("lin"), &draft)
        .unwrap();
    let result = f.project.compile();
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let source = f.project.document(&f.root.join("people.wl")).unwrap();
    assert!(source.contains("self = ref(\"character\", \"navigator\")"));
    assert!(source.contains("plain = \"lin\""));
}

#[test]
fn deleting_a_still_referenced_character_reports_missing_typed_target() {
    let mut f = fixture("delete");
    f.project
        .set_text(
            &f.root.join("people.wl"),
            "character mei as \"梅\"\n".into(),
        )
        .unwrap();
    let result = f.project.compile();
    assert!(codes(&result).contains(&"A214"));
    assert!(
        !f.project
            .deletion_impact(&TargetRef::new("character", "lin"))
            .complete
    );
}

#[test]
fn property_rename_preserves_inline_block_and_unicode_comments() {
    let mut f = fixture("comments");
    let source = FACTS.replace(
        "captain = ref(\"character\", \"lin\")",
        "captain = /* 中文 ref(\"character\", \"lin\") */ ref(\"character\", /*备注*/ \"lin\")",
    );
    f.project
        .set_text(&f.root.join("world.wl"), source)
        .unwrap();
    let plan = f
        .project
        .plan_rename_target(&TargetRef::new("character", "lin"), "navigator")
        .unwrap();
    f.project.apply_rename_plan(&plan).unwrap();
    let source = f.project.document(&f.root.join("world.wl")).unwrap();
    assert!(source.contains(
        "/* 中文 ref(\"character\", \"lin\") */ ref(\"character\", /*备注*/ \"navigator\")"
    ));
}

#[test]
fn property_rename_uses_lexer_for_tabs_and_leading_block_comments() {
    for (name, prefix) in [
        ("tab", "  property\tcaptain"),
        ("keyword-comment", "  property/*注释*/ captain"),
    ] {
        let mut f = fixture(name);
        let source = FACTS.replace("  property captain", prefix);
        f.project
            .set_text(&f.root.join("world.wl"), source)
            .unwrap();
        let compiled = f.project.compile();
        assert!(!compiled.has_errors(), "{name}: {:?}", compiled.diagnostics);
        let plan = f
            .project
            .plan_rename_target(&TargetRef::new("character", "lin"), "navigator")
            .unwrap();
        f.project.apply_rename_plan(&plan).unwrap();
        let compiled = f.project.compile();
        assert!(!compiled.has_errors(), "{name}: {:?}", compiled.diagnostics);
        let source = f.project.document(&f.root.join("world.wl")).unwrap();
        assert!(source.contains(&format!(
            "{prefix} = ref(\"character\", \"navigator\") // ref(\"character\", \"lin\") 注释不变"
        )));
        assert!(source.contains("property plain = \"lin\""));
    }
}

#[test]
fn property_rename_preserves_parenthesized_arguments_multiline_comments_and_whitespace() {
    for (name, property) in [
        ("outer-parens", "  property captain = (ref(\"character\", \"lin\"))"),
        ("arg-parens", "  property captain = ref((\"character\"), ((\"lin\")))"),
        ("leading-block", "  /*前置注释*/ property captain = ref(\"character\", \"lin\")"),
        ("multiline-block", "  /*跨行注释\n  */ property captain = ref(\"character\", \"lin\")"),
        ("value-whitespace", "  property\tcaptain\t=\t ( ref(\"character\", (\"lin\")) ) \t// ref(\"character\", \"lin\")"),
    ] {
        let mut f = fixture(name);
        let source = format!("entity boat kind ship\n{property}\nevent start\n  -> END\n");
        f.project.set_text(&f.root.join("world.wl"), source).unwrap();
        let before = f.project.compile();
        assert!(!before.has_errors(), "{name}: {:?}", before.diagnostics);
        let plan = f.project.plan_rename_target(&TargetRef::new("character", "lin"), "navigator").unwrap();
        f.project.apply_rename_plan(&plan).unwrap();
        let after = f.project.compile();
        assert!(!after.has_errors(), "{name}: {:?}", after.diagnostics);
        let source = f.project.document(&f.root.join("world.wl")).unwrap();
        assert!(source.contains(&property.replacen("\"lin\"", "\"navigator\"", 1)), "{name}: {source}");
    }
}

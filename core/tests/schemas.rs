use worldline_core::{compile_source_with_options, CompileOptions, CompileResult, LanguageVersion};

const SOURCE: &str = r#"schema city for entity entity_type place closed
  field population_id population number required
  field founding_id founding_year number required
  field mayor_id mayor ref entity entity_type organization
  field category_id category enum "city" "town"
  field flag_id public boolean
  field note_id note text
bind entity harbor to city
entity harbor kind place as "港城"
  property population = 0
  property founding_year = 1200
  property mayor = ref("entity", "council")
  property category = "city"
  property public = false
  property note = ""
entity council kind organization as "议会"
entity bell kind artifact as "钟"
event start
  你好。
  -> END
"#;
fn compile(source: &str) -> CompileResult {
    compile_source_with_options(
        "world.wl",
        source,
        CompileOptions::v1_12().with_object_refs(true),
    )
}
fn codes(result: &CompileResult) -> Vec<&str> {
    result.diagnostics.iter().map(|d| d.code).collect()
}

#[test]
fn explicit_version_and_old_defaults_remain_separate() {
    assert_eq!(
        CompileOptions::default().language_version,
        LanguageVersion::V1_9
    );
    assert_eq!(CompileOptions::v1_12().language_version.as_str(), "1.12");
    assert!(LanguageVersion::V1_12.supports_language_111());
    for options in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
    ] {
        let old = compile_source_with_options("world.wl", SOURCE, options.with_object_refs(true));
        assert!(old.has_errors());
        assert!(old.program.schemas.is_empty());
        let prose = compile_source_with_options(
            "world.wl",
            "event start\n  schema 普通旧正文\n  field 普通旧正文\n  bind 普通旧正文\n  -> END\n",
            options,
        );
        assert!(!prose.has_errors(), "{:?}", prose.diagnostics);
    }
}

#[test]
fn zero_false_and_empty_string_are_present_and_valid() {
    let source = SOURCE
        .replace("boolean\n", "boolean required\n")
        .replace("note text\n", "note text required\n");
    let result = compile(&source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.program.schemas[0].fields[0].id, "population_id");
    assert_eq!(result.program.schema_bindings[0].target.id, "harbor");
}

#[test]
fn missing_wrong_scalar_enum_and_closed_typo_are_distinct() {
    let source = SOURCE
        .replace("  property founding_year = 1200\n", "")
        .replace("population = 0", "population = \"很多\"")
        .replace("category = \"city\"", "category = \"village\"")
        .replace("public = false", "public = 0")
        .replace(
            "  property note = \"\"",
            "  property note = \"\"\n  property populaton = 8",
        );
    let result = compile(&source);
    for code in ["SCH004", "SCH005", "SCH006", "SCH008"] {
        assert!(
            codes(&result).contains(&code),
            "{code}: {:?}",
            result.diagnostics
        );
    }
    assert!(result
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with("SCH"))
        .all(|d| d.severity == worldline_core::Severity::Error && !d.related.is_empty()));
    assert_eq!(
        result.program.entities[0]
            .properties
            .iter()
            .find(|p| p.name == "population")
            .unwrap()
            .value,
        worldline_core::ast::PropertyValue::Str("很多".into())
    );
}

#[test]
fn required_reports_missing_at_instance_and_value_errors_at_property() {
    let result = compile(
        &SOURCE
            .replace("  property founding_year = 1200\n", "")
            .replace("population = 0", "population = \"wrong\""),
    );
    let missing = result
        .diagnostics
        .iter()
        .find(|d| d.code == "SCH004")
        .unwrap();
    let mismatch = result
        .diagnostics
        .iter()
        .find(|d| d.code == "SCH005")
        .unwrap();
    assert_eq!(missing.span.line, 9);
    assert_eq!(mismatch.span.line, 10);
    assert_eq!(missing.related[0].1.line, 3);
}

#[test]
fn open_schema_keeps_extra_keys_and_unbound_objects_unconstrained() {
    let source = SOURCE
        .replace("place closed", "place")
        .replace(
            "  property note = \"\"",
            "  property note = \"\"\n  property extra = 7",
        )
        .replace(
            "entity bell kind artifact as \"钟\"",
            "entity bell kind artifact as \"钟\"\n  property population = \"不受约束\"",
        );
    assert!(!compile(&source).has_errors());
    let unbound = SOURCE
        .replace("bind entity harbor to city\n", "")
        .replace("population = 0", "population = \"很多\"");
    assert!(!compile(&unbound).has_errors());
}

#[test]
fn strong_references_check_kind_subtype_and_existing_target() {
    let subtype =
        compile(&SOURCE.replace("ref(\"entity\", \"council\")", "ref(\"entity\", \"bell\")"));
    assert!(codes(&subtype).contains(&"SCH007"));
    let wrong_kind =
        compile(&SOURCE.replace("ref entity entity_type organization", "ref relation"));
    assert!(codes(&wrong_kind).contains(&"SCH007"));
    let missing = compile(&SOURCE.replace(
        "ref(\"entity\", \"council\")",
        "ref(\"entity\", \"missing\")",
    ));
    assert!(codes(&missing).contains(&"A214"));
    let plain = compile(&SOURCE.replace("ref(\"entity\", \"council\")", "\"council\""));
    assert!(codes(&plain).contains(&"SCH005"));
    let expanded = compile(&SOURCE.replace("ref entity entity_type organization", "ref character"));
    assert!(codes(&expanded).contains(&"SCH001"));
    let disabled = compile_source_with_options("world.wl", SOURCE, CompileOptions::v1_12());
    assert!(codes(&disabled).contains(&"P004"));
}

#[test]
fn duplicate_schema_field_identity_key_and_binding_are_errors() {
    for addition in [
        "schema city for entity\n",
        "bind entity harbor to city\n",
        "bind entity harbor to missing\n",
        "bind entity absent to city\n",
        "bind entity bell to city\n",
    ] {
        assert!(
            compile(&format!("{SOURCE}\n{addition}")).has_errors(),
            "{addition}"
        );
    }
    for duplicate in [
        "field population_id different number",
        "field different population number",
    ] {
        let source = SOURCE.replace(
            "  field founding_id",
            &format!("  {duplicate}\n  field founding_id"),
        );
        assert!(codes(&compile(&source)).contains(&"SCH002"));
    }
}

#[test]
fn malformed_schema_never_silently_drops_extra_constraints() {
    for bad in [
        "schema city for character entity_type place",
        "schema city for entity closed trailing",
        "schema city for entity entity_type",
        "schema city for event",
    ] {
        assert!(codes(&compile(&SOURCE.replacen(
            SOURCE.lines().next().unwrap(),
            bad,
            1
        )))
        .contains(&"SCH001"));
    }
    for bad in [
        "field population_id population number unknown",
        "field population_id population enum",
        "field population_id population enum \"x\" \"x\"",
        "field population_id population ref relation entity_type place",
        "field population_id population ref entity entity_type place extra",
        "field \"quoted\" population number",
    ] {
        assert!(
            codes(&compile(&SOURCE.replace(
                "field population_id population number required",
                bad
            )))
            .contains(&"SCH001"),
            "{bad}"
        );
    }
}

#[test]
fn world_character_and_relation_property_sets_share_validator() {
    let source = r#"schema facts for world closed
  field year_id year number required
schema person for character
  field note_id note text required
schema ties for relation
  field known_id known boolean required
world atlas
  property year = 0
character mira
  property note = ""
entity a kind place
entity b kind place
relation_type near
relation_def neighbor type near from entity a to entity b
  property known = false
bind world atlas to facts
bind character mira to person
bind relation neighbor to ties
event start
  -> END
"#;
    let result = compile(source);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(
        worldline_core::schemas::validate(&result.program, &result.analysis.catalog)
            .instances
            .len(),
        3
    );
    let bad = compile(&source.replace("known = false", "known = \"false\""));
    assert!(codes(&bad).contains(&"SCH005"));
}

#[test]
fn static_schema_edits_do_not_change_runtime_fingerprint() {
    let before = compile(SOURCE);
    let without = SOURCE
        .lines()
        .filter(|line| {
            !line.starts_with("schema ")
                && !line.starts_with("  field ")
                && !line.starts_with("bind ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let after = compile(&without);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    let changed = compile(
        &SOURCE
            .replace("number required", "text")
            .replace("place closed", "place"),
    );
    assert_eq!(before.analysis.fingerprint, changed.analysis.fingerprint);
    let older = compile_source_with_options(
        "world.wl",
        &without,
        CompileOptions::v1_11().with_object_refs(true),
    );
    assert_eq!(before.analysis.fingerprint, older.analysis.fingerprint);
}

#[test]
fn all_memory_compilation_entries_use_the_same_validator() {
    let source = SOURCE.replace("population = 0", "population = \"很多\"");
    let root = std::env::temp_dir().join("worldline-schema-memory");
    let path = root.join("world.wl");
    let multi = worldline_core::compile_sources_with_options(
        &path,
        &std::collections::BTreeMap::from([(path.clone(), source.clone())]),
        CompileOptions::v1_12().with_object_refs(true),
    );
    assert!(codes(&multi).contains(&"SCH005"));
    let direct = compile(&source);
    assert_eq!(codes(&multi), codes(&direct));
}

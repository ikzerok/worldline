use super::*;
use worldline_core::ast::PropertyValue;

#[test]
fn character_property_requires_all_three_explicit_gates() {
    let source = "character lin\nentity boat kind ship\n  property captain = ref(\"character\", \"lin\")\nevent start\n  -> END\n";
    for version in [
        CompileOptions::v1_9(),
        CompileOptions::v1_10(),
        CompileOptions::v1_11(),
        CompileOptions::v1_12(),
    ] {
        let old = compile_source_with_options(
            "world.wl",
            source,
            version.with_object_refs(true).with_character_refs(true),
        );
        assert!(
            old.has_errors(),
            "{} accepted character ref",
            version.language_version.as_str()
        );
    }
    for settings in [
        CompileOptions::v1_13(),
        CompileOptions::v1_13().with_object_refs(true),
        CompileOptions::v1_13().with_character_refs(true),
    ] {
        let rejected = compile_source_with_options("world.wl", source, settings);
        assert!(
            codes(&rejected).contains(&"P004"),
            "{:?}",
            rejected.diagnostics
        );
    }
    let accepted = compile(source);
    assert!(!accepted.has_errors(), "{:?}", accepted.diagnostics);
    let value = &accepted.program.entities[0].properties[0].value;
    assert_eq!(
        value,
        &PropertyValue::Ref(TargetRef::new("character", "lin"))
    );
    assert_eq!(
        worldline_core::authoring::property_source(value),
        "ref(\"character\", \"lin\")"
    );
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        serde_json::json!({"kind":"character","id":"lin"})
    );
}

#[test]
fn schema_character_kind_is_gated_without_relaxing_old_ref_kinds() {
    let source = full_source();
    for settings in [
        CompileOptions::v1_12()
            .with_object_refs(true)
            .with_character_refs(true),
        CompileOptions::v1_13().with_object_refs(true),
        CompileOptions::v1_13().with_character_refs(true),
    ] {
        let result = compile_source_with_options("world.wl", &source, settings);
        assert!(codes(&result).contains(&"SCH001"));
    }
    let good = compile(&source);
    assert!(!good.has_errors(), "{:?}", good.diagnostics);
    let old_refs = source
        .replace("ref character required", "ref entity required")
        .replace("ref(\"character\", \"lin\")", "ref(\"entity\", \"boat\")");
    let old = compile_source_with_options(
        "world.wl",
        &old_refs,
        CompileOptions::v1_12().with_object_refs(true),
    );
    assert!(!old.has_errors(), "{:?}", old.diagnostics);
    for kind in [
        "event", "scene", "state", "file", "asset", "world", "fragment",
    ] {
        let result = compile(&source.replace(
            "ref(\"character\", \"lin\")",
            &format!("ref(\"{kind}\", \"lin\")"),
        ));
        assert!(codes(&result).contains(&"P004"));
    }
}

#[test]
fn missing_wrong_kind_plain_string_and_duplicate_fail_precisely() {
    let source = full_source();
    for (needle, replacement, code) in [
        (
            "captain = ref(\"character\", \"lin\")",
            "captain = ref(\"character\", \"absent\")",
            "A214",
        ),
        (
            "captain = ref(\"character\", \"lin\")",
            "captain = ref(\"entity\", \"boat\")",
            "SCH007",
        ),
        (
            "captain = ref(\"character\", \"lin\")",
            "captain = \"lin\"",
            "SCH005",
        ),
        (
            "  property captain = ref(\"character\", \"lin\")",
            "  // removed captain",
            "SCH004",
        ),
        (
            "captain ref character required",
            "captain ref character entity_type ship required",
            "SCH001",
        ),
    ] {
        let result = compile(&source.replace(needle, replacement));
        assert!(
            codes(&result).contains(&code),
            "{code}: {:?}",
            result.diagnostics
        );
    }
    assert!(compile(&(source + "character lin\n")).has_errors());
}

#[test]
fn static_property_changes_keep_runtime_identity_but_character_rename_does_not() {
    let source = full_source();
    let before = compile(&source);
    let after =
        compile(&source.replace("ref(\"character\", \"lin\")", "ref(\"character\", \"mei\")"));
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    let renamed = compile(&source.replace("lin", "navigator"));
    assert!(!renamed.has_errors());
    assert_ne!(before.analysis.fingerprint, renamed.analysis.fingerprint);
}

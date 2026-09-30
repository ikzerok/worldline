use worldline_core::{compile_source_with_options, CompileOptions};
#[test]
fn relation_endpoint_consumes_the_complete_value() {
    for field in ["from", "from_kind", "to", "to_kind"] {
        for tail in [
            "entity kind organization",
            "entity 42",
            "entity + entity",
            "entity\u{2003}garbage",
            "",
        ] {
            let source = format!("relation_type owns\n  {field} {tail}\n");
            let result = compile_source_with_options("world.wl", &source, CompileOptions::v1_10());
            assert!(
                result.diagnostics.iter().any(|d| d.code == "P004"),
                "accepted {source}"
            );
        }
    }
}
#[test]
fn valid_comment_and_alias_duplicates_keep_existing_contract() {
    let result = compile_source_with_options(
        "world.wl",
        "relation_type owns\n  from entity // 合法注释\n  to entity\n",
        CompileOptions::v1_10(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let result = compile_source_with_options(
        "world.wl",
        "relation_type owns\n  from entity\n  from_kind entity\n",
        CompileOptions::v1_10(),
    );
    assert!(result.diagnostics.iter().any(|d| d.code == "A220"));
}

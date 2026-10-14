use super::*;

#[test]
fn strict_json_rejects_unknown_and_duplicate_keys_at_every_nested_boundary() {
    for input in [
        r#"{"kind":"entity","id":"harbor","extra":1}"#,
        r#"{"kind":"entity","id":"harbor","id":"remote"}"#,
        r#"{"kind":"unknown","id":"harbor"}"#,
    ] {
        assert!(parse_schema_authoring_target(input).is_err());
    }
    let f = fixture("json", true);
    let req = request(&f.project);
    let valid = serde_json::to_value(&req).unwrap();
    assert!(parse_schema_authoring_request(&valid.to_string()).is_ok());
    for pointer in ["", "/target", "/binding"] {
        let mut value = valid.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        assert!(
            parse_schema_authoring_request(&value.to_string()).is_err(),
            "{pointer}"
        );
    }
    for field in [
        serde_json::json!({"op":"set","field_id":"people_id","key":"population","value":null}),
        serde_json::json!({"op":"set","field_id":"people_id","key":"population","value":{"kind":"entity","id":"harbor","unknown":true}}),
        serde_json::json!({"op":"remove","field_id":"people_id","key":"population","value":5}),
    ] {
        let mut value = valid.clone();
        value["fields"] = serde_json::json!([field]);
        assert!(parse_schema_authoring_request(&value.to_string()).is_err());
    }
    let duplicate = format!(
        r#"{{"schema_version":1,"expected_baseline":"{}","target":{{"kind":"entity","id":"harbor"}},"binding":{{"op":"keep"}},"fields":[{{"op":"set","field_id":"owner_id","key":"owner","value":{{"kind":"entity","id":"council","id":"remote"}}}}]}}"#,
        req.expected_baseline
    );
    assert!(parse_schema_authoring_request(&duplicate).is_err());
}

#[test]
fn request_limits_and_duplicate_operations_match_rust_and_json_paths() {
    let f = fixture("limits", true);
    let mut req = request(&f.project);
    req.fields = vec![set(
        "note_id",
        "note",
        PropertyValue::Str("x".repeat(MAX_SCHEMA_AUTHORING_VALUE_BYTES + 1)),
    )];
    assert!(parse_schema_authoring_request(&serde_json::to_string(&req).unwrap()).is_err());
    assert!(f.project.preview_schema_authoring(&req).is_err());
    req.fields = vec![set("people_id", "population", PropertyValue::Num(1.0)); 2];
    assert!(parse_schema_authoring_request(&serde_json::to_string(&req).unwrap()).is_err());
    assert!(f.project.preview_schema_authoring(&req).is_err());
    req.fields = vec![
        set("people_id", "population", PropertyValue::Num(1.0));
        MAX_SCHEMA_AUTHORING_FIELDS + 1
    ];
    assert!(f.project.preview_schema_authoring(&req).is_err());
    req.fields.clear();
    req.target.id = "x".repeat(MAX_SCHEMA_AUTHORING_ID_BYTES + 1);
    assert!(f.project.preview_schema_authoring(&req).is_err());
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
}

#[test]
fn repeated_large_reference_displays_reject_before_unbounded_clone() {
    let mut f = fixture("output-budget", true);
    let mut schema = SCHEMAS.to_owned();
    for n in 0..30 {
        schema = schema.replacen("schema free_city", &format!("  field candidate_{n} candidate_{n} ref entity entity_type organization\nschema free_city"), 1);
    }
    f.project
        .set_text(&f.root.join("schema.wl"), schema)
        .unwrap();
    let source = FACTS.replace("as \"议会\"", &format!("as \"{}\"", "x".repeat(300_000)));
    f.project
        .set_text(&f.root.join("world.wl"), source)
        .unwrap();
    let baseline = f.project.content_baseline();
    let error = f
        .project
        .schema_authoring(&TargetRef::new("entity", "harbor"))
        .unwrap_err();
    assert!(error.contains("预算"), "{error}");
    assert_eq!(baseline, f.project.content_baseline());
}

#[test]
fn optimized_index_preserves_first_duplicate_instance_and_binding_order() {
    let source = "schema city for entity\n  field number_id number number\nentity same kind place\n  property number = \"bad\"\nentity same kind place\n  property number = 1\nbind entity same to city\nbind entity same to city\n";
    let compiled = worldline_core::compile_source_with_options(
        "world.wl",
        source,
        worldline_core::CompileOptions::v1_12(),
    );
    let old_contract =
        worldline_core::schemas::validate(&compiled.program, &compiled.analysis.catalog);
    let cached = SchemaIndex::from_compile(&compiled);
    assert_eq!(
        serde_json::to_value(&cached.instances).unwrap(),
        serde_json::to_value(&old_contract.instances).unwrap()
    );
    assert_eq!(cached.instances.len(), 2);
    assert_eq!(cached.instances[0].line, 3);
    assert!(cached.instances[0]
        .diagnostics
        .iter()
        .any(|d| d.code == "SCH005"));
    assert!(cached.instances[1]
        .diagnostics
        .iter()
        .any(|d| d.code == "SCH003"));
}

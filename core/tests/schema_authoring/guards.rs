use super::*;

#[test]
fn stale_baseline_digest_and_external_changes_never_partially_write() {
    let mut f = fixture("stale", true);
    let mut req = request(&f.project);
    req.binding = SchemaBindingMutation::Change {
        schema_id: "free_city".into(),
    };
    req.fields = vec![set("people_id", "population", PropertyValue::Num(2.0))];
    let plan = f.project.preview_schema_authoring(&req).unwrap();
    let baseline = f.project.content_baseline();
    assert!(f.project.apply_schema_authoring(&req, "forged").is_err());
    assert_eq!(baseline, f.project.content_baseline());
    fs::write(
        f.root.join("binding.wl"),
        "bind entity harbor to free_city\n",
    )
    .unwrap();
    assert!(f
        .project
        .apply_schema_authoring(&req, &plan.plan_digest)
        .is_err());
    assert_eq!(baseline, f.project.content_baseline());
    assert_eq!(f.project.document(&f.root.join("world.wl")).unwrap(), FACTS);
    fs::write(
        f.root.join("binding.wl"),
        "bind entity harbor to city // 独立绑定\n",
    )
    .unwrap();
    fs::write(f.root.join("new.wl"), "entity unseen kind place\n").unwrap();
    assert!(f
        .project
        .apply_schema_authoring(&req, &plan.plan_digest)
        .is_err());
    assert_eq!(baseline, f.project.content_baseline());
    fs::remove_file(f.root.join("new.wl")).unwrap();
    f.project
        .set_text(&f.root.join("world.wl"), format!("{FACTS}// 新本地稿\n"))
        .unwrap();
    let changed = f.project.content_baseline();
    assert!(f
        .project
        .apply_schema_authoring(&req, &plan.plan_digest)
        .is_err());
    assert_eq!(changed, f.project.content_baseline());
}

#[test]
fn refresh_generation_invalidates_old_plan_even_after_content_returns() {
    let mut f = fixture("generation", true);
    let mut req = request(&f.project);
    req.fields = vec![set("people_id", "population", PropertyValue::Num(2.0))];
    let plan = f.project.preview_schema_authoring(&req).unwrap();
    fs::write(f.root.join("world.wl"), format!("{FACTS}// 外部稿\n")).unwrap();
    f.project.refresh().unwrap();
    fs::write(f.root.join("world.wl"), FACTS).unwrap();
    f.project.refresh().unwrap();
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
    assert!(f
        .project
        .apply_schema_authoring(&req, &plan.plan_digest)
        .is_err());
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
    let new_plan = f.project.preview_schema_authoring(&req).unwrap();
    assert_ne!(plan.plan_digest, new_plan.plan_digest);
}

#[test]
fn ambiguous_schema_field_binding_property_and_incomplete_sources_are_read_only() {
    for (name, path, source) in [
        ("schema", "schema.wl", format!("{SCHEMAS}{SCHEMAS}")),
        (
            "field",
            "schema.wl",
            SCHEMAS.replace(
                "  field people_id population number required",
                "  field people_id population number required\n  field people_id other number",
            ),
        ),
        (
            "binding",
            "binding.wl",
            "bind entity harbor to city\nbind entity harbor to city\n".into(),
        ),
        (
            "property",
            "world.wl",
            FACTS.replace(
                "  property population = 0",
                "  property population = 1\n  property population = 0",
            ),
        ),
        (
            "missing",
            "world.wl",
            format!("include \"missing.wl\"\n{FACTS}"),
        ),
        (
            "syntax",
            "world.wl",
            format!("{FACTS}\nschema incomplete\n"),
        ),
    ] {
        let mut f = fixture(name, true);
        f.project.set_text(&f.root.join(path), source).unwrap();
        let view = f
            .project
            .schema_authoring(&TargetRef::new("entity", "harbor"))
            .unwrap();
        assert!(!view.complete && !view.writable, "{name}");
        let baseline = f.project.content_baseline();
        let mut req = request(&f.project);
        req.fields = vec![set("people_id", "population", PropertyValue::Num(4.0))];
        assert!(f.project.preview_schema_authoring(&req).is_err(), "{name}");
        assert_eq!(baseline, f.project.content_baseline());
    }
}

#[test]
fn duplicate_or_missing_object_and_stale_compile_cannot_guess_a_target() {
    let mut f = fixture("identity", true);
    let compiled = f.project.compile();
    f.project
        .set_text(
            &f.root.join("world.wl"),
            format!("{FACTS}\nentity harbor kind place\n"),
        )
        .unwrap();
    assert!(f
        .project
        .schema_authoring(&TargetRef::new("entity", "harbor"))
        .is_err());
    assert!(f
        .project
        .schema_authoring_from_compile(&compiled, &TargetRef::new("entity", "harbor"))
        .is_err());
    assert!(f
        .project
        .schema_authoring(&TargetRef::new("character", "harbor"))
        .is_err());
    assert!(f
        .project
        .schema_authoring(&TargetRef::new("entity", "港城"))
        .is_err());
}

#[test]
fn wrong_field_identity_and_binding_intent_preserve_the_entire_project() {
    let f = fixture("intent", true);
    let mut req = request(&f.project);
    req.fields = vec![set("people_id", "renamed", PropertyValue::Num(5.0))];
    assert!(f.project.preview_schema_authoring(&req).is_err());
    req.fields.clear();
    req.binding = SchemaBindingMutation::Bind {
        schema_id: "city".into(),
    };
    assert!(f.project.preview_schema_authoring(&req).is_err());
    req.binding = SchemaBindingMutation::Change {
        schema_id: "world_data".into(),
    };
    assert!(f.project.preview_schema_authoring(&req).is_err());
    req.binding = SchemaBindingMutation::Unbind;
    req.fields = vec![SchemaFieldMutation::Remove {
        field_id: "people_id".into(),
        key: "population".into(),
    }];
    assert!(f.project.preview_schema_authoring(&req).is_err());
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
}

#[test]
fn unknown_workspace_capability_and_unresolved_recovery_reject_before_edit() {
    let mut f = fixture("capability", true);
    fs::write(f.root.join(".world/project.json"), br#"{"schema_version":1,"language_version":"1.13","required_features":["content.object_refs.v1","content.character_refs.v1","content.relations.v1","future.unknown.v1"]}"#).unwrap();
    f.project = Project::open(&f.root).unwrap();
    let req = request(&f.project);
    let baseline = f.project.content_baseline();
    assert!(f.project.preview_schema_authoring(&req).is_err());
    assert_eq!(baseline, f.project.content_baseline());
    let f = fixture("recovery", true);
    fs::create_dir_all(f.root.join(".world/.transactions/unresolved")).unwrap();
    let req = request(&f.project);
    assert!(f.project.preview_schema_authoring(&req).is_err());
    assert_eq!(req.expected_baseline, f.project.content_baseline());
}

#[test]
fn unsupported_reference_capability_and_non_finite_number_never_mutate() {
    let f = fixture("value", true);
    let mut req = request(&f.project);
    req.fields = vec![set("people_id", "population", PropertyValue::Num(f64::NAN))];
    assert!(f.project.preview_schema_authoring(&req).is_err());
    req.fields = vec![set(
        "owner_id",
        "owner",
        PropertyValue::Ref(TargetRef::new("world", "globe")),
    )];
    assert!(f.project.preview_schema_authoring(&req).is_err());
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
}

#[test]
fn no_op_set_preserves_literal_bytes_and_does_not_report_changed_files() {
    let mut f = fixture("noop", true);
    let original = FACTS.replace("population = 0", "population = 0.00");
    f.project
        .set_text(&f.root.join("world.wl"), original.clone())
        .unwrap();
    let mut req = request(&f.project);
    req.fields = vec![set("people_id", "population", PropertyValue::Num(0.0))];
    let result = apply(&mut f.project, &req);
    assert!(!result.changed && result.changed_files.is_empty());
    assert_eq!(
        f.project.document(&f.root.join("world.wl")).unwrap(),
        original
    );
}

#[cfg(unix)]
#[test]
fn read_only_source_is_visible_but_structured_changes_are_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture("readonly-source", true);
    let path = f.root.join("world.wl");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let projection = f
        .project
        .schema_authoring(&TargetRef::new("entity", "harbor"))
        .unwrap();
    assert!(
        projection.complete && !projection.writable,
        "complete={}, writable={}, reasons={:?}, permissions={:?}",
        projection.complete,
        projection.writable,
        projection.read_only_reasons,
        fs::metadata(&path).unwrap().permissions()
    );
    assert!(!projection.read_only_reasons.is_empty());
    let mut req = request(&f.project);
    req.fields = vec![set("people_id", "population", PropertyValue::Num(9.0))];
    assert!(f.project.preview_schema_authoring(&req).is_err());
    assert_eq!(f.project.content_baseline(), req.expected_baseline);
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
}

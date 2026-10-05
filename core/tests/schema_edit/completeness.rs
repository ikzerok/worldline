use super::*;
use worldline_core::schemas::{SchemaEditPreview, SchemaIncompleteReason as Reason};

fn edit(project: &Project, path: &str, source: &str) -> SourceEditRequest {
    SourceEditRequest {
        path: path.into(),
        ..request(project, source)
    }
}

fn assert_loading(preview: &SchemaEditPreview, code: &str) {
    assert!(!preview.complete);
    assert_eq!(preview.incomplete_reasons, [Reason::SourceLoading]);
    assert!(preview
        .before_diagnostics
        .iter()
        .chain(&preview.after_diagnostics)
        .any(|d| d.code == code));
    assert_eq!(
        serde_json::to_value(&preview.incomplete_reasons).unwrap(),
        serde_json::json!(["source_loading"])
    );
}

#[test]
fn missing_include_with_no_known_impacts_is_not_a_complete_empty_result() {
    let f = fixture("missing-empty");
    let baseline = f.project.content_baseline();
    let draft = format!("{SCHEMA}include \"missing.wl\"\n");
    let preview = f
        .project
        .preview_schema_edit(&request(&f.project, &draft))
        .unwrap();
    assert_loading(&preview, "A105");
    assert!(preview.field_changes.is_empty());
    assert!(preview.instance_impacts.is_empty());
    assert_eq!(baseline, f.project.content_baseline());
    assert_eq!(
        f.project.document(&f.root.join("schema.wl")).unwrap(),
        SCHEMA
    );
    assert_eq!(
        fs::read_to_string(f.root.join("schema.wl")).unwrap(),
        SCHEMA
    );
}

#[test]
fn valid_unbound_schema_has_a_complete_empty_impact_list() {
    let mut f = fixture("known-empty");
    f.project
        .set_text(
            &f.root.join("world.wl"),
            FACTS.replace("bind entity harbor to city\n", ""),
        )
        .unwrap();
    let preview = f
        .project
        .preview_schema_edit(&request(&f.project, &SCHEMA.replace("number", "text")))
        .unwrap();
    assert!(preview.complete);
    assert!(preview.incomplete_reasons.is_empty());
    assert_eq!(preview.field_changes.len(), 1);
    assert!(preview.instance_impacts.is_empty());
    assert_eq!(
        serde_json::to_value(&preview).unwrap()["incomplete_reasons"],
        serde_json::json!([])
    );
}

#[test]
fn multiple_includes_keep_cross_file_known_impacts_and_deduplicate_loading_reasons() {
    let mut f = fixture("partial-multiple");
    fs::create_dir_all(f.root.join("facts")).unwrap();
    fs::write(
        f.root.join("facts/second.wl"),
        "entity hill kind place\n  property population = 1\nbind entity hill to city\n",
    )
    .unwrap();
    fs::write(
        f.root.join("world.wl"),
        format!("include \"schema.wl\"\ninclude \"facts/second.wl\"\n{FACTS}"),
    )
    .unwrap();
    f.project = Project::open(&f.root).unwrap();
    let baseline = f.project.content_baseline();
    let draft = format!(
        "{}include \"missing-one.wl\"\ninclude \"missing-two.wl\"\n",
        SCHEMA.replace("number", "text")
    );
    let preview = f
        .project
        .preview_schema_edit(&request(&f.project, &draft))
        .unwrap();
    assert_loading(&preview, "A105");
    assert_eq!(
        preview
            .after_diagnostics
            .iter()
            .filter(|d| d.code == "A105")
            .count(),
        2
    );
    assert_eq!(preview.field_changes.len(), 1);
    assert_eq!(
        preview
            .instance_impacts
            .iter()
            .map(|i| i.target.id.as_str())
            .collect::<Vec<_>>(),
        ["harbor", "hill"]
    );
    assert!(preview
        .instance_impacts
        .iter()
        .all(|i| i.after_diagnostics.iter().any(|d| d.code == "SCH005")));
    assert_eq!(baseline, f.project.content_baseline());
}

#[test]
fn resolving_missing_include_is_incomplete_once_then_complete_on_the_next_preview() {
    let mut f = fixture("repair-missing");
    let broken = format!("{SCHEMA}include \"wrong.wl\"\n");
    f.project
        .set_text(&f.root.join("schema.wl"), broken.clone())
        .unwrap();
    let repair = request(&f.project, &format!("{SCHEMA}include \"world.wl\"\n"));
    let baseline = f.project.content_baseline();
    let preview = f.project.preview_schema_edit(&repair).unwrap();
    assert_loading(&preview, "A105");
    assert!(preview.before_diagnostics.iter().any(|d| d.code == "A105"));
    assert!(!preview.after_diagnostics.iter().any(|d| d.code == "A105"));
    assert_eq!(f.project.content_baseline(), baseline);
    f.project
        .apply_schema_edit(&repair, &preview.plan_digest)
        .unwrap();
    let next = f
        .project
        .preview_schema_edit(&request(&f.project, &repair.source))
        .unwrap();
    assert!(next.complete);
    assert!(next.incomplete_reasons.is_empty());
    assert!(!next.changed);
    assert_eq!(
        fs::read_to_string(f.root.join("schema.wl")).unwrap(),
        SCHEMA
    );
}

#[test]
fn fixing_the_missing_file_restores_complete_cross_file_impacts_after_refresh() {
    let mut f = fixture("repair-file");
    fs::write(
        f.root.join("schema.wl"),
        format!("{SCHEMA}include \"late.wl\"\n"),
    )
    .unwrap();
    f.project = Project::open(&f.root).unwrap();
    let source = format!("{}include \"late.wl\"\n", SCHEMA.replace("number", "text"));
    assert_loading(
        &f.project
            .preview_schema_edit(&request(&f.project, &source))
            .unwrap(),
        "A105",
    );
    fs::write(
        f.root.join("late.wl"),
        "entity late kind place\n  property population = 2\nbind entity late to city\n",
    )
    .unwrap();
    f.project.refresh().unwrap();
    let preview = f
        .project
        .preview_schema_edit(&request(&f.project, &source))
        .unwrap();
    assert!(preview.complete);
    assert!(preview.incomplete_reasons.is_empty());
    assert_eq!(preview.instance_impacts.len(), 2);
}

#[test]
fn outside_and_absolute_includes_stay_blocked_without_loading_external_instances() {
    let f = fixture("boundary");
    let outside = fixture("external");
    fs::write(
        outside.root.join("world.wl"),
        "entity secret kind place\n  property population = 9\nbind entity secret to city\n",
    )
    .unwrap();
    let relative = format!(
        "../{}/world.wl",
        outside.root.file_name().unwrap().to_str().unwrap()
    );
    let absolute = outside
        .root
        .join("world.wl")
        .to_string_lossy()
        .replace('\\', "/");
    for path in [relative, absolute] {
        let draft = format!("{}include \"{path}\"\n", SCHEMA.replace("number", "text"));
        let preview = f
            .project
            .preview_schema_edit(&request(&f.project, &draft))
            .unwrap();
        assert_loading(&preview, "A109");
        assert_eq!(preview.instance_impacts.len(), 1);
        assert_eq!(preview.instance_impacts[0].target.id, "harbor");
        assert!(!preview
            .after_diagnostics
            .iter()
            .any(|d| d.file.contains("external")));
    }
    assert_eq!(
        f.project.document(&f.root.join("schema.wl")).unwrap(),
        SCHEMA
    );
}

#[test]
fn include_cycle_and_io_failure_report_loading_without_discarding_known_instances() {
    for (name, include) in [("cycle", "schema.wl"), ("io", "directory.wl")] {
        let f = fixture(name);
        if name == "io" {
            fs::create_dir(f.root.join(include)).unwrap();
        }
        let draft = format!(
            "{}include \"{include}\"\n",
            SCHEMA.replace("number", "text")
        );
        let preview = f
            .project
            .preview_schema_edit(&request(&f.project, &draft))
            .unwrap();
        assert_loading(&preview, "A105");
        assert_eq!(preview.instance_impacts.len(), 1);
        assert!(preview.after_diagnostics.iter().any(|d| d.code == "A105"
            && d.message.contains(if name == "io" {
                "无法读取"
            } else {
                "环路"
            })));
    }
}

#[test]
fn inactive_and_deleted_include_sources_also_make_coverage_unknown() {
    for deleted in [false, true] {
        let mut f = fixture(if deleted { "deleted" } else { "inactive" });
        fs::write(
            f.root.join("hidden.wl"),
            "entity hidden kind place\nbind entity hidden to city\n",
        )
        .unwrap();
        if !deleted {
            fs::write(f.root.join(".world/project.json"), r#"{"schema_version":1,"language_version":"1.12","required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl","schema.wl"],"archived":["hidden.wl"]}}"#).unwrap();
        }
        f.project = Project::open(&f.root).unwrap();
        if deleted {
            f.project
                .delete_document(&f.root.join("hidden.wl"))
                .unwrap();
        }
        let draft = format!(
            "{}include \"hidden.wl\"\n",
            SCHEMA.replace("number", "text")
        );
        let preview = f
            .project
            .preview_schema_edit(&request(&f.project, &draft))
            .unwrap();
        assert_loading(&preview, "A105");
        assert_eq!(preview.instance_impacts.len(), 1);
        assert_eq!(
            fs::read_to_string(f.root.join("hidden.wl")).unwrap(),
            "entity hidden kind place\nbind entity hidden to city\n"
        );
    }
}

#[test]
fn all_reason_categories_are_ordered_and_deduplicated_across_both_sides() {
    let mut f = fixture("reason-order");
    let ambiguous = FACTS.replace(
        "  property population = 0",
        "  property population = 0\n  property population = 1",
    );
    f.project
        .set_text(
            &f.root.join("world.wl"),
            format!("{ambiguous}include \"missing.wl\"\n"),
        )
        .unwrap();
    let draft = format!("{SCHEMA}schema broken for entity\n  field unfinished\nentity malformed kind place\n  property bad = (\ninclude \"other-missing.wl\"\n");
    let preview = f
        .project
        .preview_schema_edit(&request(&f.project, &draft))
        .unwrap();
    assert!(!preview.complete);
    assert_eq!(
        preview.incomplete_reasons,
        [
            Reason::SourceLoading,
            Reason::Syntax,
            Reason::AmbiguousDeclaration,
            Reason::SchemaDefinition
        ]
    );
    assert_eq!(
        serde_json::to_value(&preview.incomplete_reasons).unwrap(),
        serde_json::json!([
            "source_loading",
            "syntax",
            "ambiguous_declaration",
            "schema_definition"
        ])
    );
    assert!(preview.before_diagnostics.iter().any(|d| d.code == "A212"));
    assert!(preview.after_diagnostics.iter().any(|d| d.code == "SCH001"));
}

#[test]
fn schema_instance_violations_and_unrelated_story_errors_keep_known_coverage() {
    let mut f = fixture("known-errors");
    f.project.set_authoring_document(&f.root.join(".world/project.json"), br#"{"schema_version":1,"language_version":"1.12","required_features":["content.object_refs.v1"]}"#.to_vec()).unwrap();
    let facts = "entity harbor kind place\n  property population = 0\n  property category = \"village\"\n  property mayor = ref(\"entity\", \"harbor\")\n  property extra = 1\nbind entity harbor to city\nevent start\n  -> absent_event\nasset outside file \"../outside.txt\"\n";
    f.project
        .set_text(&f.root.join("world.wl"), facts.into())
        .unwrap();
    let draft = "schema city for entity entity_type place closed\n  field people_id population text required\n  field missing_id missing number required\n  field category_id category enum \"city\"\n  field mayor_id mayor ref entity entity_type organization\n";
    let preview = f
        .project
        .preview_schema_edit(&request(&f.project, draft))
        .unwrap();
    for code in ["SCH004", "SCH005", "SCH006", "SCH007", "SCH008", "A109"] {
        assert!(
            preview.after_diagnostics.iter().any(|d| d.code == code),
            "缺少 {code}：{:?}",
            preview.after_diagnostics
        );
    }
    assert!(preview
        .after_diagnostics
        .iter()
        .any(|d| d.message.contains("absent_event")));
    assert!(preview.complete, "{:?}", preview.incomplete_reasons);
    assert!(preview.incomplete_reasons.is_empty());
    assert_eq!(preview.instance_impacts.len(), 1);
}

#[test]
fn incomplete_draft_apply_save_and_reopen_preserve_bytes_but_cannot_publish() {
    let mut f = fixture("save-incomplete");
    let original = f.project.clone();
    let draft = format!(
        "{}include \"missing.wl\"\n",
        SCHEMA.replace("number", "text")
    );
    let request = request(&f.project, &draft);
    let preview = f.project.preview_schema_edit(&request).unwrap();
    assert_loading(&preview, "A105");
    let applied = f
        .project
        .apply_schema_edit(&request, &preview.plan_digest)
        .unwrap();
    assert_eq!(
        serde_json::to_value(applied).unwrap(),
        serde_json::to_value(preview).unwrap()
    );
    assert_eq!(
        fs::read_to_string(f.root.join("schema.wl")).unwrap(),
        SCHEMA
    );
    f.project.save().unwrap();
    assert_eq!(fs::read_to_string(f.root.join("schema.wl")).unwrap(), draft);
    let mut reopened = Project::open(&f.root).unwrap();
    assert_eq!(reopened.document(&f.root.join("world.wl")).unwrap(), FACTS);
    assert!(reopened.compile().has_errors());
    let selection = serde_json::from_value(serde_json::json!({"schema_version":2,"required_features":["reader.fields.v1"],"site_title":"公开","objects":[{"kind":"event","id":"start"}],"manuscripts":[],"attachments":[]})).unwrap();
    assert!(reopened
        .preview_reader_export(&selection)
        .unwrap_err()
        .contains("A105"));
    assert!(f.project.restore(original));
    assert_eq!(
        f.project.document(&f.root.join("schema.wl")).unwrap(),
        SCHEMA
    );
    assert_eq!(fs::read_to_string(f.root.join("schema.wl")).unwrap(), draft);
}

#[test]
fn fixing_syntax_or_ambiguous_identity_remains_incomplete_for_that_transition() {
    for broken in [
        FACTS.replace("population = 0", "population = ("),
        format!("{FACTS}entity harbor kind place\n"),
    ] {
        let mut f = fixture("repair-identity");
        f.project
            .set_text(&f.root.join("world.wl"), broken)
            .unwrap();
        let repair = edit(&f.project, "world.wl", FACTS);
        let preview = f.project.preview_schema_edit(&repair).unwrap();
        assert!(!preview.complete);
        assert!(!preview.incomplete_reasons.is_empty());
        assert!(preview.after_diagnostics.is_empty());
        f.project
            .apply_schema_edit(&repair, &preview.plan_digest)
            .unwrap();
        let next = f
            .project
            .preview_schema_edit(&edit(&f.project, "world.wl", FACTS))
            .unwrap();
        assert!(next.complete);
        assert!(next.incomplete_reasons.is_empty());
    }
}

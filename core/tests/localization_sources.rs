#![cfg(not(target_arch = "wasm32"))]
#[path = "localization_workbench/support.rs"]
mod support;
use support::*;
use worldline_core::localization::*;
use worldline_core::project::Project;

#[test]
fn included_body_uses_formal_parser_origin_for_catalog_ids_translation_and_navigation() {
    let root_source = "character speaker\nevent start\ninclude \"body.wl\"\n";
    let body = "  Hello #wl-localization:body\n  say speaker \"Spoken\" #wl-localization:spoken\n  choice \"Go\" #wl-localization:go\n    -> END\n";
    let mut fixture = fixture("included-body", root_source, None);
    std::fs::write(fixture.root.join("body.wl"), body).unwrap();
    fixture.project = Project::open(&fixture.root).unwrap();
    let page = page(&fixture.project);
    assert_eq!(page.total, 3);
    assert!(page
        .entries
        .iter()
        .all(|entry| entry.source.as_ref().unwrap().file == "body.wl"));
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.source.as_ref().unwrap().kind.as_str())
            .collect::<Vec<_>>(),
        ["text", "say", "choice"]
    );
    let draft = edit(&fixture.project, &["body", "spoken", "go"]);
    apply(&mut fixture.project, &draft);
    let snapshot = fixture
        .project
        .prepare_localization_presentation(&presentation_request(
            LocalizationPresentationPolicy::Strict,
        ))
        .unwrap();
    let compiled = fixture.project.clone().compile();
    snapshot
        .validate_program(&compiled.program, &compiled.analysis)
        .unwrap();
    assert!(snapshot
        .find_entry(fixture.root.join("body.wl").to_str().unwrap(), 1, "text")
        .is_some());
    assert!(snapshot
        .find_entry(fixture.root.join("world.wl").to_str().unwrap(), 1, "text")
        .is_none());
    let mut buffer = fixture
        .project
        .open_source_writing_buffer(&fixture.root.join("body.wl"))
        .unwrap();
    buffer.replace_source(format!(
        "{body}// unchanged runtime, real unapplied draft\n"
    ));
    let request = worldline_core::draft_rehearsal::DraftRehearsalRequest::from_writing_buffers(
        &fixture.project,
        &[buffer],
        vec![],
        false,
    )
    .unwrap();
    let rehearsal = fixture.project.compile_draft_rehearsal(&request).unwrap();
    let hit = rehearsal
        .localization_source(&snapshot.entries()[0].source)
        .unwrap();
    assert_eq!(hit.path, fixture.root.join("body.wl"));
    assert!(hit.draft);
    assert_eq!(hit.preview, "Hello #wl-localization:body");
    let id = id_draft(&fixture.project, 1, "renamed_body");
    let plan = fixture.project.preview_localization_ids(&id).unwrap();
    assert!(plan.can_apply);
    assert_eq!(plan.changes[0].file, "body.wl");
    fixture
        .project
        .apply_localization_ids(&id, &plan.plan_digest)
        .unwrap();
    assert_eq!(
        fixture
            .project
            .document(&fixture.root.join("world.wl"))
            .unwrap(),
        root_source
    );
    assert!(fixture
        .project
        .document(&fixture.root.join("body.wl"))
        .unwrap()
        .contains("#wl-localization:renamed_body"));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("body.wl")).unwrap(),
        body
    );
}

#[test]
fn indistinguishable_included_statement_origins_are_rejected_without_guessing() {
    let mut fixture = fixture(
        "ambiguous-body",
        "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n",
        None,
    );
    std::fs::write(
        fixture.root.join("a.wl"),
        "  First #wl-localization:first\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("b.wl"),
        "  Second #wl-localization:second\n",
    )
    .unwrap();
    fixture.project = Project::open(&fixture.root).unwrap();
    assert!(!fixture.project.clone().compile().has_errors());
    let baseline = fixture.project.content_baseline();
    assert_eq!(
        fixture
            .project
            .query_localization_catalog(&LocalizationCatalogQuery::default())
            .unwrap_err()
            .code,
        "INVALID_SOURCE"
    );
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert!(!fixture.project.is_dirty());
}

#[test]
fn legacy_exchange_now_uses_actual_source_and_old_wrong_source_package_is_read_only_rejected() {
    let mut fixture = fixture(
        "legacy-included",
        "event start\ninclude \"body.wl\"\n",
        None,
    );
    let body = "  Body #wl-localization:body\n  choice \"Go\" #wl-localization:go\n    -> END\n";
    std::fs::write(fixture.root.join("body.wl"), body).unwrap();
    fixture.project = Project::open(&fixture.root).unwrap();
    let selection = LocalizationSelection {
        schema_version: 1,
        source_locale: "en".into(),
        target_locale: "zh-Hant".into(),
        string_ids: vec!["body".into(), "go".into()],
    };
    let mut exchange = fixture
        .project
        .preview_localization_export(&selection)
        .unwrap()
        .exchange;
    assert!(exchange
        .entries
        .iter()
        .all(|entry| entry.source.file == "body.wl"));
    for entry in &mut exchange.entries {
        entry.translation_parts = Some(translate(&entry.source_parts));
    }
    let valid = exchange.clone();
    exchange.entries[0].source.file = "world.wl".into();
    let baseline = fixture.project.content_baseline();
    let before = disk(&fixture.root);
    let plan = fixture
        .project
        .preview_localization_import(&selection, &exchange)
        .unwrap();
    assert!(!plan.can_apply);
    let mismatch = plan
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "SOURCE_MISMATCH")
        .unwrap();
    assert_eq!(mismatch.source.as_ref().unwrap().file, "body.wl");
    assert!(mismatch.message.contains("重新导出交换包"));
    assert!(fixture
        .project
        .apply_localization_import(&selection, &exchange, &plan.plan_digest)
        .is_err());
    assert_eq!(fixture.project.content_baseline(), baseline);
    assert_eq!(disk(&fixture.root), before);
    assert!(
        exchange.entries[0].translation_parts.is_some(),
        "rejection keeps translator input"
    );
    let plan = fixture
        .project
        .preview_localization_import(&selection, &valid)
        .unwrap();
    assert!(plan.can_apply);
    fixture
        .project
        .apply_localization_import(&selection, &valid, &plan.plan_digest)
        .unwrap();
    assert!(
        !fixture.project.is_dirty(),
        "legacy native import still persists immediately"
    );
}

#[test]
fn repeated_include_in_different_event_contexts_still_has_one_physical_unit_and_id_patch() {
    let root_source =
        "event first\ninclude \"body.wl\"\n  -> END\nevent second\ninclude \"body.wl\"\n  -> END\n";
    let mut fixture = fixture("repeated-include", root_source, None);
    std::fs::write(fixture.root.join("body.wl"), "  Shared\n").unwrap();
    fixture.project = Project::open(&fixture.root).unwrap();
    let compiled = fixture.project.clone().compile();
    assert!(!compiled.has_errors());
    assert_eq!(compiled.program.events.len(), 2);
    let catalog = page(&fixture.project);
    assert_eq!((catalog.all_total, catalog.total), (1, 1));
    assert_eq!(catalog.entries[0].source.as_ref().unwrap().file, "body.wl");
    assert_eq!(catalog.entries[0].status, LocalizationStatus::MissingId);
    let draft = id_draft(&fixture.project, 1, "shared");
    let plan = fixture.project.preview_localization_ids(&draft).unwrap();
    assert!(plan.can_apply);
    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].file, "body.wl");
    assert_eq!(
        plan.changes[0]
            .after
            .matches("#wl-localization:shared")
            .count(),
        1
    );
    fixture
        .project
        .apply_localization_ids(&draft, &plan.plan_digest)
        .unwrap();
    let catalog = page(&fixture.project);
    assert_eq!(
        catalog.entries[0].status,
        LocalizationStatus::MissingTranslation
    );
    let draft = edit(&fixture.project, &["shared"]);
    apply(&mut fixture.project, &draft);
    assert_eq!(
        fixture
            .project
            .prepare_localization_presentation(&presentation_request(
                LocalizationPresentationPolicy::Strict
            ))
            .unwrap()
            .entries()
            .len(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("body.wl")).unwrap(),
        "  Shared\n"
    );
}

#[test]
fn identical_text_in_different_physical_files_is_not_merged() {
    let mut fixture = fixture(
        "distinct-physical",
        "include \"a.wl\"\ninclude \"b.wl\"\ninclude \"a.wl\"\n",
        None,
    );
    for (file, event) in [("a.wl", "first"), ("b.wl", "second")] {
        std::fs::write(
            fixture.root.join(file),
            format!("event {event}\n  Same\n  -> END\n"),
        )
        .unwrap();
    }
    fixture.project = Project::open(&fixture.root).unwrap();
    let catalog = page(&fixture.project);
    assert_eq!(catalog.total, 2);
    assert_eq!(
        catalog.entries[0].source_parts,
        catalog.entries[1].source_parts
    );
    assert_eq!(
        catalog
            .entries
            .iter()
            .map(|entry| entry.source.as_ref().unwrap().file.as_str())
            .collect::<Vec<_>>(),
        ["a.wl", "b.wl"]
    );
    let draft = LocalizationIdDraft {
        schema_version: 1,
        source_baseline: catalog.source_baseline,
        assignments: catalog
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| LocalizationIdAssignment {
                source: entry.source.clone().unwrap(),
                source_revision: entry.source_revision.clone().unwrap(),
                expected_id: None,
                id: format!("physical_{index}"),
            })
            .collect(),
    };
    let plan = fixture.project.preview_localization_ids(&draft).unwrap();
    assert!(plan.can_apply);
    assert_eq!(plan.changes.len(), 2);
    fixture
        .project
        .apply_localization_ids(&draft, &plan.plan_digest)
        .unwrap();
    let catalog = page(&fixture.project);
    assert_eq!(catalog.total, 2);
    assert!(catalog
        .entries
        .iter()
        .all(|entry| entry.status == LocalizationStatus::MissingTranslation));
}

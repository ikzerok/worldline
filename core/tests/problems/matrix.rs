use super::*;

fn valid_documents() -> Vec<(&'static str, Value)> {
    vec![
        (
            "maps",
            json!({"schema_version":1,"id":"broken","title":"地图","canvas":{"width":800,"height":600,"unit":"normalized"},"raster_layers":[],"layer_order":[],"layers":{},"placements":{},"extensions":{}}),
        ),
        (
            "graph_views",
            json!({"schema_version":1,"id":"broken","title":"图","focus":{"kind":"event","id":"start"},"filters":{"depth":1},"positions":{},"hidden_relation_ids":[]}),
        ),
        (
            "presets",
            json!({"schema_version":1,"id":"broken","title":"预设","map_id":"broken","graph_view_id":null}),
        ),
        (
            "comments",
            json!({"schema_version":1,"id":"broken","author":"作者","body":"批注","anchor":{"kind":"object","target":{"kind":"event","id":"start"}}}),
        ),
        (
            "proposals",
            json!({"schema_version":1,"id":"broken","author":"作者","reason":"说明","changes":[{"path":"world.wl","domain":"content","base":"old","proposed":"new"}]}),
        ),
        (
            "templates",
            json!({"schema_version":1,"id":"project:broken","title":"模板","applies_to":{"kind":"entity","entity_type":"place"},"fields":[]}),
        ),
        (
            "saved_queries",
            json!({"schema_version":1,"id":"broken","name":"查询","query":{"schema_version":1,"filters":[]}}),
        ),
        (
            "manuscripts",
            json!({"schema_version":1,"id":"broken","title":"书稿","entries":[]}),
        ),
        (
            "reader_profiles",
            json!({"schema_version":1,"required_features":["reader.profiles.v1"],"id":"broken","title":"阅读","selection":{"schema_version":1,"site_title":"阅读","objects":[{"kind":"event","id":"missing_selection"}],"manuscripts":[],"attachments":[]},"routes":[]}),
        ),
        (
            "localizations",
            json!({"schema_version":1,"required_features":["content.localization.v1"],"source_locale":"en","target_locale":"fr","entries":{"unselected_missing":{"source_revision":"old","translation_parts":null}}}),
        ),
    ]
}
fn valid_fixture() -> Fixture {
    let registry = manifest();
    let mut files = vec![(
        ".world/project.json".to_owned(),
        serde_json::to_vec(&registry).unwrap(),
    )];
    for (domain, mut value) in valid_documents() {
        value["unknown_optional_extension"] = json!({"keep":"原文 😀"});
        let path = registry[domain]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        files.push((path, serde_json::to_vec(&value).unwrap()));
    }
    let borrowed: Vec<_> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    fixture(&borrowed)
}
#[test]
fn every_registered_domain_has_valid_static_fixture_and_operation_checks_stay_excluded() {
    let f = valid_fixture();
    let before = f.project.snapshot_files().unwrap();
    let r = report(&f.project);
    assert!(r.entries.is_empty(), "{:?}", r.entries);
    assert!(r.complete, "{:?}", r.coverage);
    for domain in ProblemDomain::ALL.into_iter().skip(2) {
        assert!(
            r.coverage.iter().any(|c| c.domain == domain
                && c.path.is_some()
                && c.state == ProblemCoverageState::Checked),
            "{domain:?}"
        );
    }
    for domain in [ProblemDomain::ReaderProfiles, ProblemDomain::Localizations] {
        assert!(r.coverage.iter().any(|c| c.domain == domain
            && c.path.is_none()
            && c.reasons.contains(&"operation_checks_excluded".into())));
    }
    assert_eq!(before, f.project.snapshot_files().unwrap());
}
#[test]
fn every_registered_domain_isolated_missing_invalid_utf8_duplicate_key_unknown_schema_and_tombstone(
) {
    let registry = manifest();
    for (domain, _) in valid_documents() {
        let path = registry[domain]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .as_str()
            .unwrap();
        for case in [
            "missing",
            "utf8",
            "json",
            "duplicate",
            "version",
            "feature",
            "tombstone",
        ] {
            let mut f = valid_fixture();
            let full = f.root.join(path);
            match case {
                "missing" => fs::remove_file(&full).unwrap(),
                "utf8" => fs::write(&full, b"\xff\x00").unwrap(),
                "json" => fs::write(&full, b"{\"schema_version\":").unwrap(),
                "duplicate" => {
                    fs::write(&full, b"{\"schema_version\":1,\"schema_version\":1}").unwrap()
                }
                "version" | "feature" => {
                    let mut value: Value =
                        serde_json::from_slice(&fs::read(&full).unwrap()).unwrap();
                    if case == "version" {
                        value["schema_version"] = json!(99);
                    } else {
                        value["required_features"] = json!(["future.unknown.v9"]);
                    }
                    fs::write(&full, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                "tombstone" => f.project.delete_authoring_document(&full).unwrap(),
                _ => unreachable!(),
            }
            if case != "tombstone" {
                f.project.refresh().unwrap();
            }
            let baseline = f.project.content_baseline();
            let before = f
                .project
                .authoring_document(&full)
                .unwrap()
                .bytes()
                .to_vec();
            let r = report(&f.project);
            assert!(!r.complete, "{domain}/{case} claimed complete");
            assert!(
                r.coverage.iter().any(|c| c.path.as_deref() == Some(path)
                    && matches!(
                        c.state,
                        ProblemCoverageState::Partial | ProblemCoverageState::Unavailable
                    )),
                "{domain}/{case}: {:?}",
                r.coverage
            );
            assert_eq!(baseline, f.project.content_baseline());
            assert_eq!(before, f.project.authoring_document(&full).unwrap().bytes());
            if matches!(case, "version" | "feature") {
                assert!(f.project.authoring_document(&full).unwrap().is_read_only());
            }
            // An invalid sibling never suppresses other nine completed static checks.
            assert_eq!(
                r.coverage
                    .iter()
                    .filter(|c| c.path.is_some()
                        && c.domain != ProblemDomain::Content
                        && c.domain != ProblemDomain::Workspace
                        && c.path.as_deref() != Some(path)
                        && c.state == ProblemCoverageState::Checked)
                    .count(),
                9,
                "{domain}/{case}"
            );
        }
    }
}
#[test]
fn propagated_workspace_diagnostics_are_owned_once_and_invalid_registry_is_not_not_applicable() {
    let f = fixture(&[(
        ".world/project.json",
        br#"{"schema_version":1,"required_features":["future.x"],"maps":{"x":".world/x.json"}}"#,
    )]);
    let r = report(&f.project);
    let ws: Vec<_> = r
        .entries
        .iter()
        .filter(|e| e.code.starts_with("WS"))
        .collect();
    assert_eq!(ws.len(), f.project.authoring_diagnostics().len());
    assert!(ws.iter().all(|e| e.domain == ProblemDomain::Workspace
        && e.primary.precision == ProblemPrecision::Document));
    assert!(r
        .coverage
        .iter()
        .filter(|c| c.path.is_none() && c.domain != ProblemDomain::Content)
        .all(|c| c.state != ProblemCoverageState::NotApplicable));
}
#[test]
fn active_source_selection_excludes_archived_errors_and_scope_loss_is_explicit() {
    let raw=serde_json::to_vec(&json!({"schema_version":1,"required_features":["workspace.source_sets.v1"],"source_config":{"mode":"explicit","active":["world.wl"],"archived":["archived.wl"]}})).unwrap();
    let f = fixture(&[
        (".world/project.json", &raw),
        ("archived.wl", b"event broken\n  -> missing\n"),
    ]);
    let r = report(&f.project);
    assert!(!r.content_has_errors);
    assert!(r
        .coverage
        .iter()
        .all(|c| c.path.as_deref() != Some("archived.wl")));
    assert!(r.entries.is_empty());
}
#[cfg(unix)]
#[test]
fn observation_refuses_symlinks_and_never_reads_outside_workspace() {
    let f = fixture(&[]);
    let outside = f.root.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.json"), b"never read").unwrap();
    std::os::unix::fs::symlink(&outside, f.root.join("escape")).unwrap();
    assert_eq!(
        f.project.problems_observation_key().unwrap_err().code,
        "OBSERVATION_UNAVAILABLE"
    );
    let r = report(&f.project);
    assert!(!r.complete);
    assert!(r
        .reasons
        .contains(&"external_observation_unavailable".into()));
    assert!(r
        .entries
        .iter()
        .all(|e| e.primary.path.as_deref() != Some("escape/secret.json")));
    fs::remove_dir_all(&outside).unwrap();
}
#[test]
fn old_content_snapshot_is_rejected_after_asset_only_observation_changes() {
    let mut f = fixture(&[
        (
            "world.wl",
            b"asset picture image \"picture.png\"\nevent start\n  -> END\n",
        ),
        ("picture.png", b"png"),
    ]);
    let compiled = f.project.compile();
    assert!(
        !compiled.analysis.catalog.assets.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    let baseline = f.project.content_baseline();
    f.project
        .problems_report_with_content(&compiled, &baseline, &ProblemsOptions::default())
        .unwrap();
    fs::remove_file(f.root.join("picture.png")).unwrap();
    assert_eq!(baseline, f.project.content_baseline());
    assert_eq!(
        f.project
            .problems_report_with_content(&compiled, &baseline, &ProblemsOptions::default())
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
    let r = report(&f.project);
    assert!(r.entries.iter().any(|e| e.code == "A215"));
    fs::write(f.root.join("picture.png"), b"png").unwrap();
    assert!(!report(&f.project).entries.iter().any(|e| e.code == "A215"));
}
#[test]
fn report_never_reads_unapplied_disk_include_until_explicit_refresh() {
    let mut f = fixture(&[("world.wl", b"include \"later.wl\"\nevent start\n  -> END\n")]);
    let before = f.project.content_baseline();
    fs::write(f.root.join("later.wl"), b"event later\n  -> absent\n").unwrap();
    let buffered = report(&f.project);
    assert_eq!(before, f.project.content_baseline());
    assert!(buffered.entries.iter().any(|e| e.code == "A105"));
    assert!(buffered
        .coverage
        .iter()
        .all(|c| c.path.as_deref() != Some("later.wl")));
    f.project.refresh().unwrap();
    let refreshed = report(&f.project);
    assert!(refreshed
        .coverage
        .iter()
        .any(|c| c.path.as_deref() == Some("later.wl")));
    assert!(refreshed
        .entries
        .iter()
        .any(|e| e.code == "A101" && e.primary.path.as_deref() == Some("later.wl")));
}
#[test]
fn locale_static_contract_rejects_invalid_ids_and_equal_locales_without_operation_validation() {
    for (source, key) in [("fr", "good"), ("../not_locale", "good"), ("en", "bad/id")] {
        let mut f = valid_fixture();
        let path = f.root.join(".world/locale.json");
        let raw=serde_json::to_vec(&json!({"schema_version":1,"required_features":["content.localization.v1"],"source_locale":source,"target_locale":"fr","entries":{key:{"source_revision":"old","translation_parts":null}}})).unwrap();
        fs::write(&path, &raw).unwrap();
        f.project.refresh().unwrap();
        let r = report(&f.project);
        assert!(
            r.entries.iter().any(|e| e.code == "LOC001"),
            "{source}/{key}: {:?}",
            r.entries
        );
        assert_eq!(raw, f.project.authoring_document(&path).unwrap().bytes());
    }
}
#[test]
fn new_project_does_not_import_disk_only_include() {
    let f = fixture(&[("secret.wl", b"event private\n  -> hidden_target\n")]);
    let mut draft = Project::new(&f.root);
    let entry = draft.entry.clone();
    draft.documents.retain(|path, _| path == &entry);
    draft
        .set_text(
            &entry,
            "include \"secret.wl\"\nevent draft\n  -> END\n".into(),
        )
        .unwrap();
    let baseline = draft.content_baseline();
    let r = report(&draft);
    assert!(r.entries.iter().any(|e| e.code == "A105"));
    assert!(r
        .coverage
        .iter()
        .all(|c| c.path.as_deref() != Some("secret.wl")));
    assert_eq!(baseline, draft.content_baseline());
}

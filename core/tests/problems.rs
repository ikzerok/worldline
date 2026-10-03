use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use worldline_core::{project::Project, *};

struct Fixture {
    root: PathBuf,
    project: Project,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn fixture(files: &[(&str, &[u8])]) -> Fixture {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "worldline-problems-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("world.wl"), "event start\n  正文\n  -> END\n").unwrap();
    for (path, bytes) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let project = Project::open(&root).unwrap();
    Fixture { root, project }
}
fn report(project: &Project) -> ProblemsReport {
    project
        .problems_report(&ProblemsOptions::default())
        .unwrap()
}
fn manifest() -> Value {
    json!({"schema_version":1,"language_version":"1.13","required_features":[
        "presentation.maps.v1","presentation.graph_views.v1","presentation.presets.v1",
        "collaboration.comments.v1","collaboration.proposals.v1","content.templates.v1",
        "catalog.saved_queries.v1","presentation.manuscripts.v1","reader.profiles.v1","content.localization.v1"],
        "maps":{"broken":".world/maps.json"},"graph_views":{"broken":".world/graphs.json"},
        "presets":{"broken":".world/presets.json"},"comments":{"broken":".world/comments.json"},
        "proposals":{"broken":".world/proposals.json"},"templates":{"project:broken":".world/templates.json"},
        "saved_queries":{"broken":".world/queries.json"},"manuscripts":{"broken":".world/manuscripts.json"},
        "reader_profiles":{"broken":".world/readers.json"},"localizations":{"fr":".world/locale.json"}})
}
#[test]
fn scans_ten_domains_without_mutating_bytes_or_legacy_content_gate() {
    let raw = serde_json::to_vec(&manifest()).unwrap();
    let mut files = vec![(".world/project.json", raw.as_slice())];
    for path in [
        "maps",
        "graphs",
        "presets",
        "comments",
        "proposals",
        "templates",
        "queries",
        "manuscripts",
        "readers",
        "locale",
    ] {
        let path = match path {
            "maps" => ".world/maps.json",
            "graphs" => ".world/graphs.json",
            "presets" => ".world/presets.json",
            "comments" => ".world/comments.json",
            "proposals" => ".world/proposals.json",
            "templates" => ".world/templates.json",
            "queries" => ".world/queries.json",
            "manuscripts" => ".world/manuscripts.json",
            "readers" => ".world/readers.json",
            _ => ".world/locale.json",
        };
        files.push((path, b"{\"broken\":"));
    }
    files.push(("ordinary.json", b"not json"));
    let mut f = fixture(&files);
    let before = f.project.snapshot_files().unwrap();
    let baseline = f.project.content_baseline();
    let compiled = f.project.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert!(
        f.project.authoring_diagnostics().is_empty(),
        "{:?}",
        f.project.authoring_diagnostics()
    );
    let report = report(&f.project);
    assert_eq!(report.compile_count, 1);
    assert!(!report.content_has_errors);
    for domain in ProblemDomain::ALL.into_iter().skip(2) {
        assert!(
            report.entries.iter().any(|entry| entry.domain == domain),
            "missing {domain:?}"
        );
        assert!(report
            .coverage
            .iter()
            .any(|c| c.domain == domain && c.path.is_some()));
    }
    assert!(report
        .entries
        .iter()
        .all(|entry| entry.primary.precision == ProblemPrecision::Document));
    assert!(report
        .entries
        .iter()
        .all(|entry| entry.primary.span.is_none() && entry.primary.byte_range.is_none()));
    assert!(!report.complete);
    assert!(!report.read_only);
    assert_eq!(before, f.project.snapshot_files().unwrap());
    assert_eq!(baseline, f.project.content_baseline());
    assert!(!f.project.is_dirty());
    assert_eq!(
        compiled.analysis.fingerprint,
        f.project.compile().analysis.fingerprint
    );
    let reused = f
        .project
        .problems_report_with_content(&compiled, &baseline, &ProblemsOptions::default())
        .unwrap();
    assert_eq!(reused.compile_count, 0);
    assert_eq!(reused.report_version, report.report_version);
    assert_eq!(
        serde_json::from_slice::<ProblemsReport>(&serde_json::to_vec(&report).unwrap()).unwrap(),
        report
    );
}
#[test]
fn severity_and_paths_sort_deterministically_without_basename_merging() {
    let f = fixture(&[
        ("north/same.wl", b"event duplicate\n  north\n  -> END\n"),
        ("south/same.wl", b"event duplicate\n  south\n  -> missing\n"),
    ]);
    let first = report(&f.project);
    let second = report(&f.project);
    assert_eq!(first, second);
    assert!(first
        .entries
        .windows(2)
        .all(|w| w[0].severity <= w[1].severity));
    let duplicate = first
        .entries
        .iter()
        .find(|entry| entry.code == "A104")
        .unwrap();
    assert_eq!(duplicate.primary.path.as_deref(), Some("south/same.wl"));
    let related = first.related_page(&duplicate.id, None, 1).unwrap();
    assert_eq!(related.locations[0].path.as_deref(), Some("north/same.wl"));
    assert_eq!(related.locations[0].precision, ProblemPrecision::Span);
    let mut diags = vec![
        Diagnostic::hint("x", "a", Span::new(1, 1, 1), "提示"),
        Diagnostic::warning("x", "a", Span::new(1, 1, 1), "警告"),
        Diagnostic::error("x", "a", Span::new(1, 1, 1), "错误"),
    ];
    sort_diagnostics(&mut diags);
    assert_eq!(
        diags.iter().map(|d| d.severity).collect::<Vec<_>>(),
        [Severity::Error, Severity::Warning, Severity::Hint]
    );
    assert_eq!(
        serde_json::to_value(&diags[0])
            .unwrap()
            .as_object()
            .unwrap()
            .len(),
        8
    );
}
#[test]
fn query_pages_are_pure_composable_and_reject_stale_or_mixed_cursors() {
    let mut text = String::new();
    for i in 0..130 {
        text.push_str(&format!("event e{i}\n  -> missing{i}\n"));
    }
    let mut f = fixture(&[("errors.wl", text.as_bytes())]);
    let report = report(&f.project);
    let query = ProblemQuery {
        severities: vec![Severity::Error],
        domains: vec![ProblemDomain::Content],
        path: Some("errors.wl".into()),
        text: "不存在".into(),
    };
    let mut query = query;
    query.text.clear();
    let first = report.query(&query, None, 50).unwrap();
    assert_eq!(first.matched, 130);
    let second = report
        .query(&query, first.next_cursor.as_ref(), 50)
        .unwrap();
    let third = report
        .query(&query, second.next_cursor.as_ref(), 50)
        .unwrap();
    assert_eq!(third.entries.len(), 30);
    assert!(third.next_cursor.is_none());
    let ids: std::collections::BTreeSet<_> = first
        .entries
        .iter()
        .chain(&second.entries)
        .chain(&third.entries)
        .map(|e| &e.id)
        .collect();
    assert_eq!(ids.len(), 130);
    let mut other = query.clone();
    other.text = "does not match".into();
    assert_eq!(
        report
            .query(&other, first.next_cursor.as_ref(), 50)
            .unwrap_err()
            .code,
        "INVALID_CURSOR"
    );
    assert_eq!(report.query(&other, None, 50).unwrap().matched, 0);
    assert_eq!(
        report
            .related_page(&first.entries[0].id, first.next_cursor.as_ref(), 50)
            .unwrap_err()
            .code,
        "INVALID_CURSOR"
    );
    assert!(report.query(&query, None, 201).is_err());
    other.path = Some("../outside.wl".into());
    assert!(report.query(&other, None, 50).is_err());
    f.project
        .set_text(&f.root.join("errors.wl"), "event fixed\n  -> END\n".into())
        .unwrap();
    let newer = report_fn(&f.project);
    assert_eq!(
        newer
            .query(&query, first.next_cursor.as_ref(), 50)
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
    assert_eq!(
        f.project
            .problem_location(&report, &first.entries[0].id, None)
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
}
fn report_fn(project: &Project) -> ProblemsReport {
    report(project)
}
#[test]
fn cancellation_limits_and_partial_results_are_explicit() {
    let f = fixture(&[("a.wl", b"event bad\n  -> nowhere\n")]);
    assert_eq!(
        f.project
            .problems_report_with_progress(&ProblemsOptions::default(), &mut |_| false)
            .unwrap_err()
            .code,
        "CANCELLED"
    );
    let limited = f
        .project
        .problems_report(&ProblemsOptions {
            max_entries: 0,
            ..Default::default()
        })
        .unwrap();
    assert!(limited.truncated && !limited.complete && limited.entries.is_empty());
    assert!(limited.reasons.contains(&"entry_limit".to_owned()));
    assert_eq!(
        f.project
            .problems_report(&ProblemsOptions {
                max_report_bytes: 1,
                ..Default::default()
            })
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    let clipped = f
        .project
        .problems_report(&ProblemsOptions {
            max_text_bytes: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(clipped.truncated && clipped.entries.iter().any(|e| e.text_truncated));
    assert!(clipped.entries.iter().all(|e| e.message.len() <= 1));
}
#[test]
fn one_bad_reader_or_locale_does_not_hide_later_documents() {
    let mut value = manifest();
    value["reader_profiles"] =
        json!({"bad":".world/bad-reader.json","good":".world/good-reader.json"});
    value["localizations"] = json!({"fr":".world/fr.json","de":".world/de.json"});
    let raw = serde_json::to_vec(&value).unwrap();
    let locale=serde_json::to_vec(&json!({"schema_version":1,"required_features":["content.localization.v1"],"source_locale":"en","target_locale":"de","entries":{},"extension":true})).unwrap();
    let f = fixture(&[
        (".world/project.json", &raw),
        (".world/bad-reader.json", b"{bad"),
        (".world/good-reader.json", b"{}"),
        (".world/fr.json", b"\xff"),
        (".world/de.json", &locale),
    ]);
    let report = report(&f.project);
    assert_eq!(
        report
            .entries
            .iter()
            .filter(|e| e.domain == ProblemDomain::ReaderProfiles)
            .count(),
        2
    );
    assert_eq!(
        report
            .entries
            .iter()
            .filter(|e| e.domain == ProblemDomain::Localizations)
            .count(),
        1
    );
    assert!(report
        .coverage
        .iter()
        .any(|c| c.path.as_deref() == Some(".world/de.json")
            && c.state == ProblemCoverageState::Checked));
    assert!(report
        .entries
        .iter()
        .any(|e| e.primary.path.as_deref() == Some(".world/fr.json")
            && e.primary.precision == ProblemPrecision::Unavailable));
}
#[test]
fn observation_changes_on_asset_delete_restore_but_not_unsaved_buffer_existence() {
    let f = fixture(&[("asset.png", b"png")]);
    let before = f.project.problems_observation_key().unwrap();
    fs::remove_file(f.root.join("asset.png")).unwrap();
    assert_ne!(before, f.project.problems_observation_key().unwrap());
    fs::write(
        f.root.join("asset.png"),
        b"different bytes same readable contract",
    )
    .unwrap();
    assert_eq!(before, f.project.problems_observation_key().unwrap());
    let root = f.root.join("not_created");
    let draft = Project::new(&root);
    assert!(report(&draft).complete);
    assert_eq!(
        draft.problems_observation_key().unwrap(),
        draft.problems_observation_key().unwrap()
    );
}
#[test]
fn unknown_registered_capabilities_stay_partial_and_raw() {
    let raw=serde_json::to_vec(&json!({"schema_version":1,"required_features":["presentation.maps.v1"],"maps":{"m":".world/m.json"}})).unwrap();
    for bytes in [
        b"{\"schema_version\":99,\"extension\":\"keep\"}".as_slice(),
        b"{\"schema_version\":1,\"required_features\":[\"future.x\"]}",
        b"{\"schema_version\":1,\"schema_version\":1}",
    ] {
        let f = fixture(&[(".world/project.json", &raw), (".world/m.json", bytes)]);
        let before = f.project.content_baseline();
        let report = report(&f.project);
        assert!(!report.complete);
        assert_eq!(
            f.project
                .authoring_document(&f.root.join(".world/m.json"))
                .unwrap()
                .bytes(),
            bytes
        );
        assert_eq!(before, f.project.content_baseline());
    }
}

#[path = "problems/matrix.rs"]
mod matrix;

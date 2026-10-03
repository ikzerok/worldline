use super::*;
use crate::{project::Project, Diagnostic, Span};
use std::path::Path;
fn project() -> Project {
    let root = std::env::temp_dir().join(format!("problem-location-test-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project
        .set_text(&entry, "event start\r\n  中文😀abc\r\n  -> END\r\n".into())
        .unwrap();
    project
}
#[test]
fn unicode_crlf_ranges_are_core_projected_and_invalid_spans_never_clamp() {
    let project = project();
    let file = project.entry.to_string_lossy();
    let location = super::location::project_location(
        &project,
        &file,
        Span::new(2, 5, 1),
        Some(ProblemSourceRole::Target),
        512,
    );
    assert_eq!(location.precision, ProblemPrecision::Span);
    let bytes = location.byte_range.unwrap();
    let chars = location.char_range.unwrap();
    let text = project.document(&project.entry).unwrap();
    assert_eq!(&text[bytes.start..bytes.end], "😀");
    assert_eq!(
        text.chars()
            .skip(chars.start)
            .take(chars.end - chars.start)
            .collect::<String>(),
        "😀"
    );
    for span in [
        Span::new(0, 1, 1),
        Span::new(2, 0, 1),
        Span::new(500, 1, 1),
        Span::new(2, 500, 1),
        Span::new(2, 3, 500),
        Span::new(2, u32::MAX, u32::MAX),
    ] {
        let location = super::location::project_location(
            &project,
            &file,
            span,
            Some(ProblemSourceRole::Target),
            512,
        );
        assert_eq!(
            location.precision,
            ProblemPrecision::Unavailable,
            "{span:?}"
        );
        assert!(
            location.byte_range.is_none()
                && location.char_range.is_none()
                && location.span.is_none()
        );
    }
    let doc = super::location::project_location(&project, &file, Span::new(1, 1, 1), None, 5);
    assert_eq!(doc.precision, ProblemPrecision::Document);
    assert!(doc.byte_range.is_none());
    assert!(doc.excerpt_truncated);
    let outside = super::location::project_location(
        &project,
        "/outside.json",
        Span::new(1, 1, 1),
        Some(ProblemSourceRole::Target),
        512,
    );
    assert_eq!(outside.precision, ProblemPrecision::Unavailable);
    assert!(outside.path.is_none());
}
#[test]
fn many_related_sources_page_without_loss_and_budget_loss_stays_visible() {
    let project = project();
    let mut content = project.compile_current();
    let diagnostic = Diagnostic::error(
        "TEST001",
        &project.entry.to_string_lossy(),
        Span::new(2, 5, 1),
        "中文关联",
    );
    let mut diagnostic = diagnostic;
    diagnostic.related = (0..501)
        .map(|_| {
            (
                project.entry.to_string_lossy().to_string(),
                Span::new(2, 5, 1),
            )
        })
        .collect();
    content.diagnostics = vec![diagnostic];
    let report = project
        .problems_report_with_content(
            &content,
            &project.content_baseline(),
            &ProblemsOptions::default(),
        )
        .unwrap();
    let id = &report.entries[0].id;
    let p1 = report.related_page(id, None, 200).unwrap();
    let p2 = report
        .related_page(id, p1.next_cursor.as_ref(), 200)
        .unwrap();
    let p3 = report
        .related_page(id, p2.next_cursor.as_ref(), 200)
        .unwrap();
    assert_eq!(
        (p1.locations.len(), p2.locations.len(), p3.locations.len()),
        (200, 200, 101)
    );
    assert_eq!(p3.total, 501);
    assert!(!p3.truncated);
    assert!(p3.next_cursor.is_none());
    let small = project
        .problems_report_with_content(
            &content,
            &project.content_baseline(),
            &ProblemsOptions {
                max_related_locations: 3,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(small.truncated && !small.complete);
    let page = small.related_page(&small.entries[0].id, None, 50).unwrap();
    assert!(page.truncated);
    assert_eq!(page.total, 501);
    assert_eq!(page.locations.len(), 3);
    assert!(project
        .problem_location(&small, &small.entries[0].id, Some(4))
        .is_err());
}
#[test]
fn workspace_identity_and_tampered_ranges_cannot_redirect_navigation() {
    let project = project();
    let mut content = project.compile_current();
    content.diagnostics = vec![Diagnostic::error(
        "TEST001",
        &project.entry.to_string_lossy(),
        Span::new(2, 5, 1),
        "一个问题",
    )
    .with_source_role(ProblemSourceRole::Target)];
    let mut report = project
        .problems_report_with_content(
            &content,
            &project.content_baseline(),
            &ProblemsOptions::default(),
        )
        .unwrap();
    let id = report.entries[0].id.clone();
    project.problem_location(&report, &id, None).unwrap();
    report.entries[0].primary.byte_range.as_mut().unwrap().start = 0;
    assert_eq!(
        project
            .problem_location(&report, &id, None)
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
    report.entries[0].primary.path = Some("../escape.wl".into());
    assert_eq!(
        project
            .problem_location(&report, &id, None)
            .unwrap_err()
            .code,
        "INVALID_QUERY"
    );
    assert!(super::location::relative(&project, Path::new("/outside")).is_none());
}
#[test]
fn report_counts_actual_compile_calls_and_pure_queries_compile_zero_times() {
    let project = project();
    COMPILE_RUNS.with(|count| count.set(0));
    let report = project
        .problems_report(&ProblemsOptions::default())
        .unwrap();
    assert_eq!(COMPILE_RUNS.with(|count| count.get()), 1);
    assert_eq!(report.compile_count, 1);
    COMPILE_RUNS.with(|count| count.set(0));
    let _ = report.query(&ProblemQuery::default(), None, 50).unwrap();
    assert_eq!(COMPILE_RUNS.with(|count| count.get()), 0);
    let content = project.compile_current();
    COMPILE_RUNS.with(|count| count.set(0));
    let reused = project
        .problems_report_with_content(
            &content,
            &project.content_baseline(),
            &ProblemsOptions::default(),
        )
        .unwrap();
    assert_eq!(reused.compile_count, 0);
    assert_eq!(COMPILE_RUNS.with(|count| count.get()), 0);
}
#[test]
fn report_bound_ids_reject_initial_related_requests_after_ordinal_collision() {
    let project = project();
    let mut content = project.compile_current();
    content.diagnostics = vec![Diagnostic::error(
        "TEST001",
        &project.entry.to_string_lossy(),
        Span::new(2, 5, 1),
        "旧事实",
    )];
    let old = project
        .problems_report_with_content(
            &content,
            &project.content_baseline(),
            &ProblemsOptions::default(),
        )
        .unwrap();
    content.diagnostics[0].message = "新事实".into();
    let new = project
        .problems_report_with_content(
            &content,
            &project.content_baseline(),
            &ProblemsOptions::default(),
        )
        .unwrap();
    assert_ne!(old.report_version, new.report_version);
    assert_ne!(old.entries[0].id, new.entries[0].id);
    assert_eq!(
        new.related_page(&old.entries[0].id, None, 50)
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
    assert_eq!(
        project
            .problem_location(&new, &old.entries[0].id, None)
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
    assert_eq!(
        new.related_page("p1", None, 50).unwrap_err().code,
        "UNKNOWN_PROBLEM"
    );
}
#[test]
fn with_content_requires_actual_entry_but_accepts_a_missing_entry_diagnostic_snapshot() {
    let mut project = project();
    let other = project.root.join("other.wl");
    let mut document = project.documents[&project.entry].clone();
    document.text = "event other\n  另一入口\n  -> END\n".into();
    project.documents.insert(other.clone(), document);
    let normal = project.compile_problems_snapshot();
    assert_eq!(
        normal.program.files.first().map(String::as_str),
        Some(project.entry.to_str().unwrap())
    );
    project
        .problems_report_with_content(
            &normal,
            &project.content_baseline(),
            &ProblemsOptions::default(),
        )
        .unwrap();
    let different =
        crate::compile_sources_with_options(&other, &project.sources(), project.compile_options());
    assert_eq!(different.sources, normal.sources);
    assert_eq!(
        different.program.files.first().map(String::as_str),
        Some(other.to_str().unwrap())
    );
    assert_eq!(
        project
            .problems_report_with_content(
                &different,
                &project.content_baseline(),
                &ProblemsOptions::default()
            )
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
    project.documents.remove(&project.entry.clone());
    let missing = project.compile_problems_snapshot();
    assert!(missing.has_errors());
    let report = project
        .problems_report_with_content(
            &missing,
            &project.content_baseline(),
            &ProblemsOptions::default(),
        )
        .unwrap();
    assert!(report.content_has_errors);
}
#[cfg(unix)]
#[test]
fn non_utf8_root_is_observation_failure_instead_of_panicking() {
    use std::os::unix::ffi::OsStringExt;
    let mut project = project();
    project.root = std::path::PathBuf::from(std::ffi::OsString::from_vec(
        b"/tmp/worldline-invalid-root-\xff".to_vec(),
    ));
    assert_eq!(
        project.problems_observation_key().unwrap_err().code,
        "OBSERVATION_UNAVAILABLE"
    );
    let result = std::panic::catch_unwind(|| project.problems_report(&ProblemsOptions::default()));
    let report = result.unwrap().unwrap();
    assert!(!report.complete);
}

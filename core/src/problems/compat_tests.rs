use super::*;
use crate::{project::Project, Diagnostic, Span};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// Frozen schema1 response layout before source-context. Do not add new fields here.
#[derive(Serialize, Deserialize)]
struct OldLocation {
    path: Option<String>,
    precision: ProblemPrecision,
    span: Option<Span>,
    byte_range: Option<ProblemRange>,
    char_range: Option<ProblemRange>,
    excerpt: Option<String>,
    excerpt_truncated: bool,
    reason: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct OldEntry {
    id: String,
    domain: ProblemDomain,
    severity: crate::Severity,
    code: String,
    message: String,
    note: Option<String>,
    suggestion: Option<String>,
    primary: OldLocation,
    related_count: usize,
    text_truncated: bool,
}
#[derive(Serialize, Deserialize)]
struct OldReport {
    schema_version: u32,
    report_version: String,
    content_baseline: String,
    source_observation: String,
    language_version: String,
    content_has_errors: bool,
    read_only: bool,
    complete: bool,
    truncated: bool,
    reasons: Vec<String>,
    coverage: Vec<ProblemCoverage>,
    entries: Vec<OldEntry>,
    related: BTreeMap<String, Vec<OldLocation>>,
    limits: ProblemsOptions,
    compile_count: u32,
}
fn report() -> (Project, ProblemsReport) {
    let root = std::env::temp_dir().join(format!("problem-compat-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.set_text(&entry, "event start\n  中文😀abc\n  -> END\n".into()).unwrap();
    let mut content = project.compile_current();
    content.diagnostics = vec![Diagnostic::error(
        "TEST001", &entry.to_string_lossy(), Span::new(2, 5, 1), "兼容证据",
    ).with_source_role(ProblemSourceRole::Target)
        .with_related_source_role(&entry.to_string_lossy(), Span::new(2, 3, 2), ProblemSourceRole::Statement)];
    let report = project.problems_report_with_content(
        &content, &project.content_baseline(), &ProblemsOptions::default(),
    ).unwrap();
    (project, report)
}
#[test]
fn frozen_old_consumer_accepts_new_payload_and_new_consumer_reads_old_read_only() {
    let (project, current) = report();
    let new_json = serde_json::to_vec(&current).unwrap();
    let old: OldReport = serde_json::from_slice(&new_json).unwrap();
    assert_eq!(old.schema_version, 1);
    assert_eq!(old.entries[0].primary.precision, ProblemPrecision::Span);
    assert!(old.entries[0].primary.excerpt.as_ref().unwrap().contains('😀'));
    let old_json = serde_json::to_vec(&old).unwrap();
    assert!(!String::from_utf8_lossy(&old_json).contains("\"context\""));
    let received: ProblemsReport = serde_json::from_slice(&old_json).unwrap();
    assert!(received.entries[0].primary.context.is_none());
    assert!(received.related.values().flatten().all(|location| location.context.is_none()));
    let id = &received.entries[0].id;
    assert_eq!(received.query(&ProblemQuery::default(), None, 0).unwrap().entries.len(), 1);
    assert_eq!(received.related_page(id, None, 0).unwrap().locations.len(), 1);
    for related in [None, Some(0)] {
        assert_eq!(project.problem_location(&received, id, related).unwrap_err().code, "STALE_REPORT");
    }
    assert_eq!(serde_json::to_vec(&received).unwrap(), old_json);
}
#[test]
fn context_and_coherent_location_tampering_never_redirect_navigation() {
    let (project, original) = report();
    let id = &original.entries[0].id;
    project.problem_location(&original, id, None).unwrap();
    project.problem_location(&original, id, Some(0)).unwrap();
    let mutations: [fn(&mut ProblemsReport); 7] = [
        |report| report.entries[0].primary.context.as_mut().unwrap().role = ProblemSourceRole::Expression,
        |report| report.entries[0].primary.context.as_mut().unwrap().version = 2,
        |report| report.entries[0].primary.context.as_mut().unwrap().prefix_clipped = true,
        |report| report.entries[0].primary.context.as_mut().unwrap().hit_byte_range.as_mut().unwrap().start = 0,
        |report| report.entries[0].primary.context = None,
        |report| report.schema_version = 2,
        |report| report.entries[0].primary.excerpt = Some("伪造摘录".into()),
    ];
    for mutate in mutations {
        let mut changed = original.clone();
        mutate(&mut changed);
        assert_eq!(project.problem_location(&changed, id, None).unwrap_err().code, "STALE_REPORT");
    }
    let mut changed = original.clone();
    changed.entries[0].primary = super::location::project_location(
        &project, &project.entry.to_string_lossy(), Span::new(2, 3, 2),
        Some(ProblemSourceRole::Target), 512,
    );
    assert_eq!(project.problem_location(&changed, id, None).unwrap_err().code, "STALE_REPORT");
    changed = original.clone();
    changed.related.get_mut(id).unwrap()[0].context.as_mut().unwrap().role = ProblemSourceRole::Target;
    assert_eq!(project.problem_location(&changed, id, Some(0)).unwrap_err().code, "STALE_REPORT");
}
#[test]
fn streaming_identity_is_stable_and_query_location_never_compile() {
    let (project, current) = report();
    assert_eq!(current.report_version, super::version::of(&current));
    let mut transported: ProblemsReport = serde_json::from_slice(&serde_json::to_vec(&current).unwrap()).unwrap();
    assert_eq!(current.report_version, super::version::of(&transported));
    transported.compile_count += 1;
    assert_eq!(current.report_version, super::version::of(&transported));
    COMPILE_RUNS.with(|counter| counter.set(0));
    current.query(&ProblemQuery::default(), None, 1).unwrap();
    current.related_page(&current.entries[0].id, None, 1).unwrap();
    project.problem_location(&current, &current.entries[0].id, None).unwrap();
    project.problem_location(&current, &current.entries[0].id, Some(0)).unwrap();
    assert_eq!(COMPILE_RUNS.with(|counter| counter.get()), 0);
}
#[test]
fn diagnostic_json_and_precision_enum_do_not_gain_source_fields_or_roles() {
    let diagnostic = Diagnostic::error("TEST001", "world.wl", Span::new(1, 1, 1), "原事实")
        .with_source_role(ProblemSourceRole::Target);
    let object = serde_json::to_value(diagnostic).unwrap();
    let keys: Vec<_> = object.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["code", "file", "message", "note", "related", "severity", "span", "suggestion"]);
    for value in ["target", "expression", "statement", "declaration", "unknown"] {
        assert!(serde_json::from_value::<ProblemPrecision>(serde_json::json!(value)).is_err());
    }
    assert!(!crate::capabilities::feature_capabilities().iter().any(|feature| {
        serde_json::to_string(feature).unwrap().contains(PROBLEM_SOURCE_CONTEXT_CAPABILITY)
    }));
}

use super::*;
use crate::{project::Project, Diagnostic, Span};

fn stress_content(count: usize) -> (Project, crate::CompileResult) {
    let root = std::env::temp_dir().join(format!("problem-budgets-{}", std::process::id()));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    project
        .set_text(&entry, "event e\n  正文😀\n  -> END\n".into())
        .unwrap();
    let mut content = project.compile_current();
    content.diagnostics = (0..count)
        .map(|i| {
            let mut d = Diagnostic::error(
                "TEST001",
                &entry.to_string_lossy(),
                Span::new(2, 3, 2),
                format!("{i}:{}", "中文".repeat(2730)),
            );
            d.note = Some("note".repeat(4096));
            d.suggestion = Some("suggestion".repeat(1600));
            d
        })
        .collect();
    (project, content)
}
#[test]
fn bounded_report_and_page_serialize_within_hard_limits_without_losing_continuation() {
    let (project, content) = stress_content(100);
    let baseline = project.content_baseline();
    let full = project
        .problems_report_with_content(&content, &baseline, &ProblemsOptions::default())
        .unwrap();
    assert_eq!(full.entries.len(), 100);
    let mut count = 0;
    let mut cursor = None;
    loop {
        let page = full
            .query(&ProblemQuery::default(), cursor.as_ref(), 200)
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 1024 * 1024);
        assert!(!page.entries.is_empty());
        count += page.entries.len();
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(count, 100);
    for max_report_bytes in [8 * 1024, 64 * 1024, 1024 * 1024] {
        let small = project
            .problems_report_with_content(
                &content,
                &baseline,
                &ProblemsOptions {
                    max_report_bytes,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(small.truncated && !small.complete);
        assert!(small.reasons.contains(&"report_bytes".to_owned()));
        assert!(serde_json::to_vec(&small).unwrap().len() <= max_report_bytes);
    }
}
#[test]
fn clipping_and_dedup_do_not_merge_different_facts_or_related_sources() {
    let (project, mut content) = stress_content(2);
    let first = content.diagnostics[0].clone();
    content.diagnostics.push(first.clone());
    let mut different = first;
    different
        .related
        .push((project.entry.to_string_lossy().into(), Span::new(2, 5, 1)));
    content.diagnostics.push(different);
    let result = project
        .problems_report_with_content(
            &content,
            &project.content_baseline(),
            &ProblemsOptions {
                max_text_bytes: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(result.entries.len(), 3);
    assert!(result.entries.iter().all(|e| e.text_truncated));
    assert_eq!(
        result
            .entries
            .iter()
            .filter(|e| e.related_count == 1)
            .count(),
        1
    );
}
#[test]
fn request_dtos_reject_unknown_fields_and_invalid_budget_or_compile_baseline() {
    assert!(serde_json::from_str::<ProblemsOptions>(r#"{"unknown":1}"#).is_err());
    assert!(serde_json::from_str::<ProblemQuery>(r#"{"severity":"fatal"}"#).is_err());
    assert!(serde_json::from_str::<ProblemCursor>(
        r#"{"report_version":"v","query_key":"q","offset":0,"extra":true}"#
    )
    .is_err());
    let (project, mut content) = stress_content(1);
    assert_eq!(
        project
            .problems_report(&ProblemsOptions {
                max_entries: 20_001,
                ..Default::default()
            })
            .unwrap_err()
            .code,
        "BUDGET_EXCEEDED"
    );
    assert_eq!(
        project
            .problems_report_with_content(
                &content,
                "different baseline",
                &ProblemsOptions::default()
            )
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
    content.options.object_refs = !content.options.object_refs;
    assert_eq!(
        project
            .problems_report_with_content(
                &content,
                &project.content_baseline(),
                &ProblemsOptions::default()
            )
            .unwrap_err()
            .code,
        "STALE_REPORT"
    );
}

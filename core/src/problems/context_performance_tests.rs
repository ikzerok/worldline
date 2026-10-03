//! 只在 lead 显式提供本轮冻结 fixture 时运行，不自动查找研究目录。
use super::*;
use crate::project::Project;
use std::{path::Path, time::Instant};

#[test]
#[ignore = "requires WORLDLINE_PROBLEM_CONTEXT_SCALE and isolated fixed debug profile"]
fn frozen_context_navigation_reports_cost_without_compiling() {
    let root = std::env::var("WORLDLINE_PROBLEM_CONTEXT_SCALE")
        .expect("provide frozen 129-source fixture");
    let project = Project::open(Path::new(&root)).unwrap();
    let sources = project.sources();
    assert_eq!(sources.len(), 129);
    assert_eq!(sources.values().map(String::len).sum::<usize>(), 2_076_104);
    let baseline = project.content_baseline();
    let report = project
        .problems_report(&ProblemsOptions::default())
        .unwrap();
    assert_eq!(report.entries.len(), 3072);
    assert!(report.complete && !report.truncated);
    let bytes = serde_json::to_vec(&report).unwrap().len();
    assert!(bytes <= 32 * 1024 * 1024);
    project
        .problem_location(&report, &report.entries[0].id, None)
        .unwrap();
    COMPILE_RUNS.with(|counter| counter.set(0));
    let mut milliseconds = Vec::new();
    for index in 0..10 {
        let entry = &report.entries[index * (report.entries.len() - 1) / 9];
        let start = Instant::now();
        let location = project.problem_location(&report, &entry.id, None).unwrap();
        milliseconds.push(start.elapsed().as_secs_f64() * 1000.);
        assert_eq!(location.precision, ProblemPrecision::Span);
        assert_eq!(
            location.context.unwrap().visibility,
            ProblemContextVisibility::Full
        );
    }
    assert_eq!(COMPILE_RUNS.with(|counter| counter.get()), 0);
    assert_eq!(project.content_baseline(), baseline);
    eprintln!("context report_bytes={bytes} location_ms={milliseconds:?}; location/query are actions, not per-frame layout");
}

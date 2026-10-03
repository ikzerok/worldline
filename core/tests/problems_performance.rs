//! 手动性能门槛；需要明确给定冻结规模稿，不自动读取研究目录。
use std::time::Instant;
use worldline_core::{project::Project, *};
fn percentile(samples: &mut [f64], fraction: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[((samples.len() as f64 * fraction).ceil() as usize).saturating_sub(1)]
}
fn memory_kib(name: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|line| {
                line.strip_prefix(name)
                    .and_then(|s| s.split_whitespace().next()?.parse().ok())
            })
        })
        .unwrap_or(0)
}
#[test]
#[ignore = "requires WORLDLINE_PROBLEMS_SCALE fixture, fixed dev profile and isolated process"]
fn report_and_query_frozen_scale_budget() {
    let path = std::env::var("WORLDLINE_PROBLEMS_SCALE")
        .expect("set WORLDLINE_PROBLEMS_SCALE to the frozen 331-source fixture");
    let mut project = Project::open(std::path::Path::new(&path)).unwrap();
    let sources = project.sources();
    let bytes: usize = sources.values().map(String::len).sum();
    assert_eq!(sources.len(), 331);
    assert_eq!(bytes, 3_425_183);
    assert_eq!(
        project
            .compile()
            .analysis
            .catalog
            .objects
            .iter()
            .filter(|o| o.target.kind == "entity")
            .count(),
        1920
    );
    let baseline = project.content_baseline();
    let rss_before = memory_kib("VmRSS:");
    let options = ProblemsOptions::default();
    let mut report = project.problems_report(&options).unwrap();
    let mut build_ms = Vec::new();
    for _ in 0..10 {
        let now = Instant::now();
        report = project.problems_report(&options).unwrap();
        build_ms.push(now.elapsed().as_secs_f64() * 1000.);
    }
    assert!(
        build_ms.iter().all(|v| *v <= 5000.),
        "report ms {build_ms:?}"
    );
    assert_eq!(baseline, project.content_baseline());
    // Independent 5000-fact stress report, not a claim of 5000 source errors in the scale fixture.
    let prototype = report
        .entries
        .first()
        .expect("fixture contains registered-document diagnostics")
        .clone();
    report.entries = (0..5000)
        .map(|i| {
            let mut e = prototype.clone();
            e.id = format!("stress-{i}");
            e.message = format!("问题 {i}: 长中文 😀；处理作者当前稿");
            e.severity = if i % 2 == 0 {
                Severity::Error
            } else {
                Severity::Warning
            };
            e.related_count = 0;
            e
        })
        .collect();
    report.related.clear();
    let mut query_ms = Vec::new();
    for i in 0..20 {
        let query = ProblemQuery {
            text: if i % 2 == 0 {
                "中文".into()
            } else {
                "当前稿".into()
            },
            severities: if i % 3 == 0 {
                vec![Severity::Error]
            } else {
                vec![]
            },
            ..Default::default()
        };
        let now = Instant::now();
        let first = report.query(&query, None, 200).unwrap();
        let second = report
            .query(&query, first.next_cursor.as_ref(), 200)
            .unwrap();
        assert_eq!(second.entries.len(), 200);
        query_ms.push(now.elapsed().as_secs_f64() * 1000.);
    }
    let build_p50 = percentile(&mut build_ms, 0.5);
    let build_p95 = percentile(&mut build_ms, 0.95);
    let query_p95 = percentile(&mut query_ms, 0.95);
    assert!(query_p95 <= 50., "query+page p95 {query_p95} ms");
    let peak = memory_kib("VmHWM:");
    let incremental = peak.saturating_sub(rss_before);
    assert!(
        incremental <= 128 * 1024,
        "incremental RSS KiB {incremental}"
    );
    println!(
        "{}",
        serde_json::json!({"source_files":331,"source_bytes":bytes,"entities":1920,"baseline":baseline,"build_samples_ms":build_ms,"build_p50_ms":build_p50,"build_p95_ms":build_p95,"synthetic_problems":5000,"query_pair_samples_ms":query_ms,"query_pair_p95_ms":query_p95,"rss_before_kib":rss_before,"rss_peak_kib":peak,"rss_increment_kib":incremental,"profile":"dev opt0 debug0 incremental0 jobs2"})
    );
}

#[test]
#[ignore = "isolated real-diagnostic construction stress under the frozen dev profile"]
fn construct_at_least_5000_real_problems_with_single_compile() {
    let root = std::env::temp_dir().join(format!(
        "worldline-problems-real-stress-{}",
        std::process::id()
    ));
    let mut project = Project::new(&root);
    let entry = project.entry.clone();
    project.documents.retain(|path, _| path == &entry);
    let mut source = String::new();
    for i in 0..5000 {
        source.push_str(&format!("event issue_{i}\n  -> missing_{i}\n"));
    }
    project.set_text(&entry, source).unwrap();
    let rss_before = memory_kib("VmRSS:");
    let now = Instant::now();
    let report = project
        .problems_report(&ProblemsOptions::default())
        .unwrap();
    let elapsed = now.elapsed().as_secs_f64() * 1000.;
    assert!(
        report.entries.len() >= 5000,
        "only {} problems",
        report.entries.len()
    );
    assert_eq!(report.compile_count, 1);
    let rss_increment = memory_kib("VmHWM:").saturating_sub(rss_before);
    println!(
        "{}",
        serde_json::json!({"real_problem_count":report.entries.len(),"source_lines":project.document(&entry).unwrap().lines().count(),"report_build_ms":elapsed,"compile_count":report.compile_count,"rss_increment_kib":rss_increment})
    );
    assert!(
        elapsed <= 5000.,
        "report build {elapsed}ms exceeds frozen 5000ms budget"
    );
    assert!(rss_increment <= 128 * 1024);
}

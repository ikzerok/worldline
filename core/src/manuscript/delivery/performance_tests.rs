//! 仅显式外部固定1000章负载；不生成、缩减、修复或替换被测作品。
use super::*;
use crate::manuscript::validate_review_source;
use crate::project::Project;
use std::{path::Path, time::Instant};

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn percentiles(mut values: Vec<f64>) -> serde_json::Value {
    assert!(!values.is_empty());
    values.sort_by(f64::total_cmp);
    serde_json::json!({"samples":values.len(),"p50":values[values.len()/2],"p95":values[((values.len()*95).div_ceil(100)).saturating_sub(1)],"max":values.last().unwrap()})
}
fn fnv(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

#[test]
#[ignore = "requires WORLDLINE_MANUSCRIPT_DELIVERY_FIXTURE frozen 1000-chapter external input"]
fn frozen_manuscript_1000_delivery_release_profile() {
    // ignored探针可随debug全包编译；显式执行时仍拒绝把debug计时当release结果。
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "性能测量必须使用release，不能将debug结果冒作发行表现"
    );
    let root = std::env::var("WORLDLINE_MANUSCRIPT_DELIVERY_FIXTURE")
        .expect("必须明确指定固定1000章外部夹具");
    let manifest_bytes = std::fs::read(Path::new(&root).join("fixture-manifest.json"))
        .expect("缺少冻结fixture-manifest.json");
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest["fixture"], "worldline-manuscript-delivery-1000-v1");
    assert_eq!(manifest["chapter_occurrences"], 1000);
    assert_eq!(manifest["unique_sources"], 900);
    let project = Project::open_read_only(Path::new(&root)).unwrap();
    let baseline = project.content_baseline();
    let source_files = project.sources().len();
    let source_bytes = project.sources().values().map(String::len).sum::<usize>();
    assert_eq!(source_files, 11);
    let request = ManuscriptDeliveryRequest::new(ManuscriptQueryRequest {
        manuscript_id: "book".into(),
        ..Default::default()
    });
    let mut cold_snapshot = Vec::new();
    crate::problems::COMPILE_RUNS.with(|count| count.set(0));
    for _ in 0..3 {
        let start = Instant::now();
        let query = project.manuscript_query_snapshot(&[], &[]).unwrap();
        cold_snapshot.push(ms(start));
        assert_eq!(query.query(&request.query).unwrap().matching_chapters, 1000);
    }
    assert_eq!(crate::problems::COMPILE_RUNS.with(|count| count.get()), 3);
    let query = Arc::new(project.manuscript_query_snapshot(&[], &[]).unwrap());
    let mut scope_build = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        let scope = ManuscriptDeliverySnapshot::new(query.clone(), &request).unwrap();
        scope_build.push(ms(start));
        assert_eq!(scope.scope().selected_occurrences, 1000);
        assert_eq!(scope.scope().unique_sources, 900);
        assert_eq!(scope.scope().repeated_source_occurrences, 100);
    }
    crate::problems::COMPILE_RUNS.with(|count| count.set(0));
    let mut batches = Vec::new();
    let mut totals = Vec::new();
    let mut report = None;
    let mut checkpoints = 0usize;
    for _ in 0..3 {
        let snapshot = ManuscriptDeliverySnapshot::new(query.clone(), &request).unwrap();
        let mut job = ManuscriptDeliveryJob::new(snapshot).unwrap();
        let total = Instant::now();
        loop {
            let start = Instant::now();
            let completed = job.advance(1).unwrap();
            batches.push(ms(start));
            checkpoints += 1;
            if let Some(value) = completed {
                assert!(value.complete());
                report = Some(value);
                break;
            }
        }
        assert_eq!(job.progress().completed, 1000);
        totals.push(ms(total));
    }
    let report = report.unwrap();
    assert_eq!(report.chapters().len(), 1000);
    assert_eq!(checkpoints, 3000);
    let mut warm_pages = Vec::new();
    let mut return_preparation = Vec::new();
    for index in 0..300 {
        let start = Instant::now();
        let offset = index * 8 % 1000;
        let mut page_request = request.query.clone();
        page_request.offset = offset;
        page_request.limit = 8;
        let page = query.query(&page_request).unwrap();
        assert!(!page.rows.is_empty());
        let review = report.chapters()[offset].review.as_ref().unwrap();
        warm_pages.push(ms(start));
        let source = review
            .nodes
            .iter()
            .find_map(|node| node.source.as_ref())
            .unwrap();
        let start = Instant::now();
        validate_review_source(&query.content.0, review, source).unwrap();
        return_preparation.push(ms(start));
    }
    let mut cancel = Vec::new();
    for _ in 0..31 {
        let snapshot = ManuscriptDeliverySnapshot::new(query.clone(), &request).unwrap();
        let mut job = ManuscriptDeliveryJob::new(snapshot).unwrap();
        assert!(job.advance(1).unwrap().is_none());
        let start = Instant::now();
        job.cancel();
        assert_eq!(job.advance(1).unwrap_err().code, "CANCELLED");
        cancel.push(ms(start));
    }
    let mut budget_errors = 0;
    for nodes in [1, 100] {
        let mut limited = request.clone();
        limited.limits.nodes = nodes;
        let snapshot = ManuscriptDeliverySnapshot::new(query.clone(), &limited).unwrap();
        let error = generate_manuscript_delivery(snapshot, &mut |_| true).unwrap_err();
        assert_eq!(error.code, "BUDGET_EXCEEDED");
        budget_errors += 1;
    }
    assert_eq!(crate::problems::COMPILE_RUNS.with(|count| count.get()), 0);
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(
        std::fs::read(Path::new(&root).join("fixture-manifest.json")).unwrap(),
        manifest_bytes
    );
    println!(
        "MANUSCRIPT_DELIVERY_FIXED {}",
        serde_json::json!({
            "fixture":manifest["fixture"],"fixture_root":root,"fixture_manifest_fnv64":fnv(&manifest_bytes),
            "baseline":baseline,"query_snapshot":query.key(),"source_files":source_files,"source_bytes":source_bytes,
            "occurrences":1000,"unique_sources":900,"projection_bytes":report.usage().review_bytes,
            "markdown_bytes":report.usage().markdown_bytes,"nodes":report.usage().nodes,
            "snapshot_uncached_ms":percentiles(cold_snapshot),"scope_ms":percentiles(scope_build),
            "complete_generation_ms":percentiles(totals),"single_chapter_step_ms":percentiles(batches),
            "warm_query_page_ms":percentiles(warm_pages),"return_source_validation_ms":percentiles(return_preparation),
            "cancel_after_one_chapter_ms":percentiles(cancel),"progress_checkpoints":checkpoints,"budget_errors":budget_errors,
            "warm_compile_count":0,"measurement":"release headless CPU; OS cache not cleared; no GUI FPS or physical IME claim"
        })
    );
}

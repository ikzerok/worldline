//! 固定外部夹具的测量入口；测试本身不查找或拷贝研究目录。
use super::*;
use crate::project::Project;
use std::time::Instant;

fn percentiles(mut samples: Vec<f64>) -> (f64, f64, f64) {
    samples.sort_by(f64::total_cmp);
    (
        samples[samples.len() / 2],
        samples[((samples.len() * 95).div_ceil(100)).saturating_sub(1)],
        *samples.last().unwrap(),
    )
}
#[test]
#[ignore = "requires WORLDLINE_CATALOG_SCOPE_FIXTURE frozen same-load input"]
fn frozen_catalog_scope_build_paging_relations_cancel_report_exact_load() {
    let root =
        std::env::var("WORLDLINE_CATALOG_SCOPE_FIXTURE").expect("explicit frozen fixture required");
    let project = Project::open(std::path::Path::new(&root)).unwrap();
    let baseline = project.content_baseline();
    let source_files = project.sources().len();
    let source_bytes = project.sources().values().map(String::len).sum::<usize>();
    let maps = project.map_index();
    let scene_nodes = maps
        .maps
        .values()
        .filter_map(|map| map.scene.as_ref())
        .map(|scene| scene.nodes.len())
        .sum::<usize>();
    let mut cold = Vec::new();
    crate::problems::COMPILE_RUNS.with(|count| count.set(0));
    for _ in 0..3 {
        let start = Instant::now();
        let snapshot = project
            .catalog_scope_snapshot(&CatalogQuery::default(), 100_000)
            .unwrap();
        cold.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(snapshot.query().snapshot, baseline);
    }
    assert_eq!(crate::problems::COMPILE_RUNS.with(|count| count.get()), 3);
    let snapshot = project
        .catalog_scope_snapshot(&CatalogQuery::default(), 100_000)
        .unwrap();
    let wire_bytes = serde_json::to_vec(&snapshot).unwrap().len();
    crate::problems::COMPILE_RUNS.with(|count| count.set(0));
    let mut warm = Vec::new();
    let mut relations = Vec::new();
    for index in 0..300 {
        let start = Instant::now();
        let offset = index * 50 % snapshot.query().total().max(1);
        let page = snapshot.query().page(offset, 50).unwrap();
        warm.push(start.elapsed().as_secs_f64() * 1000.0);
        if let Some(item) = page.items.first() {
            let start = Instant::now();
            snapshot.query_relations(&item.target, Default::default());
            relations.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    assert_eq!(crate::problems::COMPILE_RUNS.with(|count| count.get()), 0);
    let mut checkpoints = 0;
    let start = Instant::now();
    let cancel =
        project.catalog_scope_snapshot_cancellable(&CatalogQuery::default(), 100_000, || {
            checkpoints += 1;
            checkpoints >= 3
        });
    let cancel_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(cancel.unwrap_err(), QueryError::Cancelled);
    assert_eq!(project.content_baseline(), baseline);
    let cold = percentiles(cold);
    let warm = percentiles(warm);
    let relations = if relations.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        percentiles(relations)
    };
    println!("CATALOG_SCOPE_FIXED {{\"baseline\":\"{baseline}\",\"source_files\":{source_files},\"source_bytes\":{source_bytes},\"scene_nodes\":{scene_nodes},\"matches\":{},\"bindings\":{},\"wire_bytes\":{wire_bytes},\"cold_ms\":{:?},\"warm_page_ms\":{:?},\"relation_ms\":{:?},\"cancel_ms\":{cancel_ms},\"cancel_checkpoints\":{checkpoints},\"warm_compile_count\":0}}",snapshot.query().total(),snapshot.placements().len(),[cold.0,cold.1,cold.2],[warm.0,warm.1,warm.2],[relations.0,relations.1,relations.2]);
}

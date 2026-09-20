//! D5 关系查询测量：工程载入与查询分别计时，输出 JSONL 原始样本。
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;
use worldline_core::catalog::TargetRef;
use worldline_core::project::Project;
use worldline_core::{RelationQueryDirection, RelationQueryOptions};

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    assert!(
        arguments.len() >= 2,
        "用法：relations_profile 工程目录 [暖态轮数]"
    );
    let samples: usize = arguments
        .get(2)
        .map_or(200, |v| v.parse().expect("轮数应为整数"));
    assert!(samples > 0, "暖态轮数必须大于零");
    let start = Instant::now();
    let project = Project::open(Path::new(&arguments[1])).expect("工程打开失败");
    assert!(
        !project
            .authoring_diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.severity == worldline_core::Severity::Error),
        "工作区诊断必须先修复：{:?}",
        project.authoring_diagnostics()
    );
    let content = worldline_core::compile_sources_with_options(
        &project.entry,
        &project.sources(),
        project.compile_options(),
    );
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(
        !content.has_errors(),
        "工程编译失败：{:?}",
        content.diagnostics
    );
    let catalog = &content.analysis.catalog;
    assert_eq!(catalog.entities.len(), 1000, "D5 对象数不匹配");
    assert_eq!(catalog.relations.len(), 3000, "D5 关系数不匹配");
    println!(
        "{}",
        serde_json::json!({
            "kind": "load", "elapsed_ms": load_ms,
            "objects": catalog.entities.len(), "relations": catalog.relations.len(),
            "language_version": project.language_version(),
            "baseline": project.content_baseline(),
            "note": "新进程首次打开及编译；未清空操作系统文件缓存"
        })
    );
    for (name, id, depth, direction, filter) in [
        (
            "local_both",
            "object_0500",
            1,
            RelationQueryDirection::Both,
            None,
        ),
        (
            "local_outgoing",
            "object_0500",
            2,
            RelationQueryDirection::Outgoing,
            None,
        ),
        (
            "local_filtered",
            "object_0500",
            2,
            RelationQueryDirection::Both,
            Some("records"),
        ),
        (
            "hub_incoming",
            "object_0000",
            1,
            RelationQueryDirection::Incoming,
            None,
        ),
        (
            "hub_depth_two",
            "object_0000",
            2,
            RelationQueryDirection::Both,
            None,
        ),
        (
            "hub_filtered",
            "object_0000",
            2,
            RelationQueryDirection::Both,
            Some("connects"),
        ),
    ] {
        let target = TargetRef {
            kind: "entity".into(),
            id: id.into(),
        };
        for sample in 0..=samples {
            let start = Instant::now();
            let result = black_box(catalog).query_relations(
                black_box(&target),
                RelationQueryOptions {
                    depth,
                    direction,
                    relation_type: filter.map(str::to_owned),
                    ..Default::default()
                },
            );
            black_box(&result);
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            assert!(result.nodes.len() <= 250 && result.edges.len() <= 500);
            assert!(!result.edges.is_empty(), "查询应返回显式关系");
            assert!(
                result.edges.iter().all(|edge| {
                    result.nodes.iter().any(|node| node.target == edge.from_ref)
                        && result.nodes.iter().any(|node| node.target == edge.to_ref)
                }),
                "返回的边必须有可见端点"
            );
            println!(
                "{}",
                serde_json::json!({
                    "kind": "query", "case": name, "sample": sample,
                    "phase": if sample == 0 { "first_case_query" } else { "warm" },
                    "elapsed_ms": elapsed_ms, "nodes": result.nodes.len(),
                    "edges": result.edges.len(), "truncated": result.truncated
                })
            );
        }
    }
}

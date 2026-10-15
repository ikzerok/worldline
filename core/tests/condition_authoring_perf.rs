//! 可复现 release 探针；输出原样样本，不预设成功阈值。
#[path = "condition_authoring/common.rs"]
mod common;
use common::*;
use serde_json::{json, Value};
use std::time::Instant;
use worldline_core::{condition_authoring::*, project::Project};

fn samples(mut run: impl FnMut()) -> Vec<u64> {
    (0..10)
        .map(|_| {
            let started = Instant::now();
            run();
            started.elapsed().as_micros() as u64
        })
        .collect()
}
fn summary(values: Vec<u64>, scope: &str, compile_each_sample: bool) -> Value {
    let mut sorted = values.clone();
    sorted.sort_unstable();
    json!({"samples_us":values,"median_us":(sorted[4] as f64 + sorted[5] as f64) / 2.0,
        "p95_nearest_rank_us":sorted[9],"sample_count":10,"scope":scope,
        "compile_each_sample":compile_each_sample,"compiled_reuse_inside_operation":true})
}
fn balanced(leaves: usize) -> (String, ConditionNode) {
    if leaves == 1 {
        ("true".into(), ConditionNode::Bool { value: true })
    } else {
        let (left_source, left) = balanced(leaves / 2);
        let (right_source, right) = balanced(leaves - leaves / 2);
        (
            format!("({left_source}) and ({right_source})"),
            ConditionNode::And {
                left: Box::new(left),
                right: Box::new(right),
            },
        )
    }
}
fn change_first(tree: &mut ConditionNode) {
    match tree {
        ConditionNode::Not { child } => change_first(child),
        ConditionNode::And { left, .. } => change_first(left),
        ConditionNode::Bool { value } => *value = false,
        _ => unreachable!(),
    }
}
fn condition_probe() -> Value {
    let (source, tree) = balanced(64);
    let input = input(&format!("not ({source})"));
    let mut replacement = ConditionNode::Not {
        child: Box::new(tree),
    };
    change_first(&mut replacement);
    let replacement = Some(replacement);
    let fixture = Fixture::new(
        &format!("event start after {}\n  -> END\n", input.source),
        "1.12",
    );
    let cold = samples(|| {
        let project = Project::open_read_only(&fixture.root).unwrap();
        let compiled = project.compile_read_only().unwrap();
        let projection = project
            .condition_draft_from_compile(&compiled, &input)
            .unwrap();
        assert_eq!(projection.support, ConditionSupport::Supported);
        assert!(
            project
                .preview_condition_draft(&compiled, &input, &replacement)
                .unwrap()
                .changed
        );
    });
    let compiled = fixture.compiled();
    let hot = samples(|| {
        let projection = fixture
            .project
            .condition_draft_from_compile(&compiled, &input)
            .unwrap();
        assert_eq!(projection.support, ConditionSupport::Supported);
        assert!(
            fixture
                .project
                .preview_condition_draft(&compiled, &input, &replacement)
                .unwrap()
                .changed
        );
    });
    json!({"nodes":128,"input_bytes":input.source.len(),
        "cold":summary(cold,"Project::open_read_only + compile_read_only + projection + changed preview",true),
        "hot":summary(hot,"same Project/CompileResult; projection + changed preview",false)})
}
fn candidate_probe(count: usize) -> Value {
    let mut source = String::new();
    for index in 0..count {
        source.push_str(&format!("let value_{index:05} = false\n"));
    }
    source.push_str("event start\n  -> END\n");
    let fixture = Fixture::new(&source, "1.12");
    let query = ConditionCandidateQuery {
        schema_version: 1,
        kind: ConditionCandidateKind::Variable,
        text: String::new(),
        offset: count / 2,
        limit: 100,
    };
    let cold = samples(|| {
        let project = Project::open_read_only(&fixture.root).unwrap();
        let compiled = project.compile_read_only().unwrap();
        let page = project
            .condition_candidates_from_compile(&compiled, &query)
            .unwrap();
        assert_eq!(page.total, count);
        assert_eq!(page.items.len(), 100);
    });
    let compiled = fixture.compiled();
    let hot = samples(|| {
        let page = fixture
            .project
            .condition_candidates_from_compile(&compiled, &query)
            .unwrap();
        assert_eq!(page.total, count);
        assert_eq!(page.items.len(), 100);
    });
    json!({"candidate_count":count,"source_bytes":source.len(),"page_size":100,"offset":query.offset,
        "cold":summary(cold,"Project::open_read_only + compile_read_only + typed candidate page",true),
        "hot":summary(hot,"same Project/CompileResult; typed candidate page",false)})
}

#[test]
#[ignore = "release performance probe: 10 cold/hot samples, emits machine JSON; no fixed threshold"]
fn condition_authoring_release_probe() {
    if cfg!(debug_assertions) {
        panic!("请仅使用 --release 执行性能探针");
    }
    let report = json!({"schema_version":1,"probe":"condition_authoring_v036","profile":"release",
        "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "parallelism":std::thread::available_parallelism().map(|n| n.get()).ok(),
        "cold_definition":"fresh Project and compile per sample; filesystem/OS cache is not forcibly cleared",
        "hot_definition":"prebuilt Project and CompileResult; no compile in measured sample",
        "threshold":null,"limitations":["machine/CPU/Rust version and Git SHA must be recorded by runner",
            "ten samples provide descriptive timing, not a latency guarantee","no UI frame/IME/worker scheduling measurement"],
        "conditions":condition_probe(),"candidates":[candidate_probe(1000),candidate_probe(10000)]});
    println!("{report}");
}

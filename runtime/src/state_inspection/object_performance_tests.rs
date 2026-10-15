//! 手动 release API 微基准；不测编译、进程冷缓存、原生窗口或输入到绘制延迟。
use super::*;
use crate::{Story, Value};
use serde::Serialize;
use serde_json::json;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use worldline_core::{compile_source_with_options, CompileOptions, CompileResult, TargetRef};

const SOURCE: &str =
    include_str!("../../tests/fixtures/object_state_inspection_v036/performance.wl");
const SOURCE_SHA256: &str = "39e4802815c01bafe7645b1c5e0f05d3eb84e2d7fd0648b1876ffd83e3e1e2a4";
const SOURCE_FNV1A64: u64 = 0xd86d3be59c09f2bf;
const SEED: u64 = 4242;
const SAMPLES: usize = 10;

#[derive(Serialize)]
struct Sample {
    sample: usize,
    query_ns: u64,
    serialization_ns: u64,
    output_bytes: usize,
    stamp: InspectionStamp,
    owner: InspectionOwner,
    status: InspectionStatus,
    total_items: usize,
    total_matches: usize,
    returned_rows: usize,
    next_offset: Option<usize>,
    first_observation: Option<u64>,
    previous_observation: Option<u64>,
    current_observation: Option<u64>,
    history_omitted: bool,
    state_definition_rows: usize,
    metadata_verified: bool,
    readonly_verified: bool,
}

fn source_checksum() -> u64 {
    SOURCE.bytes().fold(0xcbf29ce484222325, |value, byte| {
        (value ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn query(story: &Story<'_>) -> StateInspectionQuery {
    StateInspectionQuery {
        owner: Some(TargetRef::new("world", "w")),
        group: Some(InspectionGroup::State),
        limit: 100,
        expected_stamp: Some(story.inspection_stamp()),
        ..Default::default()
    }
}

fn ready(compiled: &CompileResult) -> Story<'_> {
    let mut story = Story::new_with_seed(&compiled.program, &compiled.analysis, SEED).unwrap();
    story.continue_story().unwrap();
    assert!(story.is_paused());
    story
}

fn readonly_state(story: &Story<'_>) -> serde_json::Value {
    json!({
        "save": story.save().unwrap(), "trace": story.replay_trace(),
        "rng": story.rng.get(), "visits": story.visits(), "turns": story.turns(),
        "choices": story.choice_presentations(), "history": story.state_history(),
        "state_actions": story.state_action_evidence(), "conditions": story.choice_evidence(),
        "stamp": story.inspection_stamp(), "state_view": story.state_view(),
        "variable_capture_enabled": story.action_capture.variables.captured,
    })
}

fn verify_page(page: &StateInspectionPage, compiled: &CompileResult) {
    assert_eq!(
        page.owner.as_ref().unwrap().target,
        TargetRef::new("world", "w")
    );
    assert_eq!(page.owner.as_ref().unwrap().display, "测量对象");
    assert_eq!(page.status, InspectionStatus::Choice);
    assert_eq!(page.total_items, 107);
    assert_eq!(page.total_matches, 107);
    assert_eq!(page.items.len(), 100);
    assert_eq!(page.items.first().unwrap().key.name, "state_000");
    assert_eq!(page.items.last().unwrap().key.name, "state_099");
    assert_eq!(page.next_offset, Some(100));
    assert_eq!(page.first_observation, Some(1));
    assert_eq!(page.previous_observation, None);
    assert_eq!(page.current_observation, Some(1));
    assert!(!page.history_omitted);
    for item in &page.items {
        let info = &compiled.analysis.catalog.states[&item.key.name];
        let definition = item.state_definition.as_ref().unwrap();
        assert_eq!(item.key.group, InspectionGroup::State);
        assert_eq!(definition.owner, info.target);
        assert_eq!(definition.owner, TargetRef::new("world", "w"));
        assert_eq!(definition.display, info.display);
        assert_eq!(definition.initial.status, InspectionCellStatus::Present);
        assert_eq!(
            definition.initial.value,
            Some(Value::TagSet(info.tags.clone()))
        );
        assert!(!definition.initial.truncated);
        let source = item.source.as_ref().unwrap();
        assert_eq!(
            source.kind,
            worldline_core::state_inspection_source::DeclarationKind::State
        );
        assert_eq!(source.id, info.id);
        assert_eq!(source.file, info.file);
        assert_eq!(source.line, info.line);
        assert_eq!(item.first, item.current);
        assert_eq!(item.previous.status, InspectionCellStatus::Unrecorded);
    }
    let first = &page.items[0];
    assert_ne!(
        first.state_definition.as_ref().unwrap().initial,
        first.first
    );
    assert_eq!(
        first.first.value,
        Some(Value::TagSet(vec!["changed".into()]))
    );
}

fn nanoseconds(started: Instant) -> u64 {
    started.elapsed().as_nanos().try_into().unwrap()
}

fn measure(
    story: &Story<'_>,
    compiled: &CompileResult,
    query: &StateInspectionQuery,
    sample: usize,
) -> Sample {
    let before = readonly_state(story);
    // 仅这一段计入 query_ns；序列化、只读快照和正确性检查在计时外。
    let started = Instant::now();
    let page = story.inspect_state(query).unwrap();
    let query_ns = nanoseconds(started);
    let serialization_started = Instant::now();
    let encoded = serde_json::to_vec(&page).unwrap();
    let serialization_ns = nanoseconds(serialization_started);
    assert!(encoded.len() <= MAX_INSPECTION_OUTPUT_BYTES);
    verify_page(&page, compiled);
    assert_eq!(Some(page.stamp), query.expected_stamp);
    assert_eq!(readonly_state(story), before);
    Sample {
        sample,
        query_ns,
        serialization_ns,
        output_bytes: encoded.len(),
        stamp: page.stamp,
        owner: page.owner.unwrap(),
        status: page.status,
        total_items: page.total_items,
        total_matches: page.total_matches,
        returned_rows: page.items.len(),
        next_offset: page.next_offset,
        first_observation: page.first_observation,
        previous_observation: page.previous_observation,
        current_observation: page.current_observation,
        history_omitted: page.history_omitted,
        state_definition_rows: page
            .items
            .iter()
            .filter(|item| item.state_definition.is_some())
            .count(),
        metadata_verified: true,
        readonly_verified: true,
    }
}

fn summary(samples: &[Sample]) -> serde_json::Value {
    let mut times: Vec<_> = samples.iter().map(|sample| sample.query_ns).collect();
    times.sort_unstable();
    assert_eq!(times.len(), SAMPLES);
    json!({
        "count": times.len(),
        "min_ns": times[0],
        "median_ns": times[4].saturating_add(times[5]) / 2,
        "p95_ns": times[(95 * times.len()).div_ceil(100) - 1],
        "max_ns": times[times.len() - 1],
        "percentile_method": "nearest_rank; with 10 samples p95 is the maximum",
    })
}

#[test]
#[ignore = "手动release API微基准：--release --ignored --nocapture --test-threads=1"]
fn object_inspection_release_query_probe() {
    if cfg!(debug_assertions) {
        panic!("本探针只接受release构建，不能把debug结果用于release报告");
    }
    // SHA-256由固定夹具字节离线计算；校验防止修改夹具后沿用旧摘要。
    assert_eq!(
        source_checksum(),
        SOURCE_FNV1A64,
        "夹具已变化，请重算SHA-256与FNV摘要"
    );
    let compiled = compile_source_with_options(
        "object-inspection-release-probe.wl",
        SOURCE,
        CompileOptions::v1_13(),
    );
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    let mut cold = Vec::with_capacity(SAMPLES);
    for sample in 1..=SAMPLES {
        let story = ready(&compiled);
        cold.push(measure(&story, &compiled, &query(&story), sample));
    }
    let story = ready(&compiled);
    let query = query(&story);
    let primed = story.inspect_state(&query).unwrap();
    verify_page(&primed, &compiled);
    let warm: Vec<_> = (1..=SAMPLES)
        .map(|sample| measure(&story, &compiled, &query, sample))
        .collect();
    let report = json!({
        "benchmark": "object_state_inspection_107_states_100_rows",
        "schema_version": 1,
        "runtime_version": env!("CARGO_PKG_VERSION"),
        "profile": "release",
        "host_os": std::env::consts::OS,
        "host_arch": std::env::consts::ARCH,
        "available_parallelism": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "process_id": std::process::id(),
        "recorded_unix_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
        "fixture": "runtime/tests/fixtures/object_state_inspection_v036/performance.wl",
        "fixture_bytes": SOURCE.len(),
        "fixture_sha256": SOURCE_SHA256,
        "fixture_fnv1a64": format!("{SOURCE_FNV1A64:016x}"),
        "runtime_fingerprint": compiled.analysis.fingerprint.to_string(),
        "seed": SEED,
        "query_template": StateInspectionQuery { expected_stamp: None, ..query.clone() },
        "expected_stamp_policy": "each sample echoes its own current run stamp; the returned sample stamp must match exactly",
        "cold_definition": "10 independent new runs; first inspection after the first real observation; one shared immutable CompileResult",
        "warm_definition": "10 repeated inspections of one separate run and stamp after one untimed priming query",
        "timed_scope": "inspect_state only, including its internal output-budget check",
        "excluded": ["compile", "Story construction", "Story continuation", "external JSON serialization", "correctness and readonly assertions", "native UI and input-to-render latency", "OS/process cold-cache claims"],
        "assertions": ["107 owned states, 100 returned rows", "complete TargetRef owner", "frozen static initial and first observation are distinct", "declaration identities", "stable ordering and pagination", "save/trace/RNG/visits/turns/choices/history/evidence/stamp/capture unchanged"],
        "sample_order": "10 cold samples, one warm priming query, 10 warm samples",
        "cold": {"summary": summary(&cold), "samples": cold},
        "warm": {"summary": summary(&warm), "samples": warm},
    });
    // 前置换行使JSON独占一行，便于从libtest输出提取并保存原始样本。
    println!("\n{}", serde_json::to_string(&report).unwrap());
}

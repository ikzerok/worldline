//! 显式运行的release测量探针；不宣称OS冷缓存，不以测试成功代替性能门。
use super::*;
use serde::Serialize;
use std::io::Write;
use std::time::Instant;

#[derive(Serialize)]
struct Sample {
    iteration: usize,
    setup_ms: f64,
    capture_ms: f64,
    preview_ms: f64,
    prepare_ms: f64,
    final_commit_guard_ms: f64,
    rss_kib_after_stages: Vec<Option<u64>>,
    content_baseline: String,
    session_digest: String,
    plan_digest: String,
    candidate_baseline: String,
}

#[derive(Serialize)]
struct Distribution {
    samples_ms: Vec<f64>,
    p50_ms: f64,
    p95_ms: f64,
    max_ms: f64,
}

#[test]
#[ignore = "显式release探针：需冻结fixture及WL_RECONCILIATION_PROBE_*参数；不自动写作者文件"]
fn reconciliation_release_stage_probe() {
    // Debug包仍须可编译；只有明确运行测量时才拒绝debug数值。
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "请使用--release测量，不能把debug数值冒作release"
    );
    let root = required_path("WL_RECONCILIATION_PROBE_ROOT");
    let input_path = required_path("WL_RECONCILIATION_PROBE_INPUT");
    let request_path = required_path("WL_RECONCILIATION_PROBE_REQUEST");
    let mode = std::env::var("WL_RECONCILIATION_PROBE_MODE").unwrap_or_else(|_| "steady".into());
    assert!(matches!(mode.as_str(), "cold" | "steady"));
    let run_id =
        std::env::var("WL_RECONCILIATION_PROBE_RUN_ID").expect("需要唯一run id保存全部测量");
    let count = if mode == "cold" { 1 } else { 31 };
    let input: ReconciliationInput = read_input(&input_path);
    let request: ReconciliationRequest = read_input(&request_path);
    let original = Project::open_reconciliation_input(&root, &input).unwrap();
    let disk = capture::disk_files(&original, &mut |_| true).unwrap();
    let files: Vec<_> = disk
        .iter()
        .map(|(path, bytes)| {
            serde_json::json!({
                "path":relative(&original.root,path).unwrap(), "bytes":bytes.len(),
                "fnv1a64":crate::problems::digest(bytes),
            })
        })
        .collect();
    let total_bytes: usize = disk.values().map(Vec::len).sum();
    let fixture_digest = digest(&disk).unwrap();
    drop(original);
    let mut samples = Vec::new();
    for iteration in 1..=count {
        let start = Instant::now();
        let mut current = Project::open_reconciliation_input(&root, &input).unwrap();
        let setup_ms = milliseconds(start);
        let mut rss = vec![rss_kib()];
        let before = current.content_baseline();
        let start = Instant::now();
        let session = current.capture_reconciliation().unwrap();
        let capture_ms = milliseconds(start);
        rss.push(rss_kib());
        let start = Instant::now();
        let plan = current.preview_reconciliation(&session, &request).unwrap();
        let preview_ms = milliseconds(start);
        rss.push(rss_kib());
        assert!(plan.can_apply, "{:?}", plan.blockers);
        let start = Instant::now();
        let prepared = current
            .prepare_reconciliation_with_progress(&plan, &mut |_| true)
            .unwrap();
        let prepare_ms = milliseconds(start);
        rss.push(rss_kib());
        assert_eq!(current.content_baseline(), before);
        // 作用于刚用于预览的真实当前Project；不测量空壳、不省略磁盘重验。
        let start = Instant::now();
        let applied = current.commit_prepared_reconciliation(prepared).unwrap();
        let final_commit_guard_ms = milliseconds(start);
        rss.push(rss_kib());
        assert_eq!(current.content_baseline(), plan.candidate_baseline);
        assert_eq!(applied.plan, plan);
        if let Some(first) = samples.first() {
            let first: &Sample = first;
            assert_eq!(
                first.session_digest, session.session_digest,
                "冻结fixture/session身份漂移"
            );
            assert_eq!(first.plan_digest, plan.plan_digest, "冻结候选身份漂移");
        }
        samples.push(Sample {
            iteration,
            setup_ms,
            capture_ms,
            preview_ms,
            prepare_ms,
            final_commit_guard_ms,
            rss_kib_after_stages: rss,
            content_baseline: before,
            session_digest: session.session_digest,
            plan_digest: plan.plan_digest,
            candidate_baseline: plan.candidate_baseline,
        });
    }
    let after = Project::open_reconciliation_input(&root, &input).unwrap();
    assert_eq!(
        capture::disk_files(&after, &mut |_| true).unwrap(),
        disk,
        "测量不能保存作者文件"
    );
    let final_guard = distribution(samples.iter().map(|sample| sample.final_commit_guard_ms));
    let report = serde_json::json!({
        "schema_version":1,"run_id":run_id,"mode":mode,"profile":"release",
        "cold_definition":"fresh process/Project session; OS filesystem cache is not cleared",
        "root":after.root,"entry":after.entry,"fixture_digest_kind":"fnv1a64, not a security signature",
        "fixture_digest":fixture_digest,"files":files,"file_count":disk.len(),"total_bytes":total_bytes,
        "request":request,"sample_count":count,"all_samples":samples,
        "distributions":{
            "setup":distribution(samples.iter().map(|s|s.setup_ms)),
            "capture":distribution(samples.iter().map(|s|s.capture_ms)),
            "preview":distribution(samples.iter().map(|s|s.preview_ms)),
            "prepare":distribution(samples.iter().map(|s|s.prepare_ms)),
            "final_commit_guard":final_guard,
        },
        "percentile_method":"nearest-rank (ceil(p*n)-1)",
        "final_guard_reference_ms":250.0,
        "final_guard_exceeds_reference":final_guard.max_ms > 250.0,
        "measurement_only":true,"disk_unchanged":true,"save_called":false,
        "linux_vm_hwm_kib":status_kib("VmHWM:"),
    });
    let bytes = serde_json::to_vec_pretty(&report).unwrap();
    println!(
        "RECONCILIATION_STAGE_PROBE {}",
        serde_json::to_string(&report).unwrap()
    );
    if let Some(path) = std::env::var_os("WL_RECONCILIATION_PROBE_OUTPUT") {
        let output = PathBuf::from(path);
        assert!(
            !crate::compiler::source_path(&output).starts_with(&after.root),
            "输出必须在冻结工程外"
        );
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .unwrap();
        file.write_all(&bytes).unwrap();
        file.write_all(b"\n").unwrap();
        file.sync_all().unwrap();
    }
}

fn required_path(key: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("缺少{key}"))
}
fn read_input<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let bytes = crate::file_access::read_limited(path, 32 * 1024 * 1024).unwrap();
    serde_json::from_value(crate::parse_unique_json(&bytes).unwrap())
        .unwrap_or_else(|error| panic!("探针材料无效：{error}"))
}
fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
fn distribution(values: impl Iterator<Item = f64>) -> Distribution {
    let samples_ms: Vec<_> = values.collect();
    let mut sorted = samples_ms.clone();
    sorted.sort_by(f64::total_cmp);
    let rank = |p: usize| sorted[(p * sorted.len()).div_ceil(100).saturating_sub(1)];
    Distribution {
        p50_ms: rank(50),
        p95_ms: rank(95),
        max_ms: *sorted.last().unwrap(),
        samples_ms,
    }
}
fn rss_kib() -> Option<u64> {
    status_kib("VmRSS:")
}
fn status_kib(key: &str) -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix(key)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

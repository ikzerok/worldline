use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

fn large_fixture(round: usize) -> (Fixture, ReaderExportSelection) {
    let mut source = String::from("let score = 7\nalias variable score as \"公开评分 Public score\"\nrelation_type connects as \"连接\"\n");
    let mut choice = selection();
    choice.objects.clear();
    choice.fields.clear();
    choice.attachments.clear();
    choice.objects.push(TargetRef::new("variable", "score"));
    for id in 0..1600 {
        source.push_str(&format!("entity place_{id} kind place as \"海港城市 City {id}\"\n  description \"这里记录航海者的公开资料。Public harbor history, trade and travel.\"\n  property climate = \"海洋气候 Mild maritime climate\"\nalias entity place_{id} as \"灯港 Harbor {id}\"\n"));
        let target = TargetRef::new("entity", &format!("place_{id}"));
        choice.objects.push(target.clone());
        choice.fields.push(ReaderFieldSelection {
            target,
            keys: vec!["climate".into()],
        });
    }
    for id in 0..99 {
        let parent = if id == 0 {
            String::new()
        } else {
            " within period_0".into()
        };
        source.push_str(&format!("period period_{id} as \"航海时期 {id}\"{parent}\nalias period period_{id} as \"Era 时期 {id}\"\n"));
        choice
            .objects
            .push(TargetRef::new("period", &format!("period_{id}")));
    }
    for id in 0..100 {
        source.push_str(&format!("relation_def edge_{id} type connects from entity place_{id} to entity place_{}\n  description \"公开商路 Trade route\"\nalias relation edge_{id} as \"航线 Route {id}\"\n",id+1));
        choice
            .objects
            .push(TargetRef::new("relation", &format!("edge_{id}")));
    }
    for id in 0..200 {
        source.push_str(&format!("event event_{id} as \"航海纪事 Voyage {id}\" during period_{}\n  沿海港启程，探索公开世界。A public story about a voyage.\n  choice \"继续航行 Continue\" if score > 0\n    -> END\nalias event event_{id} as \"故事 Story {id}\"\n",id%99));
        choice
            .objects
            .push(TargetRef::new("event", &format!("event_{id}")));
    }
    source.push_str("entity private_canary kind place as \"CANARY_PRIVATE_ENTITY\"\n  property private_note = \"CANARY_PRIVATE_FIELD\"\n");
    assert_eq!(choice.objects.len(), 2000);
    let fixture = Fixture::new(&format!("perf-{round}"), &source, "1.10");
    (fixture, choice)
}

/// 必须显式 --release --ignored 运行；普通 debug 门禁仅编译本测试。
#[test]
#[ignore]
#[allow(clippy::assertions_on_constants)]
fn release_reader_2000_preview_build_five_fresh_fixtures() {
    assert!(
        !cfg!(debug_assertions),
        "性能门槛必须使用 --release，不接受 debug 结果"
    );
    println!("reader machine os={} arch={} threads={} fresh fixtures=5; process and OS caches remain warm",std::env::consts::OS,std::env::consts::ARCH,std::thread::available_parallelism().map_or(1,usize::from));
    let mut timings = Vec::new();
    for round in 0..5 {
        let (fixture, choice) = large_fixture(round);
        let project = fixture.project();
        let before = project.export_files().unwrap();
        let baseline = project.content_baseline();
        let dirty = project.is_dirty();
        let start = Instant::now();
        let preview = project.preview_reader_export(&choice).unwrap();
        let preview_time = start.elapsed();
        let build_start = Instant::now();
        let files = project
            .build_reader_export(&choice, &preview.plan_digest)
            .unwrap();
        let build_time = build_start.elapsed();
        let total = start.elapsed();
        let bytes: usize = files.values().map(Vec::len).sum();
        assert!(!all_text(&files).contains("CANARY"));
        assert!(
            total <= Duration::from_secs(5),
            "round {round} preview+build {}ms >5000ms",
            total.as_millis()
        );
        let profile_start = Instant::now();
        let profile = project
            .create_reader_profile("performance", &choice)
            .unwrap();
        let create_time = profile_start.elapsed();
        let save_start = Instant::now();
        let save_plan = project.preview_save_reader_profile(&profile).unwrap();
        let save_time = save_start.elapsed();
        let sync = profile_start.elapsed();
        let mut apply_candidate = project.clone();
        let apply_start = Instant::now();
        apply_candidate
            .apply_save_reader_profile(&save_plan)
            .unwrap();
        let apply_time = apply_start.elapsed();
        let cancel_time = measure_cancel(&project, &choice, &preview.plan_digest);
        assert_eq!(project.export_files().unwrap(), before);
        assert_eq!(project.content_baseline(), baseline);
        assert_eq!(project.is_dirty(), dirty);
        let source_hash = hash(&serde_json::to_vec(&before).unwrap());
        println!("reader round={} preview_ms={} build_ms={} total_ms={} files={} bytes={} workspace_hash={} cancel_ms={} create_profile_ms={} preview_save_profile_ms={} profile_sync_ms={} sync_exceeds_250ms={} apply_profile_ms={} apply_exceeds_250ms={}",round+1,preview_time.as_millis(),build_time.as_millis(),total.as_millis(),files.len(),bytes,source_hash,cancel_time.as_millis(),create_time.as_millis(),save_time.as_millis(),sync.as_millis(),sync>Duration::from_millis(250),apply_time.as_millis(),apply_time>Duration::from_millis(250));
        timings.push(total.as_millis());
        if round == 4 {
            write_qa_output(&fixture, &before, &files);
        }
    }
    timings.sort_unstable();
    println!(
        "reader total_ms min={} median={} max={}",
        timings[0], timings[2], timings[4]
    );
}

fn measure_cancel(project: &Project, choice: &ReaderExportSelection, digest: &str) -> Duration {
    let output = Fixture::new("perf-cancel-output", "", "1.10");
    let target = output.root.join("cancelled-site");
    let entries_before = fs::read_dir(&output.root).unwrap().count();
    let requested = Arc::new(AtomicBool::new(false));
    let requested_at = Arc::new(Mutex::new(None));
    let (sender, receiver) = mpsc::channel();
    let control = Arc::clone(&requested);
    let control_time = Arc::clone(&requested_at);
    let thread = std::thread::spawn(move || {
        receiver.recv().unwrap();
        std::thread::sleep(Duration::from_millis(1));
        *control_time.lock().unwrap() = Some(Instant::now());
        control.store(true, Ordering::Release);
    });
    let mut sender = Some(sender);
    let result =
        project.export_reader_site_with_progress(choice, digest, &target, &mut |progress| {
            if progress.phase == "validate" {
                if let Some(sender) = sender.take() {
                    sender.send(()).unwrap();
                }
            }
            !requested.load(Ordering::Acquire)
        });
    let returned_at = Instant::now();
    thread.join().unwrap();
    assert!(result.unwrap_err().starts_with("READER_CANCELLED"));
    let latency = returned_at.duration_since(requested_at.lock().unwrap().unwrap());
    assert!(
        latency <= Duration::from_millis(500),
        "取消响应超过500ms: {}",
        latency.as_millis()
    );
    assert!(!target.exists());
    assert_eq!(fs::read_dir(&output.root).unwrap().count(), entries_before);
    latency
}

fn write_qa_output(
    _fixture: &Fixture,
    source: &BTreeMap<PathBuf, Vec<u8>>,
    files: &BTreeMap<PathBuf, Vec<u8>>,
) {
    let Some(destination) = std::env::var_os("WORLDLINE_READER_QA_OUTPUT") else {
        return;
    };
    let destination = PathBuf::from(destination);
    assert!(destination.is_absolute() && !destination.exists());
    assert!(
        !destination
            .ancestors()
            .any(|path| path.join(".git").exists()),
        "QA输出必须在仓外"
    );
    for (folder, entries) in [("workspace", source), ("site", files)] {
        for (path, bytes) in entries {
            let target = destination.join(folder).join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
        }
    }
    fs::write(destination.join("README.md"),"# reader release QA fixture\n\nworkspace 是含私有 CANARY 的合成作者工程；site 是显式公开站点。2000 对象、2000 别名，无地图或媒体场景。这里只验证代码生成与资源审计，未证明浏览器视觉或 file:// 验收。\n").unwrap();
    println!("reader QA output={}", destination.display());
}

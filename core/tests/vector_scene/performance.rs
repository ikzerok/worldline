use super::*;

#[test]
fn scene_capacity_and_import_shape_limit_are_explicit() {
    let mut s = MapScene::new(500.0, 500.0);
    for i in 0..5000 {
        let id = format!("p{i}");
        s.root_order
            .entry("places".into())
            .or_default()
            .push(id.clone());
        s.nodes.insert(id.clone(), point(&id, (i % 100) as f64));
    }
    validate_scene(&s, &SceneLimits::default()).unwrap();
    assert_eq!(project_scene(&s, 0.5).unwrap().len(), 5000);
    let source = format!(
        "<svg viewBox='0 0 2000 100'>{}</svg>",
        (0..1001)
            .map(|i| format!("<rect x='{i}' width='1' height='1'/>"))
            .collect::<String>()
    );
    assert_eq!(
        svg_import::preview_scene(&source).unwrap_err().code,
        "SCENE_LIMIT"
    );
}

#[test]
#[ignore = "需父任务以 release 显式运行并记录固定负载，debug 不作性能验收"]
fn scene_import_release_budget() {
    // ignored 测试运行时校验构建模式；const 断言会阻止正常 debug 测试编译。
    assert!(
        !std::hint::black_box(cfg!(debug_assertions)),
        "必须 release 执行"
    );
    let source = format!(
        "<svg viewBox='0 0 1000 10'>{}</svg>",
        (0..1000)
            .map(|i| format!("<rect x='{i}' width='1' height='1'/>"))
            .collect::<String>()
    );
    for trial in 0..5 {
        let (mut p, mut r) = project(&format!("perf-{trial}"));
        let operations = vec![
            SceneOp::EnableScene,
            SceneOp::ImportSvg {
                layer_id: "svg".into(),
                title: "性能".into(),
                source: source.clone(),
            },
        ];
        let request = batch(&p, r, operations);
        let start = std::time::Instant::now();
        let mut last = start;
        let mut max_gap = std::time::Duration::ZERO;
        {
            let mut progress = |_| {
                let now = std::time::Instant::now();
                max_gap = max_gap.max(now.duration_since(last));
                last = now;
                true
            };
            let plan =
                preview_batch_with_control(&p, r, request, &SceneLimits::default(), &mut progress)
                    .unwrap();
            apply_batch_with_control(&mut p, &mut r, &plan, &mut progress).unwrap();
        }
        max_gap = max_gap.max(last.elapsed());
        let elapsed = start.elapsed();
        assert!(elapsed <= std::time::Duration::from_secs(3));
        assert!(max_gap <= std::time::Duration::from_millis(250));
        let (cancel_project, cancel_revision) = project(&format!("perf-cancel-{trial}"));
        let before = cancel_project.content_baseline();
        let request = batch(
            &cancel_project,
            cancel_revision,
            vec![
                SceneOp::EnableScene,
                SceneOp::ImportSvg {
                    layer_id: "svg".into(),
                    title: "性能".into(),
                    source: source.clone(),
                },
            ],
        );
        let cancel_start = std::time::Instant::now();
        let mut requested = None;
        let result = preview_batch_with_control(
            &cancel_project,
            cancel_revision,
            request,
            &SceneLimits::default(),
            &mut |progress| {
                if progress.stage == "svg_import" && progress.completed >= 64 {
                    requested = Some(std::time::Instant::now());
                    false
                } else {
                    true
                }
            },
        );
        let cancel_end = std::time::Instant::now();
        assert_eq!(result.unwrap_err().code, "SCENE_CANCELLED");
        let response = cancel_end.duration_since(requested.expect("必须在解析后实际取消"));
        assert!(response <= std::time::Duration::from_millis(500));
        assert_eq!(cancel_project.content_baseline(), before);
        eprintln!("1000 SVG trial {trial}: parse_plan_apply_ms={:.3} max_progress_gap_ms={:.3} cancel_total_ms={:.3} cancel_response_ms={:.3}", elapsed.as_secs_f64()*1000.0, max_gap.as_secs_f64()*1000.0, cancel_end.duration_since(cancel_start).as_secs_f64()*1000.0, response.as_secs_f64()*1000.0);
    }
}

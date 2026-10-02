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
        let start = std::time::Instant::now();
        apply(
            &mut p,
            &mut r,
            vec![
                SceneOp::EnableScene,
                SceneOp::ImportSvg {
                    layer_id: "svg".into(),
                    title: "性能".into(),
                    source: source.clone(),
                },
            ],
        );
        let elapsed = start.elapsed();
        eprintln!("1000 SVG parse+plan+apply trial {trial}: {elapsed:?}");
        assert!(elapsed <= std::time::Duration::from_secs(3));
    }
}

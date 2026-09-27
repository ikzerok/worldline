use super::common::*;

#[test]

fn entity_cli_crud_uses_baseline_and_preserves_zero_write_on_stale_request() {
    let root = temp_entity_project("crud", "");
    let path = root.to_string_lossy().to_string();
    let run = |args: Vec<String>| {
        let mut out = Vec::new();
        let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
        (
            code,
            serde_json::from_slice::<serde_json::Value>(&out).unwrap(),
        )
    };
    let (code, created) = run(vec![
        "entity".into(),
        "create".into(),
        path.clone(),
        "--id=lighthouse".into(),
        "--kind=place".into(),
        "--display=雾港灯塔".into(),
        "--property=height=38".into(),
        "--json".into(),
    ]);
    assert_eq!(code, 0, "{created}");
    assert_eq!(created["entity"]["entity_type"], "place");
    assert!(std::fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("entity lighthouse kind place"));
    let baseline = created["baseline"].as_str().unwrap().to_string();
    let (code, stale) = run(vec![
        "entity".into(),
        "update".into(),
        path.clone(),
        "--id=lighthouse".into(),
        "--display=不应写入".into(),
        "--baseline=stale".into(),
        "--json".into(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"]["code"], "STALE_BASELINE");
    assert!(!std::fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("不应写入"));
    let (code, updated) = run(vec![
        "entity".into(),
        "update".into(),
        path.clone(),
        "--id=lighthouse".into(),
        "--display=新灯塔".into(),
        format!("--baseline={baseline}"),
        "--json".into(),
    ]);
    assert_eq!(code, 0, "{updated}");
    assert_eq!(updated["entity"]["display"], "新灯塔");
    let baseline = updated["baseline"].as_str().unwrap().to_string();
    let (code, deleted) = run(vec![
        "entity".into(),
        "delete".into(),
        path,
        "--id=lighthouse".into(),
        format!("--baseline={baseline}"),
        "--json".into(),
    ]);
    assert_eq!(code, 0, "{deleted}");
    assert!(deleted["entity"].is_null());
    assert!(!std::fs::read_to_string(root.join("world.wl"))
        .unwrap()
        .contains("entity lighthouse"));
}

#[test]
fn entity_cli_map_change_invalidates_baseline_before_write() {
    let root = temp_entity_project("map-baseline", "event start\n  -> END\n");
    let map_path = register_entity_test_map(&root);
    let path = root.to_string_lossy().to_string();
    let run = |args: Vec<String>| {
        let mut out = Vec::new();
        let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
        (
            code,
            serde_json::from_slice::<serde_json::Value>(&out).unwrap(),
        )
    };
    let (code, created) = run(vec![
        "entity".into(),
        "create".into(),
        path.clone(),
        "--id=dock".into(),
        "--kind=place".into(),
        "--display=码头".into(),
        "--json".into(),
    ]);
    assert_eq!(code, 0, "{created}");
    let baseline = created["baseline"].as_str().unwrap().to_string();
    let source_before = std::fs::read(root.join("world.wl")).unwrap();
    std::fs::write(
        &map_path,
        r#"{"schema_version":1,"required_features":[],"layers":[],"extensions":{"note":"外部修改"}}"#.as_bytes(),
    )
    .unwrap();
    let (code, stale) = run(vec![
        "entity".into(),
        "update".into(),
        path,
        "--id=dock".into(),
        "--display=不应写入".into(),
        format!("--baseline={baseline}"),
        "--json".into(),
    ]);
    assert_eq!(code, 1, "{stale}");
    assert_eq!(stale["error"]["code"], "STALE_BASELINE");
    assert_eq!(std::fs::read(root.join("world.wl")).unwrap(), source_before);
}

#[test]
fn entity_only_play_is_story_failure_in_json_mode() {
    let root = temp_entity_project("play", "entity lighthouse kind place as \"灯塔\"\n");
    let mut out = Vec::new();
    let code = wl::run(
        &[
            "play".into(),
            root.to_string_lossy().into(),
            "--json".into(),
        ],
        &mut out,
        &mut std::io::Cursor::new(Vec::new()),
    )
    .unwrap();
    assert_eq!(code, 1);
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["type"], "run_error");
}

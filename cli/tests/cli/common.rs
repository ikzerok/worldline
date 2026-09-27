use serde_json::Value;

pub(super) fn temp_story(name: &str, src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("wl_cli_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(name);
    std::fs::write(&f, src).unwrap();
    f
}

pub(super) fn temp_workspace(name: &str, manifest: &str, source: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir()
        .join("wl_cli_workspace_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join(".world/project.json"), manifest).unwrap();
    std::fs::write(root.join("world.wl"), source).unwrap();
    root
}

pub(super) fn temp_entity_project(name: &str, source: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir()
        .join("wl_cli_entity_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[]}"#,
    )
    .unwrap();
    std::fs::write(root.join("world.wl"), source).unwrap();
    root
}

pub(super) fn temp_presentation_project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir()
        .join("wl_cli_presentation_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world/maps")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["presentation.maps.v1"],"maps":{"overview":".world/maps/overview.json"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("world.wl"),
        "entity lighthouse kind place as \"灯塔\"\nevent start\n  -> END\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".world/maps/overview.json"),
        r#"{"schema_version":1,"id":"overview","title":"总览","canvas":{"width":100,"height":100,"unit":"normalized"},"layer_order":["places"],"layers":{"places":{"title":"地点","visible_default":true,"locked":false}},"placements":{"lighthouse_marker":{"layer_id":"places","annotation":"灯塔","role":"reference","target_ref":{"kind":"entity","id":"lighthouse"},"geometry":{"kind":"point","position":[0.2,0.3]}}}}"#,
    )
    .unwrap();
    root
}

pub(super) fn temp_relation_project(name: &str, source: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir()
        .join("wl_cli_relation_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":["content.relations.v1"]}"#,
    )
    .unwrap();
    std::fs::write(root.join("world.wl"), source).unwrap();
    root
}

pub(super) fn temp_markdown_import_fixture(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir()
        .join("wl_cli_markdown_import_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("project");
    let source = root.join("source");
    std::fs::create_dir_all(project.join(".world")).unwrap();
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        project.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.10","entry":"world.wl","required_features":[]}"#,
    )
    .unwrap();
    std::fs::write(project.join("world.wl"), "event start\n  -> END\n").unwrap();
    std::fs::write(
        source.join("harbor.md"),
        "---\nid: harbor\ntitle: 雾港\nkind: place\n---\n# 雾港\n**潮汐**\n",
    )
    .unwrap();
    (project, source)
}

pub(super) fn run_dynamic(args: Vec<String>) -> (i32, Value) {
    let mut out = Vec::new();
    let code = wl::run(&args, &mut out, &mut std::io::Cursor::new(Vec::new())).unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}

pub(super) fn register_entity_test_map(root: &std::path::Path) -> std::path::PathBuf {
    let manifest_path = root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["maps"] = serde_json::json!({"overview": ".world/maps/overview.json"});
    std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let map_path = root.join(".world/maps/overview.json");
    std::fs::create_dir_all(map_path.parent().unwrap()).unwrap();
    std::fs::write(
        &map_path,
        br#"{"schema_version":1,"required_features":[],"layers":[]}"#,
    )
    .unwrap();
    map_path
}
pub(super) fn run_args(args: &[&str]) -> (Result<i32, String>, Vec<u8>) {
    let mut out = Vec::new();
    let code = wl::run(
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        &mut out,
        &mut std::io::Cursor::new(Vec::new()),
    );
    (code, out)
}

/// 逐行解析 stdout 的事件流。
pub(super) fn json_lines(out: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8(out.to_vec())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("每行输出都应为合法 JSON"))
        .collect()
}

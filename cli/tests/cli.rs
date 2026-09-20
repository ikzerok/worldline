//! wl CLI 集成测试:直接驱动 lib 层的 run(),不启动进程。

fn temp_story(name: &str, src: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("wl_cli_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(name);
    std::fs::write(&f, src).unwrap();
    f
}

fn temp_workspace(name: &str, manifest: &str, source: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir()
        .join("wl_cli_workspace_tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(root.join(".world/project.json"), manifest).unwrap();
    std::fs::write(root.join("world.wl"), source).unwrap();
    root
}

fn temp_entity_project(name: &str, source: &str) -> std::path::PathBuf {
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

fn register_entity_test_map(root: &std::path::Path) -> std::path::PathBuf {
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

#[test]
fn check_json_output() {
    let f = temp_story("ok.wl", "event start\n  你好。\n  -> END\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["check".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("\"ok\":true"), "{text}");
    assert!(text.contains("\"diagnostics\""), "{text}");
    // JSON 必须可解析
    let v: serde_json::Value = serde_json::from_str(text.trim()).expect("输出应为合法 JSON");
    assert_eq!(v["ok"], serde_json::Value::Bool(true));
}

#[test]
fn read_only_workspace_diagnostics_stay_separate_and_fail_check() {
    let cases = [
        (
            "unknown-language",
            r#"{"schema_version":1,"language_version":"2.0","required_features":[]}"#,
        ),
        (
            "unknown-feature",
            r#"{"schema_version":1,"language_version":"1.10","required_features":["future.entities.v2"]}"#,
        ),
    ];
    for (name, manifest) in cases {
        let root = temp_workspace(name, manifest, "event start\n  -> END\n");
        let path = root.to_string_lossy().to_string();
        let mut out = Vec::new();
        let code = wl::run(
            &["check".into(), path.clone(), "--json".into()],
            &mut out,
            &mut std::io::Cursor::new(Vec::new()),
        )
        .unwrap();
        let check: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(code, 1, "{name}: {check}");
        assert_eq!(check["ok"], false, "{name}: {check}");
        assert_eq!(check["read_only"], true, "{name}: {check}");
        assert!(check["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(check["workspace_diagnostics"][0]["code"], "WS003");

        let mut out = Vec::new();
        let human_code = wl::run(
            &["check".into(), path.clone()],
            &mut out,
            &mut std::io::Cursor::new(Vec::new()),
        )
        .unwrap();
        let human = String::from_utf8(out).unwrap();
        assert_eq!(human_code, 1, "{name}: {human}");
        assert!(human.contains("WS003"), "{name}: {human}");
        assert!(human.contains("工作区只读"), "{name}: {human}");

        let mut out = Vec::new();
        let catalog_code = wl::run(
            &["catalog".into(), path, "--json".into()],
            &mut out,
            &mut std::io::Cursor::new(Vec::new()),
        )
        .unwrap();
        let catalog: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(catalog_code, 0, "{name}: {catalog}");
        assert_eq!(catalog["read_only"], true, "{name}: {catalog}");
        assert!(catalog["diagnostics"].as_array().unwrap().is_empty());
        assert_eq!(catalog["workspace_diagnostics"][0]["code"], "WS003");
    }
}

#[test]
fn catalog_json_includes_alias_and_text_link_sources() {
    let file = temp_story("navigation.wl", "character lin\nalias character lin as \"阿舟\"\nevent start\n  [[character:lin|阿舟]]。\n  -> END\n");
    let mut out = Vec::new();
    let code = wl::run(
        &[
            "catalog".into(),
            file.to_string_lossy().into(),
            "--json".into(),
        ],
        &mut out,
        &mut std::io::Cursor::new(Vec::new()),
    )
    .unwrap();
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(value["catalog"]["aliases"][0]["name"], "阿舟");
    assert_eq!(value["catalog"]["text_links"][0]["line"], 4);
    assert_eq!(value["catalog"]["text_links"][0]["source"]["id"], "start");
}

#[test]
fn check_exit_code_on_error() {
    let f = temp_story("bad.wl", "event start\n  -> missing\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["check".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1, "存在错误时退出码应为 1");
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("A101"), "{text}");
}

#[test]
fn play_scripted_choices() {
    let f = temp_story(
        "story.wl",
        "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new("1\n".as_bytes().to_vec());
    let code = wl::run(
        &["play".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("甲线。"), "{text}");
    assert!(text.contains("故事结束"), "{text}");
}

#[test]
fn graph_outputs_mermaid() {
    let f = temp_story("g.wl", "event start\n  choice \"走\"\n    -> END\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["graph".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("flowchart TD"), "{text}");
}

#[test]
fn timeline_outputs_swimlanes_and_drift() {
    let f = temp_story(
        "tl.wl",
        "storyline a as \"甲线\"\n  event start\n    -> END\n\nstoryline b\n  event b.entry\n    ->> start\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["timeline".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("flowchart LR"), "{text}");
    assert!(text.contains("subgraph"), "{text}");
    assert!(text.contains("甲线"), "{text}");
    assert!(text.contains("漂流"), "{text}");
}

#[test]
fn timeline_rejects_broken_story() {
    let f = temp_story("tl_bad.wl", "event start\n  -> missing\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["timeline".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1);
}

#[test]
fn play_rejects_broken_story() {
    let f = temp_story("broken.wl", "event start\n  -> nowhere\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["play".into(), f.to_string_lossy().into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1);
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("A101"), "{text}");
}

// -- 机器模式(spec/agent-protocol.md §2) ------------------------------------

/// 逐行解析 stdout 的事件流。
fn json_lines(out: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8(out.to_vec())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("每行输出都应为合法 JSON"))
        .collect()
}

#[test]
fn graph_json_outputs_structured() {
    let f = temp_story(
        "g_json.wl",
        "event start\n  choice \"走\"\n    -> next\n\nevent next\n  -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["graph".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let v = &json_lines(&out)[0];
    let nodes = v["graph"]["nodes"].as_array().expect("nodes 数组");
    assert!(!nodes.is_empty());
    assert_eq!(v["graph"]["entry"], 0);
    assert_eq!(v["graph"]["edges"][0]["kind"], "choice");
}

#[test]
fn timeline_json_outputs_anchors_and_stats() {
    let f = temp_story(
        "tl_json.wl",
        "storyline a as \"甲线\"\n  event start\n    开场。\n    anchor \"听闻\" as \"听说\"\n    -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &[
            "timeline".into(),
            f.to_string_lossy().into(),
            "--json".into(),
        ],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let v = &json_lines(&out)[0];
    assert_eq!(v["stats"]["events"], 1);
    assert_eq!(v["graph"]["storyline_order"][0][0], "a");
    let anchors = v["anchors"].as_array().expect("anchors 数组");
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0]["name"], "听闻");
}

#[test]
fn graph_json_compile_failed() {
    let f = temp_story("g_bad.wl", "event start\n  -> missing\n");
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &["graph".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 1);
    let v = &json_lines(&out)[0];
    assert_eq!(v["type"], "compile_failed");
    assert!(!v["diagnostics"].as_array().expect("diagnostics").is_empty());
}

#[test]
fn play_json_stepwise_choices() {
    let f = temp_story(
        "p_json.wl",
        "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n",
    );
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new("0\n".as_bytes().to_vec());
    let code = wl::run(
        &["play".into(), f.to_string_lossy().into(), "--json".into()],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let events = json_lines(&out);
    assert_eq!(events[0]["type"], "turn");
    assert_eq!(events[0]["outputs"][0]["content"], "开场。");
    assert_eq!(events[0]["choices"][0]["label"], "甲");
    assert_eq!(events[0]["choices"][0]["index"], 0);
    assert_eq!(events[0]["state"]["paused"], true);
    assert_eq!(events[1]["type"], "ended");
    assert_eq!(events[1]["state"]["ended"], true);
}

#[test]
fn play_json_eof_saves_and_reload_resumes_paused() {
    let f = temp_story(
        "p_save.wl",
        "event start\n  开场。\n  choice \"甲\"\n    甲线。\n    -> END\n  choice \"乙\"\n    乙线。\n    -> END\n",
    );
    let save = std::env::temp_dir()
        .join("wl_cli_tests")
        .join("p_save.json");
    // 第一段:空 stdin,暂停即 EOF → 落存档
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &[
            "play".into(),
            f.to_string_lossy().into(),
            "--json".into(),
            format!("--save={}", save.display()),
        ],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let events = json_lines(&out);
    assert_eq!(events[0]["type"], "turn");
    assert_eq!(events.last().unwrap()["type"], "eof");
    let save_text = std::fs::read_to_string(&save).unwrap();
    let sv: serde_json::Value = serde_json::from_str(&save_text).unwrap();
    assert!(sv["fingerprint"].is_u64(), "存档应含指纹");
    // 第二段:读档恢复,应再次停在同一个选择组
    let mut out = Vec::new();
    let mut input = std::io::Cursor::new(Vec::new());
    let code = wl::run(
        &[
            "play".into(),
            f.to_string_lossy().into(),
            "--json".into(),
            format!("--load={}", save.display()),
        ],
        &mut out,
        &mut input,
    )
    .unwrap();
    assert_eq!(code, 0);
    let events = json_lines(&out);
    assert_eq!(events[0]["type"], "turn");
    assert_eq!(events[0]["choices"][0]["label"], "甲");
    assert!(
        events[0]["outputs"].as_array().unwrap().is_empty(),
        "暂停态不重复产出文本"
    );
}

const CATALOG_STORY: &str = "tag coordinate as \"坐标\"\ntag harbor as \"港口\"\nmark tag harbor with coordinate\nmark event start with harbor\nevent start\n  -> END\n";

fn run_args(args: &[&str]) -> (Result<i32, String>, Vec<u8>) {
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

fn catalog_json(path: &std::path::Path, flags: &[&str]) -> (i32, serde_json::Value) {
    let path = path.to_string_lossy();
    let mut args = vec!["catalog", path.as_ref(), "--json"];
    args.extend_from_slice(flags);
    let (code, out) = run_args(&args);
    let mut rows = json_lines(&out);
    assert_eq!(rows.len(), 1, "catalog 每次输出一行 JSON");
    (code.unwrap(), rows.remove(0))
}

fn catalog_targets(value: &serde_json::Value) -> Vec<(String, String)> {
    let mut targets: Vec<_> = value["matches"]
        .as_array()
        .expect("matches 数组")
        .iter()
        .map(|object| {
            (
                object["target"]["kind"].as_str().unwrap().to_string(),
                object["target"]["id"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    targets.sort();
    targets
}

#[test]
fn catalog_json_exposes_full_index_and_source_locations() {
    let f = temp_story("catalog_index.wl", CATALOG_STORY);
    let (code, value) = catalog_json(&f, &[]);
    assert_eq!(code, 0);
    assert_eq!(value.as_object().unwrap().len(), 6);
    assert_eq!(value["ok"], true);
    assert!(value["diagnostics"].is_array());
    assert!(value["workspace_diagnostics"].is_array());
    assert_eq!(value["read_only"], false);
    let catalog = &value["catalog"];
    assert_eq!(value["matches"], catalog["objects"]);
    assert_eq!(catalog["tags"].as_object().unwrap().len(), 2);
    assert!(catalog["tags"]["coordinate"].is_object());
    assert!(catalog["tags"]["harbor"].is_object());
    assert_eq!(catalog["assets"], serde_json::json!({}));
    assert_eq!(catalog["marks"].as_array().unwrap().len(), 2);
    assert_eq!(catalog["attachments"], serde_json::json!([]));
    let harbor = catalog["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["target"] == serde_json::json!({"kind": "tag", "id": "harbor"}))
        .expect("港口标签对象");
    assert_eq!(harbor["display"], "港口");
    assert_eq!(harbor["line"], 2);
    assert!(harbor["file"]
        .as_str()
        .unwrap()
        .ends_with("catalog_index.wl"));
}

#[test]
fn catalog_filters_direct_recursive_and_kind_queries() {
    let f = temp_story("catalog_queries.wl", CATALOG_STORY);
    let cases = [
        (&["--tag", "coordinate"][..], &[("tag", "harbor")][..]),
        (&["--tag", "harbor"], &[("event", "start")]),
        (
            &["--tag", "coordinate", "--recursive"],
            &[("event", "start"), ("tag", "harbor")],
        ),
        (
            &["--tag", "coordinate", "--recursive", "--kind", "tag"],
            &[("tag", "harbor")],
        ),
        (
            &["--tag", "coordinate", "--recursive", "--kind", "event"],
            &[("event", "start")],
        ),
        (&["--tag", "coordinate", "--kind", "event"], &[]),
        (&["--kind", "event"], &[("event", "start")]),
    ];
    for (flags, expected) in cases {
        let (code, value) = catalog_json(&f, flags);
        assert_eq!(code, 0, "{flags:?}: {value}");
        let expected: Vec<_> = expected
            .iter()
            .map(|(kind, id)| (kind.to_string(), id.to_string()))
            .collect();
        assert_eq!(catalog_targets(&value), expected, "{flags:?}");
        assert_eq!(value["catalog"]["tags"].as_object().unwrap().len(), 2);
    }
}

#[test]
fn catalog_recursive_queries_terminate_and_deduplicate_cycles() {
    let f = temp_story(
        "catalog_cycle.wl",
        &format!("{CATALOG_STORY}mark tag coordinate with harbor\nmark event start with coordinate\nmark event start with harbor\n"),
    );
    let (code, value) = catalog_json(&f, &["--tag", "coordinate", "--recursive"]);
    assert_eq!(code, 0, "{value}");
    let targets = catalog_targets(&value);
    assert_eq!(
        targets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        targets.len(),
        "同一对象经不同标签路径命中时只出现一次"
    );
    assert!(targets.contains(&("event".into(), "start".into())));
    assert!(targets.contains(&("tag".into(), "harbor".into())));
}

#[test]
fn catalog_compile_failure_keeps_catalog_json_contract() {
    let f = temp_story(
        "catalog_broken.wl",
        &format!("{CATALOG_STORY}mark event missing with harbor\n"),
    );
    let (code, value) = catalog_json(&f, &[]);
    assert_eq!(code, 1);
    assert_eq!(value.as_object().unwrap().len(), 6);
    assert_eq!(value["ok"], false);
    assert!(value["catalog"].is_object());
    assert!(value["matches"].is_array());
    assert!(value["workspace_diagnostics"].is_array());
    assert_eq!(value["read_only"], false);
    assert!(value["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == "A214" && d["severity"] == "error"));
}

#[test]
fn catalog_parameters_allow_reordering_and_reject_usage_errors() {
    let f = temp_story("catalog_args.wl", CATALOG_STORY);
    let path = f.to_string_lossy();
    let (code, out) = run_args(&[
        "catalog",
        "--kind",
        "event",
        "--json",
        "--tag",
        "coordinate",
        &path,
        "--recursive",
    ]);
    assert_eq!(code.unwrap(), 0);
    assert_eq!(
        catalog_targets(&json_lines(&out)[0]),
        vec![("event".into(), "start".into())]
    );
    let invalid: &[&[&str]] = &[
        &["catalog"],
        &["catalog", &path, "--tag"],
        &["catalog", &path, "--tag", "--json"],
        &["catalog", &path, "--tag", ""],
        &["catalog", &path, "--kind"],
        &["catalog", &path, "--kind", "--recursive"],
        &["catalog", &path, "--tag", "harbor", "--tag", "coordinate"],
        &["catalog", &path, "--kind", "event", "--kind", "tag"],
        &["catalog", &path, "--unknown"],
        &["catalog", &path, "--load=save.json"],
        &["catalog", &path, "--save=save.json"],
        &["catalog", &path, &path],
    ];
    for args in invalid {
        let (code, out) = run_args(args);
        assert!(code.is_err(), "用法错误应返回 Err: {args:?}");
        assert!(out.is_empty(), "用法错误不输出成功结果");
    }
}

#[test]
fn catalog_unknown_tag_exits_with_usage_code_and_help() {
    let f = temp_story("catalog_unknown.wl", CATALOG_STORY);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wl"))
        .arg("catalog")
        .arg(f)
        .args(["--tag", "missing", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("未知标签 `missing`"), "{error}");
    assert!(error.contains("wl catalog"), "{error}");
}

#[test]
fn catalog_flags_remain_invalid_for_existing_commands() {
    for command in ["check", "play", "graph", "timeline"] {
        for flags in [
            vec!["--tag", "harbor"],
            vec!["--kind", "tag"],
            vec!["--recursive"],
        ] {
            let mut args = vec![command, "story.wl"];
            args.extend(flags);
            let (code, out) = run_args(&args);
            assert!(code.unwrap_err().contains("未知参数"), "{args:?}");
            assert!(out.is_empty());
        }
    }
}

#[test]
fn catalog_project_directory_resolves_cross_file_marks_once() {
    let root = std::env::temp_dir()
        .join("wl_cli_tests")
        .join("catalog project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("world.wl"),
        "include \"tags.wl\"\ninclude \"events.wl\"\ninclude \"tags.wl\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tags.wl"),
        "tag coordinate as \"坐标\"\ntag harbor as \"港口\"\nmark tag harbor with coordinate\n",
    )
    .unwrap();
    std::fs::write(
        root.join("events.wl"),
        "event start\n  -> END\nmark event start with harbor\n",
    )
    .unwrap();
    let (code, value) = catalog_json(&root, &["--tag", "coordinate", "--recursive"]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(
        catalog_targets(&value),
        vec![
            ("event".into(), "start".into()),
            ("tag".into(), "harbor".into())
        ]
    );
    let event = value["matches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["target"]["kind"] == "event")
        .unwrap();
    assert_eq!(event["line"], 1);
    assert!(event["file"].as_str().unwrap().ends_with("events.wl"));
}

#[test]
fn catalog_human_output_includes_labels_and_source() {
    let f = temp_story("catalog_human.wl", CATALOG_STORY);
    let (code, out) = run_args(&["catalog", &f.to_string_lossy(), "--tag", "coordinate"]);
    assert_eq!(code.unwrap(), 0);
    let text = String::from_utf8(out).unwrap();
    for expected in ["1 个命中对象", "tag harbor", "港口", "catalog_human.wl:2"] {
        assert!(text.contains(expected), "{text}");
    }
}

#[test]
fn catalog_entity_json_uses_manifest_language_and_target_identity() {
    let root = temp_entity_project("catalog", "entity lighthouse kind place as \"雾港灯塔\"\n");
    let (code, value) = catalog_json(&root, &["--kind", "entity"]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["language_version"], "1.10");
    assert_eq!(
        value["matches"][0]["target"],
        serde_json::json!({
            "kind": "entity",
            "id": "lighthouse"
        })
    );
    assert_eq!(
        value["catalog"]["entities"]["lighthouse"]["entity_type"],
        "place"
    );
}

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

use super::common::*;

const CATALOG_STORY: &str = "tag coordinate as \"坐标\"\ntag harbor as \"港口\"\nmark tag harbor with coordinate\nmark event start with harbor\nevent start\n  -> END\n";

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

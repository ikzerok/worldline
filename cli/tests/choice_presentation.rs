use serde_json::Value;
const SOURCE: &str = "event start\n  choice \"档案室\" enable false disabled \"还缺银钥匙\"\n    -> END\n  choice \"离开\"\n    -> END\n";

fn run(name: &str, options: &[&str], input: &str) -> (i32, String) {
    let path = std::env::temp_dir().join(format!(
        "wl-choice-presentation-{name}-{}.wl",
        std::process::id()
    ));
    std::fs::write(&path, SOURCE).unwrap();
    let mut args = vec![
        "play".into(),
        path.to_string_lossy().into_owned(),
        "--language-version=1.12".into(),
        "--seed=42".into(),
    ];
    args.extend(options.iter().map(|o| o.to_string()));
    let mut output = Vec::new();
    let result = wl::run(&args, &mut output, &mut std::io::Cursor::new(input)).unwrap();
    std::fs::remove_file(path).unwrap();
    (result, String::from_utf8(output).unwrap())
}

#[test]
fn default_json_is_selectable_only_and_opt_in_projection_does_not_shift_stdin_indices() {
    for (name, options, expected) in [
        ("legacy", vec!["--json"], false),
        ("aware", vec!["--json", "--choice-presentation"], true),
    ] {
        let (code, text) = run(name, &options, "0\n");
        assert_eq!(code, 0, "{text}");
        let rows: Vec<Value> = text
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(rows[0]["choices"].as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["choices"][0]["label"], "离开");
        assert_eq!(rows[0]["choices"][0]["index"], 0);
        assert_eq!(rows[0].get("choice_presentation").is_some(), expected);
        if expected {
            assert_eq!(rows[0]["choice_presentation"][0]["enabled"], false);
            assert_eq!(rows[0]["choice_presentation"][0]["index"], Value::Null);
            assert_eq!(
                rows[0]["choice_presentation"][0]["disabled_reason"],
                "还缺银钥匙"
            );
            assert_eq!(rows[0]["choice_presentation"][1]["index"], 0);
        }
        assert_eq!(rows.last().unwrap()["type"], "ended");
        assert_eq!(rows.last().unwrap()["state"]["turns"], 1);
    }
}

#[test]
fn human_menu_displays_lock_without_allocating_selectable_number() {
    let (code, text) = run("human", &[], "1\n");
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("[锁定] 档案室：还缺银钥匙"));
    assert!(text.contains("1) 离开"));
    assert!(!text.contains("1) 档案室"));
    assert!(text.contains("故事结束"));
}

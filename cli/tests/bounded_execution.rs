use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str, source: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("wl-bounded-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("world.wl");
    std::fs::write(&path, source).unwrap();
    path
}
fn invoke(path: &std::path::Path, options: &[String], input: &str) -> (i32, String) {
    let mut args = vec!["play".into(), path.to_string_lossy().into_owned()];
    args.extend_from_slice(options);
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut std::io::Cursor::new(input)).unwrap();
    (code, String::from_utf8(output).unwrap())
}
fn rows(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
fn budget(steps: u64) -> Vec<String> {
    vec![
        "--json".into(),
        format!("--max-steps={steps}"),
        format!("--time-budget-ms={}", u64::MAX),
    ]
}

#[test]
fn ordinary_play_loop_returns_story_error_instead_of_hanging_or_ending() {
    let path = fixture(
        "default-loop",
        "event start\n  -> next\nevent next\n  -> start\n",
    );
    let (code, text) = invoke(&path, &["--json".into()], "");
    assert_eq!(code, 1);
    let value = &rows(&text)[0];
    assert_eq!(value["type"], "run_error");
    assert_eq!(value["ok"], false);
    assert_eq!(value["state"]["ended"], false);
    assert!(value.get("outcome").is_none());
    assert!(value["message"].as_str().unwrap().contains("预算"));
}

#[test]
fn negotiated_output_and_save_load_deliver_each_random_line_once() {
    let path = fixture(
        "save",
        "event start\n  左:{rnd(1,100)}。~\n  右:{rnd(1,100)}。\n  -> END\n",
    );
    let save = path.with_extension("json");
    let mut options = budget(1);
    options.extend([
        "--bounded-continue".into(),
        "--seed=51".into(),
        format!("--save={}", save.display()),
    ]);
    let (code, text) = invoke(&path, &options, "");
    assert_eq!(code, 1);
    let first = rows(&text).remove(0);
    assert_eq!(first["type"], "suspended");
    assert_eq!(first["outcome"], "step_budget_exceeded");
    assert_eq!(first["executed_steps"], 1);
    assert_eq!(first["state"]["ended"], false);
    let mut options = budget(100);
    options.extend([
        "--bounded-continue".into(),
        format!("--load={}", save.display()),
    ]);
    let (code, text) = invoke(&path, &options, "");
    assert_eq!(code, 0, "{text}");
    let second = rows(&text).remove(0);
    assert_eq!(second["outcome"], "ended");
    assert_eq!(second["outputs"][0]["new_line"], false);
    let mut outputs = first["outputs"].as_array().unwrap().clone();
    outputs.extend(second["outputs"].as_array().unwrap().clone());
    let mut options = budget(100);
    options.push("--seed=51".into());
    let (_, text) = invoke(&path, &options, "");
    let full = rows(&text).remove(0);
    assert_eq!(Value::Array(outputs), full["outputs"]);
    assert_eq!(second["state"], full["state"]);
    assert!(full.get("outcome").is_none());
    assert!(full.get("executed_steps").is_none());
}

#[test]
fn legacy_partial_outputs_and_human_suspension_are_not_silent() {
    let path = fixture("legacy", "event start\n  部分正文\n  -> END\n");
    let (code, text) = invoke(&path, &budget(1), "");
    assert_eq!(code, 1);
    let value = rows(&text).remove(0);
    assert_eq!(value["outputs"][0]["content"], "部分正文");
    assert_eq!(value["type"], "run_error");
    assert_eq!(value["ok"], false);
    assert!(value.get("outcome").is_none());
    let (code, text) = invoke(
        &path,
        &[
            "--max-steps".into(),
            "1".into(),
            "--time-budget-ms".into(),
            u64::MAX.to_string(),
        ],
        "",
    );
    assert_eq!(code, 1);
    assert!(text.contains("部分正文"));
    assert!(text.contains("已暂停"));
    assert!(text.contains("--load"));
    assert!(!text.contains("故事结束"));
}

#[test]
fn capability_is_opt_in_and_choice_indices_keep_the_existing_shape() {
    let path = fixture("choice", "event start\n  choice \"继续\"\n    -> END\n");
    for enabled in [false, true] {
        let mut options = budget(100);
        if enabled {
            options.push("--bounded-continue".into());
        }
        let (code, text) = invoke(&path, &options, "0\n");
        assert_eq!(code, 0);
        let values = rows(&text);
        assert_eq!(values[0]["choices"][0]["index"], 0);
        assert_eq!(values[0].get("outcome").is_some(), enabled);
        if enabled {
            assert_eq!(values[0]["outcome"], "choice");
            assert_eq!(values[1]["outcome"], "ended");
        }
    }
}

#[test]
fn invalid_budget_and_non_play_flags_remain_usage_errors() {
    let path = fixture("args", "event start\n  -> END\n");
    for (command, flag) in [
        ("play", "--max-steps=-1"),
        ("play", "--time-budget-ms=no"),
        ("check", "--bounded-continue"),
        ("check", "--max-steps=2"),
    ] {
        let args = vec![
            command.into(),
            path.to_string_lossy().into_owned(),
            flag.into(),
        ];
        assert!(wl::run(&args, &mut Vec::new(), &mut std::io::Cursor::new("")).is_err());
    }
}

#[test]
fn dated_history_check_stays_quiet_but_explicit_play_reports_hint_on_stderr() {
    let path = fixture(
        "dated",
        "period past\nevent history during past\n  一段历史。\n",
    );
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_wl"))
        .args(["check", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(check.status.success());
    let check: Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(check["diagnostics"], Value::Array(Vec::new()));
    let play = std::process::Command::new(env!("CARGO_BIN_EXE_wl"))
        .args(["play", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(play.status.success());
    assert!(String::from_utf8(play.stderr).unwrap().contains("A202"));
    let value: Value = serde_json::from_slice(&play.stdout).unwrap();
    assert_eq!(value["type"], "ended");
    assert!(value.get("execution_diagnostics").is_none());
    assert!(value.get("outcome").is_none());
}

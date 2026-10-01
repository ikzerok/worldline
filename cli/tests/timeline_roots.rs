use serde_json::Value;
use std::io::Cursor;
const SOURCE: &str = "period year\nperiod summer within year\nperiod autumn within year\nevent opening during summer\n  开幕\nevent closing during autumn follows opening\n  闭幕\n";

fn invoke(source: &str, version: &str) -> (i32, Value) {
    let directory =
        std::env::temp_dir().join(format!("wl-cli-timeline-roots-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("{version}-{}.wl", source.len()));
    std::fs::write(&path, source).unwrap();
    let mut out = Vec::new();
    let code = wl::run(
        &[
            "timeline".into(),
            path.to_string_lossy().into(),
            "--json".into(),
            format!("--language-version={version}"),
        ],
        &mut out,
        &mut Cursor::new(Vec::<u8>::new()),
    )
    .unwrap();
    (code, serde_json::from_slice(&out).unwrap())
}

#[test]
fn timeline_cli_projects_root_order_without_changing_direct_rank() {
    let (code, json) = invoke(SOURCE, "1.13");
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["ok"], true);
    assert_eq!(json["timeline"]["status"], "complete");
    assert_eq!(json["timeline"]["order_scope"], "root_period");
    let event = &json["timeline"]["events"][1];
    assert_eq!(event["period"], "autumn");
    assert_eq!(event["rank"], 0);
    assert_eq!(event["root"], "year");
    assert_eq!(event["order_scope"], "year");
    assert_eq!(event["root_rank"], 1);
    assert_eq!(json["timeline"]["edges"][0]["order_scope"], "year");
}

#[test]
fn old_version_and_invalid_root_return_partial_story_failures() {
    for (source, version) in [
        (SOURCE.to_string(), "1.12"),
        (
            SOURCE.replace("period autumn within year", "period autumn"),
            "1.13",
        ),
        ("period\n".into(), "1.13"),
    ] {
        let (code, json) = invoke(&source, version);
        assert_eq!(code, 1, "{json}");
        assert_eq!(json["type"], "compile_failed");
        assert_eq!(json["ok"], false);
        assert_eq!(json["timeline"]["status"], "partial");
        assert!(!json["diagnostics"].as_array().unwrap().is_empty());
        assert!(json["timeline"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["root_rank"].is_null()));
    }
}

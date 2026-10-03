use serde_json::{json, Value};
use std::io::Cursor;
#[path = "support/problems_fixture.rs"]
mod fixture;
use fixture::{Fixture, BAD, GOOD};

fn invoke(fixture: &Fixture, extra: &[&str]) -> (i32, Value) {
    let mut args = vec![
        "problems".into(),
        fixture.root.display().to_string(),
        "--json".into(),
    ];
    args.extend(extra.iter().map(|value| value.to_string()));
    let mut output = Vec::new();
    let code = wl::run(&args, &mut output, &mut Cursor::new("")).unwrap();
    (code, serde_json::from_slice(&output).unwrap())
}

#[test]
fn default_page_matches_core_and_report_errors_are_successful_reads() {
    for (source, expected_exit) in [(GOOD, 0), (BAD, 1)] {
        let fixture = Fixture::new(source);
        let report = fixture.report();
        let expected = report.query(&Default::default(), None, 0).unwrap();
        let (code, response) = invoke(&fixture, &[]);
        assert_eq!(code, expected_exit, "{response}");
        assert_eq!(response["ok"], true);
        assert_eq!(response["page"], serde_json::to_value(expected).unwrap());
        assert_eq!(response["report"]["compile_count"], 1);
        let mut metadata = serde_json::to_value(report).unwrap();
        metadata.as_object_mut().unwrap().remove("entries");
        metadata.as_object_mut().unwrap().remove("related");
        assert_eq!(response["report"], metadata);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
            source
        );
    }
}

#[test]
fn pagination_related_and_filtered_exit_status_preserve_core_contract() {
    let fixture = Fixture::new(BAD);
    let report = fixture.report();
    assert!(report.entries.len() > 1);
    let (_, first) = invoke(&fixture, &["--limit", "1"]);
    let cursor = first["page"]["next_cursor"].to_string();
    let (_, second) = invoke(&fixture, &["--limit", "1", "--cursor-json", &cursor]);
    assert_ne!(
        first["page"]["entries"][0]["id"],
        second["page"]["entries"][0]["id"]
    );
    let (code, filtered) = invoke(
        &fixture,
        &["--query-json", r#"{"text":"definitely absent"}"#],
    );
    assert_eq!(code, 1);
    assert_eq!(filtered["page"]["matched"], 0);
    let entry = report
        .entries
        .iter()
        .find(|entry| entry.related_count > 0)
        .unwrap();
    let (_, related) = invoke(&fixture, &["--related", &entry.id, "--query-json", "{}"]);
    assert_eq!(
        related["page"],
        serde_json::to_value(report.related_page(&entry.id, None, 0).unwrap()).unwrap()
    );
    let (code, crossed) = invoke(
        &fixture,
        &["--related", &entry.id, "--cursor-json", &cursor],
    );
    assert_eq!(code, 2);
    assert_eq!(crossed["error"]["code"], "INVALID_CURSOR");
    std::fs::write(fixture.root.join("world.wl"), GOOD).unwrap();
    let (code, stale) = invoke(&fixture, &["--cursor-json", &cursor]);
    assert_eq!(code, 2);
    assert_eq!(stale["error"]["code"], "STALE_REPORT");
}

#[test]
fn strict_machine_errors_are_bounded_and_parseable() {
    let fixture = Fixture::new(GOOD);
    for extra in [
        vec!["--query-json", r#"{"text":"a","text":"b"}"#],
        vec!["--query-json", r#"{"unknown":true}"#],
        vec!["--query-json", r#"{"severities":["fatal"]}"#],
        vec![
            "--cursor-json",
            r#"{"report_version":"a","query_key":"b","offset":0,"extra":0}"#,
        ],
        vec!["--options-json", r#"{"extra":1}"#],
        vec!["--options-json", r#"{"max_entries":true}"#],
        vec!["--related", "x", "--query-json", r#"{"text":"a"}"#],
        vec!["--limit", "-1"],
        vec!["--limit", "1", "--limit", "2"],
        vec!["--json"],
        vec!["--unknown"],
        vec!["another-path"],
    ] {
        let (code, response) = invoke(&fixture, &extra);
        assert_eq!(code, 2, "{extra:?}: {response}");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "INVALID_PARAMS");
    }
    for (extra, expected) in [
        (
            vec!["--query-json", r#"{"path":"../outside.wl"}"#],
            "INVALID_QUERY",
        ),
        (vec!["--limit", "201"], "INVALID_QUERY"),
        (vec!["--related", "unknown"], "UNKNOWN_PROBLEM"),
        (
            vec!["--options-json", r#"{"max_report_bytes":1}"#],
            "BUDGET_EXCEEDED",
        ),
    ] {
        let (code, response) = invoke(&fixture, &extra);
        assert_eq!(code, 2, "{response}");
        assert_eq!(response["error"]["code"], expected);
    }
    let (code, partial) = invoke(&fixture, &["--options-json", r#"{"max_entries":0}"#]);
    assert_eq!(partial["ok"], true);
    assert_eq!(code, i32::from(partial["report"]["complete"] == false));
    assert_eq!(
        invoke(&fixture, &["--query-json", &json!({}).to_string()]).1["ok"],
        true
    );
}

#[test]
fn old_check_scope_and_help_are_unchanged() {
    let fixture = Fixture::new(GOOD);
    let manifest = json!({"schema_version":1,"language_version":"1.10","required_features":["presentation.maps.v1"],"maps":{"broken":".world/map.json"}});
    std::fs::write(
        fixture.root.join(".world/project.json"),
        manifest.to_string(),
    )
    .unwrap();
    std::fs::write(fixture.root.join(".world/map.json"), "{bad json").unwrap();
    let mut output = Vec::new();
    let args = vec![
        "check".into(),
        fixture.root.display().to_string(),
        "--json".into(),
    ];
    assert_eq!(
        wl::run(&args, &mut output, &mut Cursor::new("")).unwrap(),
        0
    );
    let (code, problems) = invoke(&fixture, &[]);
    assert_eq!(code, 1, "{problems}");
    assert_eq!(problems["ok"], true);
    output.clear();
    assert_eq!(
        wl::run(
            &["problems".into(), "--help".into()],
            &mut output,
            &mut Cursor::new("")
        )
        .unwrap(),
        0
    );
    assert!(String::from_utf8(output).unwrap().contains("--related"));
}

#[test]
fn related_first_request_rejects_old_identity_after_same_ordinal_changes() {
    let fixture = Fixture::new("event old\n  -> END\nevent old\n  -> END\n");
    let (_, original) = invoke(&fixture, &[]);
    let old = original["page"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["related_count"].as_u64().unwrap() > 0)
        .unwrap();
    let old_id = old["id"].as_str().unwrap();
    assert_eq!(invoke(&fixture, &["--related", old_id]).1["ok"], true);
    std::fs::write(
        fixture.root.join("world.wl"),
        "event new\n  -> END\nevent new\n  -> END\n",
    )
    .unwrap();
    let (_, current) = invoke(&fixture, &[]);
    let new = current["page"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["related_count"].as_u64().unwrap() > 0)
        .unwrap();
    let new_id = new["id"].as_str().unwrap();
    assert_ne!(old_id, new_id);
    // Deliberately collide the report-local ordinal; clients still copy the full ID.
    assert_eq!(
        old_id.rsplit_once(':').unwrap().1,
        new_id.rsplit_once(':').unwrap().1
    );
    let (code, stale) = invoke(&fixture, &["--related", old_id]);
    assert_eq!(code, 2);
    assert_eq!(stale["error"]["code"], "STALE_REPORT");
    assert!(stale.get("page").is_none());
    let (_, fresh) = invoke(&fixture, &["--related", new_id]);
    assert_eq!(fresh["ok"], true);
    assert!(fresh["page"]["locations"][0]["excerpt"]
        .as_str()
        .unwrap()
        .contains("event new"));
}

#[test]
fn source_context_is_additive_bounded_and_keeps_tail_hit_in_cli_json() {
    let source = format!(
        "event start\n  {}{{missing}}\n  -> END\n",
        "长中文😀".repeat(400)
    );
    let fixture = Fixture::new(&source);
    for budget in [0, 1, 2, 3, 4, 511, 512] {
        let options = format!("{{\"max_excerpt_bytes\":{budget}}}");
        let (exit, response) = invoke(&fixture, &["--options-json", &options]);
        assert_eq!(exit, 1);
        assert_eq!(response["ok"], true);
        assert_eq!(response["report"]["schema_version"], 1);
        assert!(response.to_string().len() <= 1024 * 1024);
        let primary = &response["page"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["code"] == "A102")
            .unwrap()["primary"];
        assert_eq!(primary["precision"], "span");
        assert_eq!(primary["context"]["version"], 1);
        assert_eq!(primary["context"]["role"], "target");
        assert_eq!(primary["excerpt"], primary["context"]["text"]);
        if budget == 0 {
            assert_eq!(primary["context"]["visibility"], "no_text");
        } else {
            assert!(primary["context"]["text"].as_str().unwrap().len() <= budget);
        }
        if budget >= 7 {
            assert_eq!(primary["context"]["visibility"], "full");
            assert!(primary["context"]["text"]
                .as_str()
                .unwrap()
                .contains("missing"));
        }
    }
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("world.wl")).unwrap(),
        source
    );
}

#[test]
fn cli_response_budget_counts_summary_and_context_without_skipping() {
    let suffix = "x".repeat(8192);
    let mut source = String::from("event start\n");
    for index in 0..150 {
        source.push_str(&format!("  {{missing_{index}_{suffix}}}\n"));
    }
    source.push_str("  -> END\n");
    let fixture = Fixture::new(&source);
    // Loaded empty sources add real coverage metadata without extra diagnostics.
    for index in 0..256 {
        std::fs::write(fixture.root.join(format!("context_{index:03}.wl")), "\n").unwrap();
    }
    let report = fixture.report();
    let raw_page = report.query(&Default::default(), None, 200).unwrap();
    assert!(serde_json::to_vec(&raw_page).unwrap().len() <= 1024 * 1024);
    let (exit, mut response) = invoke(&fixture, &["--limit", "200"]);
    assert_eq!(exit, 1);
    assert_eq!(response["ok"], true);
    let unbounded = json!({"ok":true,"report":response["report"],"page":&raw_page});
    let metadata_bytes = serde_json::to_vec(&response["report"]).unwrap().len();
    let page_bytes = serde_json::to_vec(&raw_page).unwrap().len();
    eprintln!("CLI bounded context: metadata_bytes={metadata_bytes} raw_page_bytes={page_bytes} unbounded_response_bytes={}", unbounded.to_string().len());
    assert!(unbounded.to_string().len() > 1024 * 1024);
    assert!(response["page"]["entries"].as_array().unwrap().len() < raw_page.entries.len());
    let mut ids = Vec::new();
    loop {
        assert!(response.to_string().len() < 1024 * 1024); // includes room for CLI newline
        let entries = response["page"]["entries"].as_array().unwrap();
        assert!(!entries.is_empty());
        ids.extend(
            entries
                .iter()
                .map(|entry| entry["id"].as_str().unwrap().to_owned()),
        );
        assert!(entries
            .iter()
            .all(|entry| entry["primary"]["context"]["version"] == 1));
        if response["page"]["next_cursor"].is_null() {
            break;
        }
        let cursor = response["page"]["next_cursor"].to_string();
        response = invoke(&fixture, &["--limit", "200", "--cursor-json", &cursor]).1;
        assert_eq!(response["ok"], true);
    }
    assert_eq!(
        ids,
        report
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn actual_cli_json_lines_bound_error_details_and_include_final_lf() {
    let fixture = Fixture::new(GOOD);
    let invoke_bytes = |extra: Vec<String>| {
        let mut args = vec![
            "problems".into(),
            fixture.root.display().to_string(),
            "--json".into(),
        ];
        args.extend(extra);
        let mut output = Vec::new();
        assert_eq!(
            wl::run(&args, &mut output, &mut Cursor::new("")).unwrap(),
            2
        );
        assert_eq!(output.last(), Some(&b'\n'));
        assert!(output.len() <= 1024 * 1024);
        let value: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["error"]["code"], "INVALID_PARAMS");
        (output, value)
    };
    let empty = json!({"ok":false,"error":{"code":"INVALID_PARAMS","message":"未知参数 --"}});
    let exact_padding = 1024 * 1024 - (empty.to_string().len() + 1);
    let (exact, value) = invoke_bytes(vec![format!("--{}", "x".repeat(exact_padding))]);
    assert_eq!(exact.len(), 1024 * 1024);
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with("未知参数 --"));
    let (bounded, value) = invoke_bytes(vec![format!("--{}", "x".repeat(exact_padding + 1))]);
    assert!(bounded.len() < 1024);
    assert_eq!(value["error"]["message"], "工程问题错误详情超过字节预算");
    let mut query = json!({});
    query
        .as_object_mut()
        .unwrap()
        .insert("\\\"".repeat(512 * 1024), json!(true));
    let (bounded, value) = invoke_bytes(vec!["--query-json".into(), query.to_string()]);
    assert!(bounded.len() < 1024);
    assert_eq!(value["error"]["message"], "工程问题错误详情超过字节预算");
}

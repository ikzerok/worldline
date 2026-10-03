use super::*;
#[path = "../../../cli/tests/support/problems_fixture.rs"]
pub(super) mod fixture;
use fixture::{Fixture, BAD, GOOD};

fn call(server: &mut Server, method: &str, params: Value) -> Value {
    server
        .dispatch(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string())
        .unwrap()
}
fn problems(server: &mut Server, params: Value) -> Value {
    call(server, "project.problems", params)["result"].clone()
}
fn open(server: &mut Server, fixture: &Fixture) {
    assert_eq!(
        call(server, "project.open", json!({"path":fixture.root}))["result"]["project_id"],
        "p1"
    );
}

#[test]
fn cache_queries_pages_related_and_explicit_refresh_count_only_real_builds() {
    let fixture = Fixture::new(BAD);
    let report = fixture.report();
    let mut server = Server::default();
    assert!(
        call(&mut server, "initialize", json!({}))["result"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("authoring.problems.v1"))
    );
    open(&mut server, &fixture);
    let first = problems(&mut server, json!({"project_id":"p1","limit":1}));
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["report"]["compile_count"], 1);
    assert_eq!(
        first["page"],
        serde_json::to_value(report.query(&Default::default(), None, 1).unwrap()).unwrap()
    );
    let next = problems(
        &mut server,
        json!({"project_id":"p1","limit":1,"cursor":first["page"]["next_cursor"]}),
    );
    assert_eq!(next["report"]["compile_count"], 0);
    assert_eq!(
        next["report"]["report_version"],
        first["report"]["report_version"]
    );
    assert_ne!(
        next["page"]["entries"][0]["id"],
        first["page"]["entries"][0]["id"]
    );
    let filtered = problems(
        &mut server,
        json!({"project_id":"p1","query":{"text":"missing"}}),
    );
    assert_eq!(filtered["report"]["compile_count"], 0);
    let related = report
        .entries
        .iter()
        .find(|entry| entry.related_count > 0)
        .unwrap();
    let response = problems(
        &mut server,
        json!({"project_id":"p1","related_id":related.id}),
    );
    assert_eq!(response["report"]["compile_count"], 0);
    assert_eq!(
        response["page"],
        serde_json::to_value(report.related_page(&related.id, None, 0).unwrap()).unwrap()
    );
    let refreshed = problems(&mut server, json!({"project_id":"p1","refresh":true}));
    assert_eq!(refreshed["report"]["compile_count"], 1);
    assert_eq!(
        refreshed["report"]["report_version"],
        first["report"]["report_version"]
    );
    let limited = problems(
        &mut server,
        json!({"project_id":"p1","options":{"max_entries":1}}),
    );
    assert_eq!(limited["report"]["compile_count"], 1);
    let repeated = problems(
        &mut server,
        json!({"project_id":"p1","options":{"max_entries":1}}),
    );
    assert_eq!(repeated["report"]["compile_count"], 0);
    for _ in 0..2 {
        assert_eq!(
            problems(&mut server, json!({"path":fixture.root}))["report"]["compile_count"],
            1
        );
    }
}

#[test]
fn shape_errors_business_errors_and_duplicate_keys_stay_distinct() {
    let fixture = Fixture::new(GOOD);
    let mut server = Server::default();
    for params in [
        json!({}),
        json!([]),
        json!({"path":fixture.root,"project_id":"p1"}),
        json!({"path":null}),
        json!({"path":fixture.root,"extra":true}),
        json!({"path":fixture.root,"query":{"extra":true}}),
        json!({"path":fixture.root,"options":{"extra":true}}),
        json!({"path":fixture.root,"query":{"domains":["unknown"]}}),
        json!({"path":fixture.root,"cursor":{"report_version":"a","query_key":"b","offset":0,"extra":true}}),
        json!({"path":fixture.root,"query":null}),
        json!({"path":fixture.root,"refresh":"yes"}),
        json!({"path":fixture.root,"limit":-1}),
        json!({"path":fixture.root,"limit":1.5}),
        json!({"path":fixture.root,"related_id":null}),
        json!({"path":fixture.root,"related_id":"x","query":{"text":"a"}}),
        json!({"project_id":"unknown"}),
    ] {
        assert_eq!(
            call(&mut server, "project.problems", params)["error"]["code"],
            -32602
        );
    }
    for (params, code) in [
        (
            json!({"path":fixture.root,"query":{"path":"../escape"}}),
            "INVALID_QUERY",
        ),
        (json!({"path":fixture.root,"limit":201}), "INVALID_QUERY"),
        (
            json!({"path":fixture.root,"options":{"max_report_bytes":1}}),
            "BUDGET_EXCEEDED",
        ),
        (
            json!({"path":fixture.root,"related_id":"missing"}),
            "UNKNOWN_PROBLEM",
        ),
        (json!({"path":fixture.root.join("missing.wl")}), "IO_ERROR"),
    ] {
        let response = call(&mut server, "project.problems", params);
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["error"]["code"], code);
    }
    for raw in [
        r#"{"jsonrpc":"2.0","id":1,"method":"project.problems","params":{"path":"a","path":"b"}}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"project.problems","params":{"path":"a","query":{"text":"a","text":"b"}}}"#,
    ] {
        assert_eq!(server.dispatch(raw).unwrap()["error"]["code"], -32700);
    }
}

#[test]
fn changed_source_rejects_old_cursor_and_conflicts_keep_local_buffers() {
    let fixture = Fixture::new(BAD);
    let mut server = Server::default();
    open(&mut server, &fixture);
    let first = problems(&mut server, json!({"project_id":"p1","limit":1}));
    std::fs::write(fixture.root.join("world.wl"), GOOD).unwrap();
    let stale = problems(
        &mut server,
        json!({"project_id":"p1","cursor":first["page"]["next_cursor"]}),
    );
    assert_eq!(stale["error"]["code"], "STALE_REPORT");
    let project = &mut server.projects.get_mut("p1").unwrap().project;
    project
        .set_text(&fixture.root.join("world.wl"), BAD.into())
        .unwrap();
    let local = problems(&mut server, json!({"project_id":"p1"}));
    std::fs::write(fixture.root.join("world.wl"), format!("{GOOD}# external\n")).unwrap();
    let conflict = problems(&mut server, json!({"project_id":"p1"}));
    assert_eq!(conflict["report"]["compile_count"], 1);
    assert_eq!(
        conflict["report"]["content_baseline"],
        local["report"]["content_baseline"]
    );
    assert_eq!(conflict["conflicts"].as_array().unwrap().len(), 1);
    assert_eq!(conflict["report"]["complete"], false);
    assert!(conflict["report"]["reasons"]
        .as_array()
        .unwrap()
        .contains(&json!("source_conflict")));
    assert_eq!(
        server.projects["p1"]
            .project
            .document(&fixture.root.join("world.wl"))
            .unwrap(),
        BAD
    );
}

#[test]
fn resource_removal_and_restoration_invalidate_without_changing_content_baseline() {
    let fixture = Fixture::new(&format!(
        "asset cover image \"cover.png\" as \"封面\"\n{GOOD}"
    ));
    std::fs::write(fixture.root.join("cover.png"), b"image placeholder").unwrap();
    let mut server = Server::default();
    open(&mut server, &fixture);
    let initial = problems(&mut server, json!({"project_id":"p1"}));
    assert_eq!(initial["ok"], true);
    assert!(!initial["page"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["code"] == "A215"));
    assert_eq!(
        problems(&mut server, json!({"project_id":"p1"}))["report"]["compile_count"],
        0
    );
    std::fs::remove_file(fixture.root.join("cover.png")).unwrap();
    let missing = problems(&mut server, json!({"project_id":"p1"}));
    assert_eq!(missing["report"]["compile_count"], 1);
    assert_eq!(
        missing["report"]["content_baseline"],
        initial["report"]["content_baseline"]
    );
    assert_ne!(
        missing["report"]["source_observation"],
        initial["report"]["source_observation"]
    );
    assert!(missing["page"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["code"] == "A215"));
    std::fs::write(fixture.root.join("cover.png"), b"image placeholder").unwrap();
    let restored = problems(&mut server, json!({"project_id":"p1"}));
    assert_eq!(restored["report"]["compile_count"], 1);
    assert_eq!(
        restored["report"]["source_observation"],
        initial["report"]["source_observation"]
    );
    assert_eq!(restored["page"], initial["page"]);
}

#[test]
fn response_budget_includes_metadata_and_continues_without_skipping() {
    let fixture = Fixture::new(BAD);
    let mut report = fixture.report();
    let mut entry = report.entries[0].clone();
    entry.message = "m".repeat(16384);
    entry.note = Some("n".repeat(16384));
    entry.suggestion = Some("s".repeat(16384));
    assert_eq!(entry.primary.context.as_ref().unwrap().version, 1);
    let full_entry = serde_json::to_value(&entry).unwrap();
    let mut old_entry = full_entry.clone();
    old_entry["primary"]
        .as_object_mut()
        .unwrap()
        .remove("context");
    assert!(full_entry.to_string().len() > old_entry.to_string().len());
    let version = report.report_version.clone();
    report.entries = (1..=80)
        .map(|index| {
            let mut entry = entry.clone();
            entry.id = format!("{version}:p{index}");
            entry
        })
        .collect();
    report.reasons.push("metadata".repeat(16384));
    let mut params = Params {
        limit: 200,
        ..Default::default()
    };
    let mut seen = Vec::new();
    loop {
        let page = response(&report, &params, true, &[], MAX_RESPONSE_BYTES);
        assert_eq!(page["ok"], true, "{page}");
        assert!(page.to_string().len() <= 1024 * 1024);
        assert!(page["page"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["primary"]["context"]["version"] == 1));
        seen.extend(
            page["page"]["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["id"].as_str().unwrap().to_owned()),
        );
        params.cursor = serde_json::from_value(page["page"]["next_cursor"].clone()).unwrap();
        if params.cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        seen,
        (1..=80)
            .map(|index| format!("{version}:p{index}"))
            .collect::<Vec<_>>()
    );
    report.reasons = vec!["x".repeat(1024 * 1024)];
    assert_eq!(
        response(&report, &Params::default(), true, &[], MAX_RESPONSE_BYTES)["error"]["code"],
        "BUDGET_EXCEEDED"
    );
}

#[test]
fn related_first_request_rejects_old_identity_after_same_ordinal_changes() {
    let fixture = Fixture::new("event old\n  -> END\nevent old\n  -> END\n");
    let mut server = Server::default();
    open(&mut server, &fixture);
    let original = problems(&mut server, json!({"project_id":"p1"}));
    let old = original["page"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["related_count"].as_u64().unwrap() > 0)
        .unwrap();
    let old_id = old["id"].as_str().unwrap();
    std::fs::write(
        fixture.root.join("world.wl"),
        "event new\n  -> END\nevent new\n  -> END\n",
    )
    .unwrap();
    for target in [
        json!({"project_id":"p1","related_id":old_id}),
        json!({"path":fixture.root,"related_id":old_id}),
    ] {
        let stale = call(&mut server, "project.problems", target);
        assert!(stale.get("error").is_none());
        assert_eq!(stale["result"]["error"]["code"], "STALE_REPORT");
        assert!(stale["result"].get("page").is_none());
    }
    let current = problems(&mut server, json!({"project_id":"p1"}));
    let new = current["page"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["related_count"].as_u64().unwrap() > 0)
        .unwrap();
    let new_id = new["id"].as_str().unwrap();
    assert_ne!(old_id, new_id);
    assert_eq!(
        old_id.rsplit_once(':').unwrap().1,
        new_id.rsplit_once(':').unwrap().1
    );
    let fresh = problems(&mut server, json!({"project_id":"p1","related_id":new_id}));
    assert_eq!(fresh["ok"], true);
    assert_eq!(fresh["report"]["compile_count"], 0);
    assert!(fresh["page"]["locations"][0]["excerpt"]
        .as_str()
        .unwrap()
        .contains("event new"));
}

#[test]
fn unstable_observation_reports_rebuild_but_stable_partial_reports_reuse() {
    let fixture = Fixture::new(&format!(
        "asset cover image \"cover.png\" as \"封面\"\n{GOOD}"
    ));
    let mut server = Server::default();
    open(&mut server, &fixture);
    std::fs::write(fixture.root.join("cover.png"), b"image placeholder").unwrap();
    let unit = server.projects.get_mut("p1").unwrap();
    // Produce a real unstable report: the compile/start observation see the file,
    // then it disappears during validators and is restored before the next request.
    let report = unit
        .project
        .problems_report_with_progress(&Default::default(), &mut |domain| {
            if domain == worldline_core::problems::ProblemDomain::Workspace {
                std::fs::remove_file(fixture.root.join("cover.png")).unwrap();
            }
            true
        })
        .unwrap();
    assert!(!report.complete);
    assert!(report
        .reasons
        .iter()
        .any(|reason| reason == "external_observation_changed"));
    std::fs::write(fixture.root.join("cover.png"), b"image placeholder").unwrap();
    assert_eq!(
        report.source_observation,
        unit.project.problems_observation_key().unwrap()
    );
    assert_eq!(report.content_baseline, unit.project.content_baseline());
    unit.problems_report = Some(CachedReport {
        report,
        conflicts: Vec::new(),
    });
    let rebuilt = problems(&mut server, json!({"project_id":"p1"}));
    assert_eq!(rebuilt["report"]["compile_count"], 1);
    assert_eq!(rebuilt["report"]["complete"], true);
    assert!(!rebuilt["report"]["reasons"]
        .as_array()
        .unwrap()
        .contains(&json!("external_observation_changed")));
    assert!(!rebuilt["page"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["code"] == "A215"));
    assert_eq!(
        problems(&mut server, json!({"project_id":"p1"}))["report"]["compile_count"],
        0
    );
    std::fs::write(fixture.root.join("world.wl"), BAD).unwrap();
    let partial = problems(
        &mut server,
        json!({"project_id":"p1","options":{"max_entries":0}}),
    );
    assert_eq!(partial["report"]["complete"], false);
    assert_eq!(partial["report"]["truncated"], true);
    assert_eq!(partial["report"]["compile_count"], 1);
    let cached = problems(
        &mut server,
        json!({"project_id":"p1","options":{"max_entries":0}}),
    );
    assert_eq!(cached["report"]["compile_count"], 0);
    assert_eq!(cached["page"], partial["page"]);
}

#[test]
fn context_capability_tail_hit_and_cached_pages_preserve_schema_one_requests() {
    let source = format!(
        "event start\n  {}{{missing}}\n  -> END\n",
        "长中文😀".repeat(400)
    );
    let fixture = Fixture::new(&source);
    let mut server = Server::default();
    let initialized = call(&mut server, "initialize", json!({}));
    assert!(initialized["result"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!(
            worldline_core::problems::PROBLEM_SOURCE_CONTEXT_CAPABILITY
        )));
    open(&mut server, &fixture);
    for budget in [0, 1, 2, 3, 4, 511, 512] {
        let params = json!({"project_id":"p1","options":{"max_excerpt_bytes":budget}});
        let response = problems(&mut server, params.clone());
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
        let cached = problems(&mut server, params);
        assert_eq!(cached["report"]["compile_count"], 0);
        assert_eq!(cached["page"], response["page"]);
    }
    assert_eq!(
        call(
            &mut server,
            "project.problems",
            json!({
                "project_id":"p1", "context":true
            })
        )["error"]["code"],
        -32602
    );
}

use super::tests::fixture::{Fixture, BAD};
use super::*;
use std::io::Cursor;

fn request(id: Value, params: Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"method":"project.problems","params":params}).to_string()
}
fn open(server: &mut Server, fixture: &Fixture) {
    server
        .dispatch(
            &json!({"jsonrpc":"2.0","id":0,"method":"project.open","params":{"path":fixture.root}})
                .to_string(),
        )
        .unwrap();
}
fn shell_bytes(id: &Value) -> usize {
    json!({"jsonrpc":"2.0","id":id,"result":null})
        .to_string()
        .len()
        - 4
        + 1
}

#[test]
fn actual_rpc_envelope_shrinks_primary_and_related_pages_without_losing_ids() {
    let fixture = Fixture::new(BAD);
    for id in [json!(7), json!("中\"\\\n".repeat(8192)), Value::Null] {
        for related in [false, true] {
            let mut server = Server::default();
            open(&mut server, &fixture);
            let mut report = fixture.report();
            let mut first = report.entries[0].clone();
            first.id = format!("{}:p1", report.report_version);
            let mut second = first.clone();
            second.id = format!("{}:p2", report.report_version);
            first.related_count = 2;
            report.entries = vec![first.clone(), second];
            report.related.clear();
            report.related.insert(
                first.id.clone(),
                vec![first.primary.clone(), first.primary.clone()],
            );
            report.reasons = vec![String::new()];
            let params = Params {
                project_id: Some("p1".into()),
                limit: 2,
                related_id: related.then(|| first.id.clone()),
                ..Default::default()
            };
            let initial = response(&report, &params, true, &[], MAX_RESPONSE_BYTES);
            let padding = MAX_RESPONSE_BYTES - shell_bytes(&id) - initial.to_string().len() + 1;
            report.reasons[0] = "m".repeat(padding);
            let raw = response(&report, &params, true, &[], MAX_RESPONSE_BYTES);
            let raw_bytes = raw.to_string().len();
            assert!(raw_bytes <= MAX_RESPONSE_BYTES);
            assert_eq!(raw_bytes + shell_bytes(&id), MAX_RESPONSE_BYTES + 1);
            eprintln!(
                "RPC boundary: related={related} result_bytes={raw_bytes} shell_and_lf_bytes={}",
                shell_bytes(&id)
            );
            server.projects.get_mut("p1").unwrap().problems_report = Some(CachedReport {
                report,
                conflicts: Vec::new(),
            });
            let mut wire_params = json!({"project_id":"p1","limit":2});
            if related {
                wire_params["related_id"] = json!(first.id);
            }
            let wire = request(id.clone(), wire_params.clone());
            let actual = server.dispatch(&wire).unwrap();
            assert_eq!(actual["id"], id);
            assert_eq!(actual["result"]["ok"], true);
            assert_eq!(actual["result"]["report"]["compile_count"], 0);
            assert!(actual.to_string().len() < MAX_RESPONSE_BYTES);
            let list = if related { "locations" } else { "entries" };
            assert_eq!(actual["result"]["page"][list].as_array().unwrap().len(), 1);
            assert_eq!(actual["result"]["page"]["next_cursor"]["offset"], 1);
            let handled = server.handle(&wire).unwrap();
            assert!(handled.len() < MAX_RESPONSE_BYTES);
            assert_eq!(serde_json::from_str::<Value>(&handled).unwrap(), actual);
            wire_params["cursor"] = actual["result"]["page"]["next_cursor"].clone();
            let next = server.dispatch(&request(id.clone(), wire_params)).unwrap();
            assert!(next.to_string().len() < MAX_RESPONSE_BYTES);
            assert_eq!(next["result"]["page"][list].as_array().unwrap().len(), 1);
            assert!(next["result"]["page"]["next_cursor"].is_null());
            if !related {
                assert_ne!(
                    actual["result"]["page"][list][0]["id"],
                    next["result"]["page"][list][0]["id"]
                );
            }
        }
    }
}

#[test]
fn huge_identifiers_fail_before_method_execution_without_truncation() {
    let fixture = Fixture::new(BAD);
    let mut server = Server::default();
    open(&mut server, &fixture);
    assert!(server.projects["p1"].problems_report.is_none());
    for id in [
        json!("x".repeat(MAX_RESPONSE_BYTES)),
        json!("\"".repeat(MAX_RESPONSE_BYTES / 2)),
    ] {
        let response = server
            .dispatch(&request(id, json!({"project_id":"p1"})))
            .unwrap();
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], -32600);
        assert_eq!(
            response["error"]["data"],
            "request_id_exceeds_response_budget"
        );
        assert!(response.to_string().len() < MAX_RESPONSE_BYTES);
        assert!(server.projects["p1"].problems_report.is_none());
    }
    // The same large ID on another method keeps its existing protocol behavior.
    let id = json!("x".repeat(MAX_RESPONSE_BYTES));
    let initialized = server
        .dispatch(&json!({"jsonrpc":"2.0","id":id,"method":"initialize"}).to_string())
        .unwrap();
    assert_eq!(initialized["id"], id);
    assert!(initialized.to_string().len() > MAX_RESPONSE_BYTES);
}

#[test]
fn oversize_protocol_detail_preserves_error_code_and_representable_id() {
    let mut server = Server::default();
    let id = json!("保留\"原标识");
    let mut params = json!({"path":"unused"});
    params
        .as_object_mut()
        .unwrap()
        .insert("x".repeat(MAX_RESPONSE_BYTES), json!(true));
    let response = server.dispatch(&request(id.clone(), params)).unwrap();
    assert_eq!(response["id"], id);
    assert_eq!(response["error"]["code"], -32602);
    assert!(response["error"]["data"].is_null());
    assert!(response.to_string().len() < MAX_RESPONSE_BYTES);
    let invalid = server
        .dispatch(&json!({"jsonrpc":"wrong","id":id,"method":"project.problems"}).to_string())
        .unwrap();
    assert_eq!(invalid["id"], id);
    assert_eq!(invalid["error"]["code"], -32600);
}

#[test]
fn actual_line_transport_includes_lf_and_notifications_remain_silent() {
    let fixture = Fixture::new(BAD);
    let representable_id = json!("x".repeat(MAX_RESPONSE_BYTES - 512));
    let giant_id = json!("x".repeat(MAX_RESPONSE_BYTES));
    let input = [
        request(representable_id.clone(), json!({"path":fixture.root})),
        request(giant_id, json!({"path":fixture.root})),
        json!({"jsonrpc":"2.0","method":"project.problems","params":{"path":fixture.root}})
            .to_string(),
    ]
    .join("\n")
        + "\n";
    let mut output = Vec::new();
    assert_eq!(run(&mut Cursor::new(input), &mut output), 0);
    let lines: Vec<_> = output.split_inclusive(|byte| *byte == b'\n').collect();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|line| line.len() <= MAX_RESPONSE_BYTES));
    let first: Value = serde_json::from_slice(lines[0]).unwrap();
    assert_eq!(first["id"], representable_id);
    assert_eq!(first["result"]["error"]["code"], "BUDGET_EXCEEDED");
    let second: Value = serde_json::from_slice(lines[1]).unwrap();
    assert!(second["id"].is_null());
    assert_eq!(second["error"]["code"], -32600);
}

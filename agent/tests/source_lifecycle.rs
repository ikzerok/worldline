use serde_json::{json, Value};
use std::io::{Cursor, Write};
use std::process::{Command, Stdio};
#[path = "../../cli/tests/support/source_lifecycle_fixture.rs"]
mod fixture;
use fixture::Fixture;

fn request(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}

fn lines(requests: &[Value]) -> String {
    requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn responses(bytes: &[u8]) -> Vec<Value> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn exchange(requests: &[Value]) -> Vec<Value> {
    let mut output = Vec::new();
    assert_eq!(
        worldline_agent::run(&mut Cursor::new(lines(requests)), &mut output),
        0
    );
    responses(&output)
}

#[test]
fn rpc_source_lifecycle_path_and_project_preview_match_core_and_apply_saves() {
    let fixture = Fixture::new("rpc-create", true);
    let edit = json!({"operation":"create","path":"章节/新 章.wl"});
    let plan = fixture.plan(&edit);
    let original = fixture.bytes();
    let rows = exchange(&[
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"request":edit}),
        ),
        request("project.open", json!({"path":fixture.root})),
        request(
            "project.source_lifecycle_preview",
            json!({"project_id":"p1","request":edit}),
        ),
    ]);
    for index in [0, 2] {
        assert_eq!(rows[index]["result"]["ok"], true, "{}", rows[index]);
        assert_eq!(
            rows[index]["result"]["plan"],
            serde_json::to_value(&plan).unwrap()
        );
        assert_eq!(rows[index]["result"]["saved"], false);
    }
    assert_eq!(fixture.bytes(), original);
    let rows = exchange(&[
        request("project.open", json!({"path":fixture.root})),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit,"plan_digest":"wrong"}),
        ),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit,"plan_digest":plan.plan_digest}),
        ),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit,"plan_digest":plan.plan_digest}),
        ),
        request("project.analyze", json!({"project_id":"p1"})),
    ]);
    assert!(rows.iter().all(|row| row.get("error").is_none()));
    assert_eq!(rows[1]["result"]["ok"], false);
    assert_eq!(rows[1]["result"]["baseline"], rows[0]["result"]["baseline"]);
    assert_eq!(rows[2]["result"]["ok"], true, "{}", rows[2]);
    assert_eq!(
        rows[2]["result"]["plan"],
        serde_json::to_value(plan).unwrap()
    );
    assert_eq!(rows[2]["result"]["applied"], true);
    assert_eq!(rows[2]["result"]["saved"], true);
    assert_eq!(rows[3]["result"]["ok"], false);
    assert_eq!(rows[3]["result"]["baseline"], rows[2]["result"]["baseline"]);
    assert_eq!(
        rows[4]["result"]["baseline"],
        fixture.project().content_baseline()
    );
    assert!(fixture.root.join("章节/新 章.wl").is_file());
    assert!(!fixture.project().compile().has_errors());
}

#[test]
fn rpc_source_lifecycle_move_and_include_project_the_core_plan() {
    for (label, edit) in [
        (
            "move",
            json!({"operation":"move","from":"old.wl","to":"章节/中文 空间/新章.wl"}),
        ),
        (
            "include",
            json!({"operation":"include","path":"archived.wl"}),
        ),
    ] {
        let fixture = Fixture::new(&format!("rpc-{label}"), false);
        let plan = fixture.plan(&edit);
        let rows = exchange(&[
            request(
                "project.source_lifecycle_preview",
                json!({"path":fixture.root,"request":edit}),
            ),
            request(
                "project.source_lifecycle_apply",
                json!({"path":fixture.root,"request":edit,"plan_digest":plan.plan_digest}),
            ),
        ]);
        for row in rows {
            assert_eq!(row["result"]["ok"], true, "{row}");
            assert_eq!(row["result"]["plan"], serde_json::to_value(&plan).unwrap());
        }
        assert!(!fixture.project().compile().has_errors());
        if label == "move" {
            assert!(!fixture.root.join("old.wl").exists());
            assert!(fixture.root.join("章节/中文 空间/新章.wl").is_file());
        } else {
            assert!(std::fs::read_to_string(fixture.root.join("world.wl"))
                .unwrap()
                .contains("archived.wl"));
        }
    }
}

#[test]
fn rpc_source_lifecycle_business_errors_remain_results_and_do_not_write() {
    let fixture = Fixture::new("rpc-rejections", true);
    let original = fixture.bytes();
    let rows = exchange(&[
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"request":{"operation":"include","path":"archived.wl"}}),
        ),
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"request":{"operation":"move","from":"world.wl","to":"other.wl"}}),
        ),
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"request":{"operation":"create","path":"../escape.wl"}}),
        ),
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"request":{"operation":"create","path":"old.wl"}}),
        ),
    ]);
    for row in rows {
        assert!(row.get("error").is_none(), "{row}");
        assert_eq!(row["result"]["ok"], false);
        assert_eq!(row["result"]["error"]["code"], "SOURCE_LIFECYCLE_REJECTED");
        assert_eq!(row["result"]["applied"], false);
        assert_eq!(row["result"]["saved"], false);
    }
    assert_eq!(fixture.bytes(), original);
}

#[test]
fn rpc_source_lifecycle_invalid_shapes_are_protocol_errors() {
    let fixture = Fixture::new("rpc-shape", false);
    let original = fixture.bytes();
    let edit = json!({"operation":"create","path":"new.wl"});
    let previews = [
        json!([]),
        json!({"request":edit}),
        json!({"path":fixture.root,"project_id":"p1","request":edit}),
        json!({"path":23,"request":edit}),
        json!({"path":"","request":edit}),
        json!({"project_id":"unknown","request":edit}),
        json!({"path":fixture.root,"request":edit,"unknown":true}),
        json!({"path":fixture.root,"request":edit,"plan_digest":"unexpected"}),
        json!({"path":fixture.root,"request":{"operation":"create","path":"new.wl","unknown":true}}),
        json!({"path":fixture.root,"request":{"operation":"move","from":"old.wl"}}),
        json!({"path":fixture.root,"request":{"operation":"delete","path":"old.wl"}}),
        json!({"path":fixture.root,"request":{"operation":"create","path":42}}),
        json!({"path":fixture.root,"request":null}),
    ];
    let mut requests = previews
        .into_iter()
        .map(|params| request("project.source_lifecycle_preview", params))
        .collect::<Vec<_>>();
    for digest in [None, Some(json!(false)), Some(json!(" "))] {
        let mut params = json!({"path":fixture.root,"request":edit});
        if let Some(digest) = digest {
            params["plan_digest"] = digest;
        }
        requests.push(request("project.source_lifecycle_apply", params));
    }
    for row in exchange(&requests) {
        assert_eq!(row["error"]["code"], -32602, "{row}");
        assert!(row.get("result").is_none());
    }
    let duplicate = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"project.source_lifecycle_preview\",\"params\":{{\"path\":{},\"request\":{{\"operation\":\"create\",\"path\":\"a.wl\",\"path\":\"b.wl\"}}}}}}\n",
        serde_json::to_string(&fixture.root).unwrap()
    );
    let mut output = Vec::new();
    worldline_agent::run(&mut Cursor::new(duplicate), &mut output);
    assert_eq!(responses(&output)[0]["error"]["code"], -32700);
    assert_eq!(fixture.bytes(), original);
}

#[test]
fn rpc_source_lifecycle_external_changes_reject_reviewed_plan() {
    let fixture = Fixture::new("rpc-external", true);
    let edit = json!({"operation":"create","path":"new.wl"});
    let plan = fixture.plan(&edit);
    std::fs::write(fixture.root.join("old.wl"), "character changed\n").unwrap();
    let changed = fixture.bytes();
    let rows = exchange(&[request(
        "project.source_lifecycle_apply",
        json!({
            "path":fixture.root,"request":edit,"plan_digest":plan.plan_digest
        }),
    )]);
    assert_eq!(rows[0]["result"]["ok"], false, "{}", rows[0]);
    assert_eq!(
        rows[0]["result"]["error"]["code"],
        "SOURCE_LIFECYCLE_REJECTED"
    );
    assert_eq!(fixture.bytes(), changed);
}

#[test]
fn rpc_source_lifecycle_save_failure_keeps_session_candidate_and_recoverable_journal() {
    let fixture = Fixture::new("rpc-save-failure", false);
    let original = fixture.bytes();
    let edit = json!({"operation":"create","path":"章节/新章.wl"});
    let plan = fixture.plan(&edit);
    let mut candidate = fixture.project();
    candidate
        .apply_source_lifecycle(&fixture.request(&edit), &plan.plan_digest)
        .unwrap();
    let expected_baseline = candidate.content_baseline();
    let requests = [
        request("project.open", json!({"path":fixture.root})),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit,"plan_digest":plan.plan_digest}),
        ),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit,"plan_digest":"wrong"}),
        ),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_wl-agent"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("WORLDLINE_SAVE_FAIL_PHASE", "middle")
        .env_remove("WORLDLINE_SAVE_FAIL_THREAD")
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(lines(&requests).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let rows = responses(&output.stdout);
    assert_eq!(rows.len(), 3);
    let failure = &rows[1]["result"];
    assert_eq!(failure["ok"], false, "{failure}");
    assert_eq!(failure["applied"], true);
    assert_eq!(failure["saved"], false);
    assert_eq!(failure["error"]["stage"], "save");
    assert_eq!(failure["baseline"], expected_baseline);
    assert_eq!(failure["plan"], serde_json::to_value(plan).unwrap());
    assert_eq!(rows[2]["result"]["ok"], false);
    assert_eq!(rows[2]["result"]["baseline"], expected_baseline);
    assert_ne!(rows[0]["result"]["baseline"], expected_baseline);
    assert_eq!(fixture.journal_count(), 1);
    assert_ne!(fixture.bytes(), original);
    assert!(!fixture.root.join("章节/新章.wl").exists());
    assert_eq!(fixture.project().content_baseline(), expected_baseline);
    assert!(fixture.root.join("章节/新章.wl").is_file());
    assert_eq!(fixture.journal_count(), 0);
}

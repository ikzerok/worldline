//! RPC 的 move_entity 与 core 一致，故事拒绝和协议形状错误清楚分离。
#[path = "../../core/tests/support/entity_source_move_fixture.rs"]
mod fixture;
use fixture::*;
use serde_json::{json, Value};
use std::io::{Cursor, Write};
use std::process::{Command, Stdio};

fn request(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
}
fn edit(to: &str) -> Value {
    json!({"operation":"move_entity","id":ID,"to":to})
}
fn lines(requests: &[Value]) -> String {
    requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
fn rows(output: &[u8]) -> Vec<Value> {
    std::str::from_utf8(output)
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
    rows(&output)
}
fn process(requests: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wl-agent"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(lines(requests).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "机器进程失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    rows(&output.stdout)
}

#[test]
fn real_rpc_path_and_project_preview_apply_save_and_reopen_match_core() {
    let fixture = Fixture::full();
    let plan = fixture.plan();
    let original = fixture.bytes();
    let previews = process(&[
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"request":edit(TARGET)}),
        ),
        request("project.open", json!({"path":fixture.root})),
        request(
            "project.source_lifecycle_preview",
            json!({"project_id":"p1","request":edit(TARGET)}),
        ),
    ]);
    for index in [0, 2] {
        assert_eq!(previews[index]["result"]["ok"], true, "{}", previews[index]);
        assert_eq!(
            previews[index]["result"]["plan"],
            serde_json::to_value(&plan).unwrap()
        );
        assert_eq!(previews[index]["result"]["saved"], false);
    }
    assert_eq!(fixture.bytes(), original);
    let responses = process(&[
        request("project.open", json!({"path":fixture.root})),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit(TARGET),"plan_digest":"forged"}),
        ),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit(TARGET),"plan_digest":plan.plan_digest}),
        ),
        request(
            "project.source_lifecycle_apply",
            json!({"project_id":"p1","request":edit(TARGET),"plan_digest":plan.plan_digest}),
        ),
        request("project.analyze", json!({"project_id":"p1"})),
    ]);
    assert!(responses.iter().all(|row| row.get("error").is_none()));
    assert_eq!(responses[1]["result"]["ok"], false);
    assert_eq!(
        responses[1]["result"]["baseline"],
        responses[0]["result"]["baseline"]
    );
    assert_eq!(responses[2]["result"]["ok"], true, "{}", responses[2]);
    assert_eq!(
        responses[2]["result"]["plan"],
        serde_json::to_value(&plan).unwrap()
    );
    assert_eq!(responses[2]["result"]["applied"], true);
    assert_eq!(responses[2]["result"]["saved"], true);
    assert_eq!(responses[3]["result"]["ok"], false);
    assert_eq!(
        responses[3]["result"]["baseline"],
        responses[2]["result"]["baseline"]
    );
    let mut reopened = fixture.project();
    assert_eq!(
        responses[4]["result"]["baseline"],
        reopened.content_baseline()
    );
    let compiled = reopened.compile();
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    assert_eq!(
        compiled.analysis.catalog.entities[ID].file,
        fixture.root.join(TARGET).to_string_lossy()
    );
    assert_eq!(compiled.analysis.catalog.states["lamp"].target.id, ID);
    assert_eq!(
        compiled.analysis.fingerprint,
        plan.runtime_fingerprint_before
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(SOURCE)).unwrap(),
        format!("{PREFIX}{SUFFIX}")
    );
    assert!(std::fs::read_to_string(fixture.root.join(TARGET))
        .unwrap()
        .ends_with(DECLARATION));
}

#[test]
fn rpc_business_rejections_are_results_with_zero_disk_mutation() {
    let fixture = Fixture::simple();
    let original = fixture.bytes();
    let requests = [
        edit("archive.wl"),
        edit("inactive.wl"),
        edit("missing.wl"),
        edit("../escape.wl"),
        json!({"operation":"move_entity","id":"start","to":TARGET}),
        json!({"operation":"move_entity","id":"missing","to":TARGET}),
    ]
    .into_iter()
    .map(|request_edit| {
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"request":request_edit}),
        )
    })
    .collect::<Vec<_>>();
    for response in exchange(&requests) {
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["ok"], false);
        assert_eq!(
            response["result"]["error"]["code"],
            "SOURCE_LIFECYCLE_REJECTED"
        );
        assert_eq!(response["result"]["applied"], false);
        assert_eq!(response["result"]["saved"], false);
    }
    assert_eq!(fixture.bytes(), original);
}

#[test]
fn rpc_bad_move_entity_shapes_are_protocol_errors_before_any_mutation() {
    let fixture = Fixture::simple();
    let original = fixture.bytes();
    let bad_requests = [
        json!({"operation":"move_entity","to":TARGET}),
        json!({"operation":"move_entity","id":23,"to":TARGET}),
        json!({"operation":"move_entity","id":ID}),
        json!({"operation":"move_entity","id":ID,"to":false}),
        json!({"operation":"move_entity","id":ID,"to":TARGET,"from":SOURCE}),
        json!({"operation":"move_entity","id":ID,"to":TARGET,"unknown":true}),
    ];
    let mut requests = bad_requests
        .into_iter()
        .map(|request_edit| {
            request(
                "project.source_lifecycle_preview",
                json!({"path":fixture.root,"request":request_edit}),
            )
        })
        .collect::<Vec<_>>();
    requests.extend([
        request(
            "project.source_lifecycle_preview",
            json!({"path":fixture.root,"project_id":"p1","request":edit(TARGET)}),
        ),
        request(
            "project.source_lifecycle_apply",
            json!({"path":fixture.root,"request":edit(TARGET)}),
        ),
        request(
            "project.source_lifecycle_apply",
            json!({"path":fixture.root,"request":edit(TARGET),"plan_digest":false}),
        ),
    ]);
    for response in exchange(&requests) {
        assert!(response.get("result").is_none(), "{response}");
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    assert_eq!(fixture.bytes(), original);
}

#[test]
fn rpc_apply_rejects_disk_changes_since_preview_without_overwriting_them() {
    let fixture = Fixture::simple();
    let plan = fixture.plan();
    fixture.write(
        SOURCE,
        "// 作者外部编辑\nentity north_lighthouse kind place\n",
    );
    let changed = fixture.bytes();
    let response = exchange(&[request(
        "project.source_lifecycle_apply",
        json!({"path":fixture.root,"request":edit(TARGET),"plan_digest":plan.plan_digest}),
    )])
    .remove(0);
    assert!(response.get("error").is_none());
    assert_eq!(response["result"]["ok"], false);
    assert_eq!(
        response["result"]["error"]["code"],
        "SOURCE_LIFECYCLE_REJECTED"
    );
    assert_eq!(fixture.bytes(), changed);
}

#[test]
fn rpc_same_source_apply_reports_no_change_without_saving() {
    let fixture = Fixture::simple();
    let original = fixture.bytes();
    let preview = process(&[request(
        "project.source_lifecycle_preview",
        json!({"path":fixture.root,"request":edit(SOURCE)}),
    )])
    .remove(0);
    assert_eq!(preview["result"]["ok"], true, "{preview}");
    assert_eq!(preview["result"]["plan"]["changes"], json!([]));
    let digest = preview["result"]["plan"]["plan_digest"].as_str().unwrap();
    let applied = process(&[request(
        "project.source_lifecycle_apply",
        json!({"path":fixture.root,"request":edit(SOURCE),"plan_digest":digest}),
    )])
    .remove(0);
    assert_eq!(applied["result"]["ok"], true, "{applied}");
    assert_eq!(applied["result"]["applied"], false);
    assert_eq!(applied["result"]["saved"], false);
    assert_eq!(applied["result"]["plan"], preview["result"]["plan"]);
    assert_eq!(fixture.bytes(), original);
}

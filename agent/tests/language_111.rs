//! 1.11 机器接口必须使用相同编译器及结构化台词输出。
use serde_json::{json, Value};
fn exchange(requests: Vec<Value>) -> Vec<Value> {
    let input = requests
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Vec::new();
    assert_eq!(
        worldline_agent::run(&mut std::io::Cursor::new(input), &mut output),
        0
    );
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
#[test]
fn explicit_language_and_speaker_survive_rpc() {
    let source="character doctor\nrule fee(n: num) -> num = n * 2\nfragment explain(x: num)\n  say doctor \"费用{fee(x)}\" direction \"私密备注\"\n  return\nevent start\n  call explain(3)\n  -> END\n";
    let rows = exchange(vec![
        request(
            1,
            "compile",
            json!({"source":source,"language_version":"1.11"}),
        ),
        request(2, "session.open", json!({"story_id":"s1","seed":31})),
        request(3, "session.continue", json!({"session_id":"c1"})),
    ]);
    assert_eq!(rows[0]["result"]["ok"], true, "{}", rows[0]);
    assert_eq!(rows[0]["result"]["language_version"], "1.11");
    assert_eq!(
        rows[2]["result"]["outputs"][0]["content"], "费用6",
        "{}",
        rows[2]
    );
    assert_eq!(rows[2]["result"]["outputs"][0]["speaker"]["id"], "doctor");
    assert!(!rows.iter().any(|r| r.to_string().contains("私密备注")));
}

#[test]
fn source_edit_rpc_preview_apply_and_stale_rejection_share_core_transaction() {
    use worldline_core::{project::Project, source_edit::SourceEditRequest};
    let root = std::env::temp_dir().join(format!("source-edit-rpc-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".world")).unwrap();
    std::fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"language_version":"1.11","required_features":[]}"#,
    )
    .unwrap();
    std::fs::write(root.join("world.wl"), "event start\n  原文\n  -> END\n").unwrap();
    let project = Project::open(&root).unwrap();
    let edit = SourceEditRequest {
        schema_version: 1,
        path: "world.wl".into(),
        expected_baseline: project.content_baseline(),
        source: "rule cost(n: num) -> num = n * 2\nevent start\n  {cost(3)}\n  -> END\n".into(),
    };
    let plan = project.preview_source_edit(&edit).unwrap();
    let rows = exchange(vec![
        request(
            1,
            "source.edit.preview",
            json!({"path":root,"request":edit}),
        ),
        request(
            2,
            "source.edit.apply",
            json!({"path":root,"request":edit,"plan_digest":"wrong"}),
        ),
        request(
            3,
            "source.edit.apply",
            json!({"path":root,"request":edit,"plan_digest":plan.plan_digest}),
        ),
        request(
            4,
            "source.edit.apply",
            json!({"path":root,"request":edit,"plan_digest":plan.plan_digest}),
        ),
    ]);
    assert_eq!(rows[0]["result"]["ok"], true, "{}", rows[0]);
    assert_eq!(rows[1]["result"]["ok"], false);
    assert_eq!(rows[2]["result"]["ok"], true, "{}", rows[2]);
    assert_eq!(rows[3]["result"]["ok"], false);
    assert!(rows.iter().all(|r| r.get("error").is_none()));
    assert_eq!(
        std::fs::read_to_string(root.join("world.wl")).unwrap(),
        edit.source
    );
    std::fs::remove_dir_all(root).unwrap();
}
